use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use reqwest::{header::CONTENT_TYPE, redirect::Policy, Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::{AudioArtifact, DeliveryStyle, TextToSpeechProvider, TtsError, VoiceSettings};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_WAV_BYTES: usize = 20 * 1024 * 1024;
const SUPPORTED_SPEAKERS: [&str; 5] = ["Jon", "Lea", "Gary", "Jenna", "Mike"];

#[derive(Clone, Debug)]
pub struct ParlerConfiguration {
    base_url: Url,
    output_directory: PathBuf,
    speaker: String,
}

impl ParlerConfiguration {
    pub fn new(base_url: &str, output_directory: &Path, speaker: &str) -> Result<Self, TtsError> {
        let base_url = validate_local_base_url(base_url)?;
        if !is_supported_speaker(speaker) {
            return Err(TtsError::InvalidConfiguration);
        }
        fs::create_dir_all(output_directory).map_err(|_| TtsError::InvalidConfiguration)?;
        let output_directory = output_directory
            .canonicalize()
            .map_err(|_| TtsError::InvalidConfiguration)?;
        if !output_directory.is_dir() {
            return Err(TtsError::InvalidConfiguration);
        }
        Ok(Self {
            base_url,
            output_directory,
            speaker: speaker.to_owned(),
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, TtsError> {
        self.base_url
            .join(path)
            .map_err(|_| TtsError::InvalidConfiguration)
    }
}

pub fn validate_local_base_url(value: &str) -> Result<Url, TtsError> {
    let mut url = Url::parse(value).map_err(|_| TtsError::InvalidConfiguration)?;
    let is_loopback = url
        .host_str()
        .and_then(|host| host.parse::<std::net::IpAddr>().ok())
        .is_some_and(|address| address.is_loopback());
    if url.scheme() != "http"
        || !is_loopback
        || url.port().is_none()
        || (url.path() != "/" && !url.path().is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(TtsError::InvalidConfiguration);
    }
    url.set_path("/");
    Ok(url)
}

pub fn is_supported_speaker(speaker: &str) -> bool {
    SUPPORTED_SPEAKERS.contains(&speaker)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParlerHealth {
    pub ready: bool,
    pub provider: String,
    pub model: String,
    pub sample_rate: u32,
    pub speakers: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SynthesisRequest<'a> {
    text: &'a str,
    speaker: &'a str,
    delivery_style: DeliveryStyle,
    rate: f32,
    volume: f32,
}

#[derive(Clone)]
pub struct ParlerMiniTtsProvider {
    configuration: ParlerConfiguration,
    client: Client,
}

impl ParlerMiniTtsProvider {
    pub fn new(configuration: ParlerConfiguration) -> Result<Self, TtsError> {
        Self::with_timeout(configuration, REQUEST_TIMEOUT)
    }

    fn with_timeout(
        configuration: ParlerConfiguration,
        request_timeout: Duration,
    ) -> Result<Self, TtsError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(request_timeout)
            .build()
            .map_err(|_| TtsError::InvalidConfiguration)?;
        Ok(Self {
            configuration,
            client,
        })
    }

    pub async fn health_check(&self) -> Result<ParlerHealth, TtsError> {
        let response = self
            .client
            .get(self.configuration.endpoint("health")?)
            .send()
            .await
            .map_err(map_request_error)?;
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        let health: ParlerHealth = response
            .json()
            .await
            .map_err(|_| TtsError::InvalidArtifact)?;
        if !health.ready
            || health.provider != "parler_tts_mini"
            || health.model != "parler-tts-mini-v1"
            || !(8_000..=192_000).contains(&health.sample_rate)
            || !health
                .speakers
                .iter()
                .any(|speaker| speaker == &self.configuration.speaker)
        {
            return Err(TtsError::InvalidArtifact);
        }
        Ok(health)
    }

    async fn receive_wav(
        &self,
        text: &str,
        settings: &VoiceSettings,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, TtsError> {
        validate_request(text, settings, &self.configuration.speaker)?;
        let request = SynthesisRequest {
            text,
            speaker: &self.configuration.speaker,
            delivery_style: settings.delivery_style,
            rate: settings.rate,
            volume: settings.volume,
        };
        let send = self
            .client
            .post(self.configuration.endpoint("synthesize")?)
            .json(&request)
            .send();
        let mut response = tokio::select! {
            _ = cancellation.cancelled() => return Err(TtsError::Cancelled),
            result = send => result.map_err(map_request_error)?,
        };
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type.starts_with("audio/wav") && !content_type.starts_with("audio/x-wav") {
            return Err(TtsError::InvalidArtifact);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_WAV_BYTES as u64)
        {
            return Err(TtsError::InvalidArtifact);
        }
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = cancellation.cancelled() => return Err(TtsError::Cancelled),
                result = response.chunk() => result.map_err(map_request_error)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            if bytes.len().saturating_add(chunk.len()) > MAX_WAV_BYTES {
                return Err(TtsError::InvalidArtifact);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

#[async_trait]
impl TextToSpeechProvider for ParlerMiniTtsProvider {
    async fn synthesize(
        &self,
        text: &str,
        settings: &VoiceSettings,
        cancellation: CancellationToken,
    ) -> Result<AudioArtifact, TtsError> {
        let bytes = self.receive_wav(text, settings, &cancellation).await?;
        if cancellation.is_cancelled() {
            return Err(TtsError::Cancelled);
        }
        let duration_ms = validate_wav(&bytes)?;
        let artifact_id = Uuid::new_v4().to_string();
        let final_path = self
            .configuration
            .output_directory
            .join(format!("{artifact_id}.wav"));
        let temporary_path = self
            .configuration
            .output_directory
            .join(format!("{artifact_id}.tmp"));
        write_atomic(&temporary_path, &final_path, &bytes)?;
        if cancellation.is_cancelled() {
            let _ = fs::remove_file(&final_path);
            return Err(TtsError::Cancelled);
        }
        Ok(AudioArtifact {
            artifact_id,
            local_path: Some(path_string(&final_path)?),
            duration_ms: Some(duration_ms),
            is_mock: false,
        })
    }

    async fn cancel(&self) -> Result<(), TtsError> {
        Ok(())
    }
}

fn validate_request(text: &str, settings: &VoiceSettings, speaker: &str) -> Result<(), TtsError> {
    if text.trim().is_empty()
        || text.len() > 4_000
        || text.contains('\0')
        || settings.voice_id != speaker
        || !settings.rate.is_finite()
        || !(0.5..=2.0).contains(&settings.rate)
        || !settings.volume.is_finite()
        || !(0.0..=1.0).contains(&settings.volume)
    {
        Err(TtsError::InvalidConfiguration)
    } else {
        Ok(())
    }
}

fn validate_wav(bytes: &[u8]) -> Result<u64, TtsError> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(TtsError::InvalidArtifact);
    }
    let declared = read_u32(bytes, 4)? as usize;
    if declared.checked_add(8) != Some(bytes.len()) {
        return Err(TtsError::InvalidArtifact);
    }
    let mut offset = 12_usize;
    let mut format = None;
    let mut data = None;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[offset..offset + 4];
        let length = read_u32(bytes, offset + 4)? as usize;
        let start = offset + 8;
        let end = start.checked_add(length).ok_or(TtsError::InvalidArtifact)?;
        if end > bytes.len() {
            return Err(TtsError::InvalidArtifact);
        }
        if id == b"fmt " {
            if length < 16 {
                return Err(TtsError::InvalidArtifact);
            }
            format = Some((
                read_u16(bytes, start)?,
                read_u16(bytes, start + 2)?,
                read_u32(bytes, start + 4)?,
                read_u16(bytes, start + 14)?,
            ));
        } else if id == b"data" {
            data = Some(length);
        }
        offset = end + (length % 2);
    }
    let (audio_format, channels, sample_rate, bits) = format.ok_or(TtsError::InvalidArtifact)?;
    let data_length = data.ok_or(TtsError::InvalidArtifact)?;
    if audio_format != 1
        || channels != 1
        || !(8_000..=192_000).contains(&sample_rate)
        || bits != 16
        || data_length == 0
        || data_length % 2 != 0
    {
        return Err(TtsError::InvalidArtifact);
    }
    let samples = u64::try_from(data_length / 2).map_err(|_| TtsError::InvalidArtifact)?;
    samples
        .checked_mul(1_000)
        .map(|value| value / u64::from(sample_rate))
        .filter(|duration| *duration > 0 && *duration <= 300_000)
        .ok_or(TtsError::InvalidArtifact)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, TtsError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(TtsError::InvalidArtifact)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, TtsError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(TtsError::InvalidArtifact)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn write_atomic(temporary: &Path, final_path: &Path, bytes: &[u8]) -> Result<(), TtsError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|_| TtsError::InvalidArtifact)?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        let _ = fs::remove_file(temporary);
        return Err(TtsError::InvalidArtifact);
    }
    fs::rename(temporary, final_path).map_err(|_| {
        let _ = fs::remove_file(temporary);
        TtsError::InvalidArtifact
    })
}

fn path_string(path: &Path) -> Result<String, TtsError> {
    path.to_str()
        .filter(|value| !value.contains('\0'))
        .map(str::to_owned)
        .ok_or(TtsError::InvalidArtifact)
}

fn map_request_error(error: reqwest::Error) -> TtsError {
    if error.is_timeout() || error.is_connect() {
        TtsError::Unavailable
    } else {
        TtsError::SynthesisFailed
    }
}

fn map_status(status: StatusCode) -> TtsError {
    if status == StatusCode::BAD_REQUEST {
        TtsError::InvalidConfiguration
    } else if status == StatusCode::SERVICE_UNAVAILABLE {
        TtsError::Unavailable
    } else {
        TtsError::SynthesisFailed
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use super::*;

    fn voice() -> VoiceSettings {
        VoiceSettings {
            voice_id: "Jon".into(),
            rate: 1.0,
            volume: 1.0,
            delivery_style: DeliveryStyle::Energetic,
        }
    }

    fn wav() -> Vec<u8> {
        let samples = vec![0_i16; 44_100];
        let data_size = (samples.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_size as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&44_100_u32.to_le_bytes());
        bytes.extend_from_slice(&88_200_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    async fn provider(server: &MockServer, output: &TempDir) -> ParlerMiniTtsProvider {
        let configuration = ParlerConfiguration::new(&server.uri(), output.path(), "Jon")
            .unwrap_or_else(|error| panic!("configuration failed: {error}"));
        ParlerMiniTtsProvider::new(configuration)
            .unwrap_or_else(|error| panic!("provider failed: {error}"))
    }

    #[test]
    fn rejects_non_loopback_and_unknown_speaker() {
        let output = TempDir::new().unwrap_or_else(|error| panic!("temp failed: {error}"));
        assert!(
            ParlerConfiguration::new("https://example.com:43822", output.path(), "Jon").is_err()
        );
        assert!(
            ParlerConfiguration::new("http://127.0.0.1:43822", output.path(), "Other").is_err()
        );
    }

    #[tokio::test]
    async fn health_check_validates_model_and_speaker() {
        let server = MockServer::start().await;
        let output = TempDir::new().unwrap_or_else(|error| panic!("temp failed: {error}"));
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ready": true,
                "provider": "parler_tts_mini",
                "model": "parler-tts-mini-v1",
                "sampleRate": 44100,
                "speakers": ["Jon", "Gary"]
            })))
            .mount(&server)
            .await;

        let health = provider(&server, &output)
            .await
            .health_check()
            .await
            .unwrap_or_else(|error| panic!("health failed: {error}"));
        assert!(health.ready);
        assert_eq!(health.sample_rate, 44_100);
    }

    #[tokio::test]
    async fn synthesis_sends_only_bounded_voice_input_and_writes_valid_wav() {
        let server = MockServer::start().await;
        let output = TempDir::new().unwrap_or_else(|error| panic!("temp failed: {error}"));
        Mock::given(method("POST"))
            .and(path("/synthesize"))
            .and(body_json(serde_json::json!({
                "text": "A short radio line.",
                "speaker": "Jon",
                "deliveryStyle": "energetic",
                "rate": 1.0,
                "volume": 1.0
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/wav")
                    .set_body_bytes(wav()),
            )
            .mount(&server)
            .await;

        let artifact = provider(&server, &output)
            .await
            .synthesize("A short radio line.", &voice(), CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("synthesis failed: {error}"));
        assert_eq!(artifact.duration_ms, Some(1_000));
        assert!(artifact
            .local_path
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file()));
    }

    #[tokio::test]
    async fn cancellation_aborts_a_slow_request_without_artifact() {
        let server = MockServer::start().await;
        let output = TempDir::new().unwrap_or_else(|error| panic!("temp failed: {error}"));
        Mock::given(method("POST"))
            .and(path("/synthesize"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .insert_header("content-type", "audio/wav")
                    .set_body_bytes(wav()),
            )
            .mount(&server)
            .await;
        let cancellation = CancellationToken::new();
        let token = cancellation.clone();
        let provider = provider(&server, &output).await;
        let task = tokio::spawn(async move {
            provider
                .synthesize("A short radio line.", &voice(), token)
                .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancellation.cancel();
        let result = task
            .await
            .unwrap_or_else(|error| panic!("task failed: {error}"));
        assert!(matches!(result, Err(TtsError::Cancelled)));
        assert_eq!(
            fs::read_dir(output.path())
                .unwrap_or_else(|error| panic!("read failed: {error}"))
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn rejects_malformed_audio_without_leaving_an_artifact() {
        let server = MockServer::start().await;
        let output = TempDir::new().unwrap_or_else(|error| panic!("temp failed: {error}"));
        Mock::given(method("POST"))
            .and(path("/synthesize"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/wav")
                    .set_body_bytes(b"not a wav"),
            )
            .mount(&server)
            .await;

        let result = provider(&server, &output)
            .await
            .synthesize("A short radio line.", &voice(), CancellationToken::new())
            .await;
        assert!(matches!(result, Err(TtsError::InvalidArtifact)));
        assert_eq!(
            fs::read_dir(output.path())
                .unwrap_or_else(|error| panic!("read failed: {error}"))
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn request_deadline_maps_to_typed_unavailable_error() {
        let server = MockServer::start().await;
        let output = TempDir::new().unwrap_or_else(|error| panic!("temp failed: {error}"));
        Mock::given(method("POST"))
            .and(path("/synthesize"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .insert_header("content-type", "audio/wav")
                    .set_body_bytes(wav()),
            )
            .mount(&server)
            .await;
        let configuration = ParlerConfiguration::new(&server.uri(), output.path(), "Jon")
            .unwrap_or_else(|error| panic!("configuration failed: {error}"));
        let provider =
            ParlerMiniTtsProvider::with_timeout(configuration, Duration::from_millis(20))
                .unwrap_or_else(|error| panic!("provider failed: {error}"));

        let result = provider
            .synthesize("A short radio line.", &voice(), CancellationToken::new())
            .await;
        assert!(matches!(result, Err(TtsError::Unavailable)));
    }
}
