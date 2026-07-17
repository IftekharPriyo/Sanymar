use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use serde::Serialize;
use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AudioArtifact, DeliveryStyle, TextToSpeechProvider, TtsError, VoiceSettings};

const MODEL_FILE: &str = "model.onnx";
const VOICES_FILE: &str = "voices.bin";
const TOKENS_FILE: &str = "tokens.txt";
const DATA_DIRECTORY: &str = "espeak-ng-data";

#[derive(Clone, Debug)]
pub struct SherpaKokoroConfiguration {
    model: PathBuf,
    voices: PathBuf,
    tokens: PathBuf,
    data_directory: PathBuf,
    output_directory: PathBuf,
}

impl SherpaKokoroConfiguration {
    pub fn new(
        model_directory: impl AsRef<Path>,
        output_directory: &Path,
    ) -> Result<Self, TtsError> {
        let model_directory = canonical_directory(model_directory.as_ref())?;
        fs::create_dir_all(output_directory).map_err(|_| TtsError::InvalidConfiguration)?;
        let output_directory = canonical_directory(output_directory)?;
        Ok(Self {
            model: canonical_child_file(&model_directory, MODEL_FILE)?,
            voices: canonical_child_file(&model_directory, VOICES_FILE)?,
            tokens: canonical_child_file(&model_directory, TOKENS_FILE)?,
            data_directory: canonical_child_directory(&model_directory, DATA_DIRECTORY)?,
            output_directory,
        })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SherpaTtsHealth {
    pub ready: bool,
    pub sample_rate: u32,
    pub available_voices: u16,
}

struct GeneratedSamples {
    samples: Vec<f32>,
    sample_rate: u32,
}

trait SynthesisBackend: Send + Sync {
    fn health(&self) -> Result<SherpaTtsHealth, TtsError>;
    fn synthesize(
        &self,
        text: &str,
        voice_id: u16,
        speed: f32,
        cancellation: CancellationToken,
    ) -> Result<GeneratedSamples, TtsError>;
}

struct SherpaBackend {
    engine: OfflineTts,
}

impl SherpaBackend {
    fn new(configuration: &SherpaKokoroConfiguration) -> Result<Self, TtsError> {
        let model = path_string(&configuration.model)?;
        let voices = path_string(&configuration.voices)?;
        let tokens = path_string(&configuration.tokens)?;
        let data_directory = path_string(&configuration.data_directory)?;
        let config = OfflineTtsConfig {
            model: OfflineTtsModelConfig {
                kokoro: OfflineTtsKokoroModelConfig {
                    model: Some(model),
                    voices: Some(voices),
                    tokens: Some(tokens),
                    data_dir: Some(data_directory),
                    ..Default::default()
                },
                num_threads: 2,
                provider: Some("cpu".into()),
                debug: false,
                ..Default::default()
            },
            max_num_sentences: 1,
            ..Default::default()
        };
        let engine = OfflineTts::create(&config).ok_or(TtsError::InvalidConfiguration)?;
        Ok(Self { engine })
    }
}

impl SynthesisBackend for SherpaBackend {
    fn health(&self) -> Result<SherpaTtsHealth, TtsError> {
        let sample_rate =
            u32::try_from(self.engine.sample_rate()).map_err(|_| TtsError::InvalidConfiguration)?;
        let available_voices = u16::try_from(self.engine.num_speakers())
            .map_err(|_| TtsError::InvalidConfiguration)?;
        if sample_rate == 0 || available_voices == 0 {
            return Err(TtsError::InvalidConfiguration);
        }
        Ok(SherpaTtsHealth {
            ready: true,
            sample_rate,
            available_voices,
        })
    }

    fn synthesize(
        &self,
        text: &str,
        voice_id: u16,
        speed: f32,
        cancellation: CancellationToken,
    ) -> Result<GeneratedSamples, TtsError> {
        let generation = GenerationConfig {
            sid: i32::from(voice_id),
            speed,
            ..Default::default()
        };
        let callback_cancellation = cancellation.clone();
        let audio = self
            .engine
            .generate_with_config(
                text,
                &generation,
                Some(move |_: &[f32], _: f32| !callback_cancellation.is_cancelled()),
            )
            .ok_or(TtsError::SynthesisFailed)?;
        if cancellation.is_cancelled() {
            return Err(TtsError::Cancelled);
        }
        let sample_rate =
            u32::try_from(audio.sample_rate()).map_err(|_| TtsError::InvalidArtifact)?;
        Ok(GeneratedSamples {
            samples: audio.samples().to_vec(),
            sample_rate,
        })
    }
}

pub struct SherpaKokoroTtsProvider {
    backend: Arc<dyn SynthesisBackend>,
    output_directory: PathBuf,
    active_cancellation: Arc<Mutex<Option<CancellationToken>>>,
}

impl SherpaKokoroTtsProvider {
    pub fn new(configuration: SherpaKokoroConfiguration) -> Result<Self, TtsError> {
        let backend = Arc::new(SherpaBackend::new(&configuration)?);
        Ok(Self::from_backend(backend, configuration.output_directory))
    }

    fn from_backend(backend: Arc<dyn SynthesisBackend>, output_directory: PathBuf) -> Self {
        Self {
            backend,
            output_directory,
            active_cancellation: Arc::new(Mutex::new(None)),
        }
    }

    pub fn health(&self) -> Result<SherpaTtsHealth, TtsError> {
        self.backend.health()
    }

    fn register_cancellation(&self) -> Result<CancellationToken, TtsError> {
        let token = CancellationToken::new();
        let mut active = self
            .active_cancellation
            .lock()
            .map_err(|_| TtsError::Unavailable)?;
        if let Some(previous) = active.replace(token.clone()) {
            previous.cancel();
        }
        Ok(token)
    }
}

#[async_trait]
impl TextToSpeechProvider for SherpaKokoroTtsProvider {
    async fn synthesize(
        &self,
        text: &str,
        settings: &VoiceSettings,
        cancellation: CancellationToken,
    ) -> Result<AudioArtifact, TtsError> {
        validate_request(text, settings, &self.health()?)?;
        let internal_cancellation = self.register_cancellation()?;
        let backend = self.backend.clone();
        let output_directory = self.output_directory.clone();
        let text = text.to_owned();
        let settings = settings.clone();
        tokio::task::spawn_blocking(move || {
            synthesize_to_artifact(
                backend,
                output_directory,
                &text,
                &settings,
                cancellation,
                internal_cancellation,
            )
        })
        .await
        .map_err(|_| TtsError::SynthesisFailed)?
    }

    async fn cancel(&self) -> Result<(), TtsError> {
        let mut active = self
            .active_cancellation
            .lock()
            .map_err(|_| TtsError::Unavailable)?;
        if let Some(token) = active.take() {
            token.cancel();
        }
        Ok(())
    }
}

fn synthesize_to_artifact(
    backend: Arc<dyn SynthesisBackend>,
    output_directory: PathBuf,
    text: &str,
    settings: &VoiceSettings,
    cancellation: CancellationToken,
    internal_cancellation: CancellationToken,
) -> Result<AudioArtifact, TtsError> {
    if cancellation.is_cancelled() || internal_cancellation.is_cancelled() {
        return Err(TtsError::Cancelled);
    }
    let combined = CancellationToken::new();
    let combined_for_external = combined.clone();
    let combined_for_internal = combined.clone();
    let external = cancellation.clone();
    let internal = internal_cancellation.clone();
    let cancellation_thread = std::thread::spawn(move || {
        while !combined_for_external.is_cancelled() {
            if external.is_cancelled() || internal.is_cancelled() {
                combined_for_internal.cancel();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    let voice_id = settings
        .voice_id
        .parse::<u16>()
        .map_err(|_| TtsError::InvalidConfiguration)?;
    let speed = delivery_rate(settings.rate, settings.delivery_style)?;
    let generated = backend.synthesize(text, voice_id, speed, combined.clone());
    combined.cancel();
    let _ = cancellation_thread.join();
    let mut generated = generated?;
    if cancellation.is_cancelled() || internal_cancellation.is_cancelled() {
        return Err(TtsError::Cancelled);
    }
    apply_volume(&mut generated.samples, settings.volume)?;
    validate_samples(&generated)?;
    let artifact_id = Uuid::new_v4().to_string();
    let final_path = output_directory.join(format!("{artifact_id}.wav"));
    let temporary_path = output_directory.join(format!("{artifact_id}.tmp"));
    write_wav(&temporary_path, &generated)?;
    if cancellation.is_cancelled() || internal_cancellation.is_cancelled() {
        let _ = fs::remove_file(&temporary_path);
        return Err(TtsError::Cancelled);
    }
    fs::rename(&temporary_path, &final_path).map_err(|_| TtsError::InvalidArtifact)?;
    validate_wav(&final_path, generated.sample_rate, generated.samples.len())?;
    let duration_ms = u64::try_from(generated.samples.len())
        .ok()
        .and_then(|samples| samples.checked_mul(1_000))
        .map(|value| value / u64::from(generated.sample_rate))
        .ok_or(TtsError::InvalidArtifact)?;
    Ok(AudioArtifact {
        artifact_id,
        local_path: Some(path_string(&final_path)?),
        duration_ms: Some(duration_ms),
        is_mock: false,
    })
}

fn validate_request(
    text: &str,
    settings: &VoiceSettings,
    health: &SherpaTtsHealth,
) -> Result<(), TtsError> {
    if text.trim().is_empty() || text.len() > 4_000 || text.contains('\0') {
        return Err(TtsError::InvalidConfiguration);
    }
    let voice_id = settings
        .voice_id
        .parse::<u16>()
        .map_err(|_| TtsError::InvalidConfiguration)?;
    if voice_id >= health.available_voices
        || !settings.rate.is_finite()
        || !(0.5..=2.0).contains(&settings.rate)
        || !settings.volume.is_finite()
        || !(0.0..=1.0).contains(&settings.volume)
    {
        return Err(TtsError::InvalidConfiguration);
    }
    Ok(())
}

fn delivery_rate(base_rate: f32, style: DeliveryStyle) -> Result<f32, TtsError> {
    if !base_rate.is_finite() || !(0.5..=2.0).contains(&base_rate) {
        return Err(TtsError::InvalidConfiguration);
    }
    let factor = match style {
        DeliveryStyle::Neutral => 1.0,
        DeliveryStyle::Warm => 0.98,
        DeliveryStyle::Energetic => 1.05,
        DeliveryStyle::Playful => 1.07,
        DeliveryStyle::Reflective => 0.94,
        DeliveryStyle::Authoritative => 0.96,
    };
    Ok((base_rate * factor).clamp(0.5, 2.0))
}

fn validate_samples(generated: &GeneratedSamples) -> Result<(), TtsError> {
    if generated.sample_rate < 8_000
        || generated.sample_rate > 192_000
        || generated.samples.is_empty()
        || generated.samples.len() > 100_000_000
        || generated.samples.iter().any(|sample| !sample.is_finite())
    {
        Err(TtsError::InvalidArtifact)
    } else {
        Ok(())
    }
}

fn apply_volume(samples: &mut [f32], volume: f32) -> Result<(), TtsError> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(TtsError::InvalidConfiguration);
    }
    for sample in samples {
        *sample = (*sample * volume).clamp(-1.0, 1.0);
    }
    Ok(())
}

fn write_wav(path: &Path, generated: &GeneratedSamples) -> Result<(), TtsError> {
    let data_size = generated
        .samples
        .len()
        .checked_mul(2)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(TtsError::InvalidArtifact)?;
    let riff_size = 36_u32
        .checked_add(data_size)
        .ok_or(TtsError::InvalidArtifact)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| TtsError::InvalidArtifact)?;
    file.write_all(b"RIFF")
        .and_then(|_| file.write_all(&riff_size.to_le_bytes()))
        .and_then(|_| file.write_all(b"WAVEfmt "))
        .and_then(|_| file.write_all(&16_u32.to_le_bytes()))
        .and_then(|_| file.write_all(&1_u16.to_le_bytes()))
        .and_then(|_| file.write_all(&1_u16.to_le_bytes()))
        .and_then(|_| file.write_all(&generated.sample_rate.to_le_bytes()))
        .and_then(|_| file.write_all(&(generated.sample_rate * 2).to_le_bytes()))
        .and_then(|_| file.write_all(&2_u16.to_le_bytes()))
        .and_then(|_| file.write_all(&16_u16.to_le_bytes()))
        .and_then(|_| file.write_all(b"data"))
        .and_then(|_| file.write_all(&data_size.to_le_bytes()))
        .map_err(|_| TtsError::InvalidArtifact)?;
    for sample in &generated.samples {
        let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        file.write_all(&pcm.to_le_bytes())
            .map_err(|_| TtsError::InvalidArtifact)?;
    }
    file.sync_all().map_err(|_| TtsError::InvalidArtifact)
}

fn validate_wav(path: &Path, sample_rate: u32, samples: usize) -> Result<(), TtsError> {
    let mut file = File::open(path).map_err(|_| TtsError::InvalidArtifact)?;
    let metadata = file.metadata().map_err(|_| TtsError::InvalidArtifact)?;
    let expected_length = 44_u64
        .checked_add(
            u64::try_from(samples)
                .map_err(|_| TtsError::InvalidArtifact)?
                .checked_mul(2)
                .ok_or(TtsError::InvalidArtifact)?,
        )
        .ok_or(TtsError::InvalidArtifact)?;
    let mut header = [0_u8; 44];
    file.read_exact(&mut header)
        .map_err(|_| TtsError::InvalidArtifact)?;
    if metadata.len() != expected_length
        || &header[0..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
        || &header[36..40] != b"data"
        || u32::from_le_bytes([header[24], header[25], header[26], header[27]]) != sample_rate
    {
        Err(TtsError::InvalidArtifact)
    } else {
        Ok(())
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, TtsError> {
    if !path.is_absolute() {
        return Err(TtsError::InvalidConfiguration);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| TtsError::InvalidConfiguration)?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(TtsError::InvalidConfiguration)
    }
}

fn canonical_child_file(parent: &Path, name: &str) -> Result<PathBuf, TtsError> {
    let path = parent
        .join(name)
        .canonicalize()
        .map_err(|_| TtsError::InvalidConfiguration)?;
    if path.starts_with(parent) && path.is_file() {
        Ok(path)
    } else {
        Err(TtsError::InvalidConfiguration)
    }
}

fn canonical_child_directory(parent: &Path, name: &str) -> Result<PathBuf, TtsError> {
    let path = parent
        .join(name)
        .canonicalize()
        .map_err(|_| TtsError::InvalidConfiguration)?;
    if path.starts_with(parent) && path.is_dir() {
        Ok(path)
    } else {
        Err(TtsError::InvalidConfiguration)
    }
}

fn path_string(path: &Path) -> Result<String, TtsError> {
    let value = path
        .to_str()
        .filter(|value| !value.contains('\0'))
        .map(str::to_owned)
        .ok_or(TtsError::InvalidConfiguration)?;
    Ok(normalize_windows_verbatim_path(value))
}

#[cfg(windows)]
fn normalize_windows_verbatim_path(value: String) -> String {
    if let Some(path) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else if let Some(path) = value.strip_prefix(r"\\?\") {
        path.to_owned()
    } else {
        value
    }
}

#[cfg(not(windows))]
fn normalize_windows_verbatim_path(value: String) -> String {
    value
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use tempfile::TempDir;

    use super::*;

    enum FakeOutput {
        Valid,
        Invalid,
        WaitForCancellation,
    }

    struct FakeBackend {
        output: FakeOutput,
        calls: AtomicUsize,
    }

    impl FakeBackend {
        fn new(output: FakeOutput) -> Self {
            Self {
                output,
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl SynthesisBackend for FakeBackend {
        fn health(&self) -> Result<SherpaTtsHealth, TtsError> {
            Ok(SherpaTtsHealth {
                ready: true,
                sample_rate: 24_000,
                available_voices: 11,
            })
        }

        fn synthesize(
            &self,
            _text: &str,
            _voice_id: u16,
            _speed: f32,
            cancellation: CancellationToken,
        ) -> Result<GeneratedSamples, TtsError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.output {
                FakeOutput::Valid => Ok(GeneratedSamples {
                    samples: vec![0.25; 24_000],
                    sample_rate: 24_000,
                }),
                FakeOutput::Invalid => Ok(GeneratedSamples {
                    samples: vec![f32::NAN],
                    sample_rate: 24_000,
                }),
                FakeOutput::WaitForCancellation => {
                    while !cancellation.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(TtsError::Cancelled)
                }
            }
        }
    }

    fn provider(directory: &TempDir, backend: Arc<FakeBackend>) -> SherpaKokoroTtsProvider {
        SherpaKokoroTtsProvider::from_backend(backend, directory.path().to_path_buf())
    }

    fn voice() -> VoiceSettings {
        VoiceSettings {
            voice_id: "3".into(),
            rate: 1.0,
            volume: 0.8,
            delivery_style: DeliveryStyle::Neutral,
        }
    }

    #[test]
    fn delivery_profiles_adjust_and_bound_the_base_rate() {
        let playful = delivery_rate(1.0, DeliveryStyle::Playful)
            .unwrap_or_else(|error| panic!("playful rate failed: {error}"));
        let reflective = delivery_rate(1.0, DeliveryStyle::Reflective)
            .unwrap_or_else(|error| panic!("reflective rate failed: {error}"));

        assert!(playful > 1.0);
        assert!(reflective < 1.0);
        assert!(playful > reflective);
        assert_eq!(
            delivery_rate(2.0, DeliveryStyle::Energetic)
                .unwrap_or_else(|error| panic!("bounded rate failed: {error}")),
            2.0
        );
        assert!(matches!(
            delivery_rate(f32::NAN, DeliveryStyle::Neutral),
            Err(TtsError::InvalidConfiguration)
        ));
    }

    #[tokio::test]
    async fn creates_a_valid_bounded_wav_artifact() {
        let directory =
            TempDir::new().unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let backend = Arc::new(FakeBackend::new(FakeOutput::Valid));
        let artifact = provider(&directory, backend)
            .synthesize(
                "A short English radio line.",
                &voice(),
                CancellationToken::new(),
            )
            .await
            .unwrap_or_else(|error| panic!("synthesis failed: {error}"));

        assert!(!artifact.is_mock);
        assert_eq!(artifact.duration_ms, Some(1_000));
        let path = artifact
            .local_path
            .as_deref()
            .map(Path::new)
            .unwrap_or_else(|| panic!("artifact path was missing"));
        assert!(path.starts_with(directory.path()));
        assert!(path.is_file());
        assert!(validate_wav(path, 24_000, 24_000).is_ok());
    }

    #[tokio::test]
    async fn rejects_voice_outside_model_before_synthesis() {
        let directory =
            TempDir::new().unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let backend = Arc::new(FakeBackend::new(FakeOutput::Valid));
        let provider = provider(&directory, backend.clone());
        let mut settings = voice();
        settings.voice_id = "11".into();

        let result = provider
            .synthesize("Test", &settings, CancellationToken::new())
            .await;
        assert!(matches!(result, Err(TtsError::InvalidConfiguration)));
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejects_invalid_audio_without_leaving_an_artifact() {
        let directory =
            TempDir::new().unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let backend = Arc::new(FakeBackend::new(FakeOutput::Invalid));

        let result = provider(&directory, backend)
            .synthesize("Test", &voice(), CancellationToken::new())
            .await;
        assert!(matches!(result, Err(TtsError::InvalidArtifact)));
        let files = fs::read_dir(directory.path())
            .unwrap_or_else(|error| panic!("output directory read failed: {error}"))
            .count();
        assert_eq!(files, 0);
    }

    #[tokio::test]
    async fn external_cancellation_stops_blocking_synthesis() {
        let directory =
            TempDir::new().unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let backend = Arc::new(FakeBackend::new(FakeOutput::WaitForCancellation));
        let provider = provider(&directory, backend);
        let cancellation = CancellationToken::new();
        let token = cancellation.clone();
        let task = tokio::spawn(async move { provider.synthesize("Test", &voice(), token).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancellation.cancel();

        let result = task
            .await
            .unwrap_or_else(|error| panic!("synthesis task failed: {error}"));
        assert!(matches!(result, Err(TtsError::Cancelled)));
        assert_eq!(
            fs::read_dir(directory.path())
                .unwrap_or_else(|error| panic!("output directory read failed: {error}"))
                .count(),
            0
        );
    }

    #[test]
    fn model_configuration_requires_absolute_contained_assets() {
        assert!(matches!(
            SherpaKokoroConfiguration::new("relative-model", Path::new("relative-output")),
            Err(TtsError::InvalidConfiguration)
        ));
        let model =
            TempDir::new().unwrap_or_else(|error| panic!("model directory failed: {error}"));
        let output =
            TempDir::new().unwrap_or_else(|error| panic!("output directory failed: {error}"));
        fs::write(model.path().join(MODEL_FILE), [])
            .unwrap_or_else(|error| panic!("model fixture failed: {error}"));
        fs::write(model.path().join(VOICES_FILE), [])
            .unwrap_or_else(|error| panic!("voices fixture failed: {error}"));
        fs::write(model.path().join(TOKENS_FILE), [])
            .unwrap_or_else(|error| panic!("tokens fixture failed: {error}"));
        fs::create_dir(model.path().join(DATA_DIRECTORY))
            .unwrap_or_else(|error| panic!("data fixture failed: {error}"));

        assert!(SherpaKokoroConfiguration::new(
            model.path().to_string_lossy().as_ref(),
            output.path()
        )
        .is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn native_paths_drop_the_windows_verbatim_prefix() {
        assert_eq!(
            path_string(Path::new(r"\\?\C:\models\kokoro\model.onnx"))
                .unwrap_or_else(|error| panic!("DOS path conversion failed: {error}")),
            r"C:\models\kokoro\model.onnx"
        );
        assert_eq!(
            path_string(Path::new(r"\\?\UNC\server\share\model.onnx"))
                .unwrap_or_else(|error| panic!("UNC path conversion failed: {error}")),
            r"\\server\share\model.onnx"
        );
    }
}
