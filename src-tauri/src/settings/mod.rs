use serde::{Deserialize, Serialize};

use crate::errors::AppError;

pub const SPOTIFY_REDIRECT_URI: &str = "http://127.0.0.1:43821/callback";
const LEGACY_SPOTIFY_REDIRECT_URI: &str = "http://127.0.0.1:43821/oauth/callback";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TalkFrequency {
    Minimal,
    Normal,
    Talkative,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsProviderSetting {
    Mock,
    SherpaKokoro,
    ParlerMini,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub mock_mode: bool,
    pub spotify_client_id: Option<String>,
    pub spotify_redirect_uri: String,
    pub ollama_base_url: String,
    pub ollama_model: Option<String>,
    #[serde(default)]
    pub use_ollama: bool,
    pub dj_profile_id: String,
    pub talk_frequency: TalkFrequency,
    pub maximum_segment_words: u16,
    pub musicbrainz_contact: Option<String>,
    pub cache_retention_days: u16,
    pub debug_logging: bool,
    #[serde(default)]
    pub automatic_transition_speech: bool,
    pub tts_provider: TtsProviderSetting,
    #[serde(default)]
    pub tts_model_directory: Option<String>,
    #[serde(default)]
    pub tts_voice_id: u16,
    #[serde(default = "default_tts_speed_percent")]
    pub tts_speed_percent: u16,
    #[serde(default = "default_parler_base_url")]
    pub parler_base_url: String,
    #[serde(default = "default_parler_speaker")]
    pub parler_speaker: String,
    pub audio_output_device: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mock_mode: true,
            spotify_client_id: None,
            spotify_redirect_uri: SPOTIFY_REDIRECT_URI.into(),
            ollama_base_url: "http://127.0.0.1:11434".into(),
            ollama_model: None,
            use_ollama: false,
            dj_profile_id: "mira-vale".into(),
            talk_frequency: TalkFrequency::Normal,
            maximum_segment_words: 42,
            musicbrainz_contact: None,
            cache_retention_days: 90,
            debug_logging: false,
            automatic_transition_speech: false,
            tts_provider: TtsProviderSetting::Mock,
            tts_model_directory: None,
            tts_voice_id: 0,
            tts_speed_percent: default_tts_speed_percent(),
            parler_base_url: default_parler_base_url(),
            parler_speaker: default_parler_speaker(),
            audio_output_device: None,
        }
    }
}

impl AppSettings {
    pub fn normalize_legacy_values(&mut self) {
        if self.spotify_redirect_uri == LEGACY_SPOTIFY_REDIRECT_URI {
            self.spotify_redirect_uri = SPOTIFY_REDIRECT_URI.into();
        }
        self.normalize_user_input();
    }

    pub fn normalize_user_input(&mut self) {
        if let Some(directory) = self.tts_model_directory.as_mut() {
            let trimmed = directory.trim();
            let unquoted = trimmed
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(trimmed)
                .trim();
            *directory = unquoted.to_owned();
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.maximum_segment_words == 0 || self.maximum_segment_words > 150 {
            return Err(AppError::Configuration(
                "maximum segment length must be between 1 and 150 words".into(),
            ));
        }
        if self.cache_retention_days == 0 || self.cache_retention_days > 3650 {
            return Err(AppError::Configuration(
                "fact cache retention must be between 1 and 3650 days".into(),
            ));
        }
        if self.tts_speed_percent < 50 || self.tts_speed_percent > 200 {
            return Err(AppError::Configuration(
                "TTS speed must be between 50 and 200 percent".into(),
            ));
        }
        if matches!(self.tts_provider, TtsProviderSetting::SherpaKokoro)
            && self.tts_model_directory.is_none()
        {
            return Err(AppError::Configuration(
                "select a Kokoro model directory before enabling Sherpa-ONNX TTS".into(),
            ));
        }
        if self.tts_model_directory.as_ref().is_some_and(|directory| {
            directory.is_empty()
                || directory.len() > 1_024
                || directory.contains('\0')
                || !std::path::Path::new(directory).is_absolute()
        }) {
            return Err(AppError::Configuration(
                "Kokoro model directory must be an absolute local path".into(),
            ));
        }
        crate::tts::parler::validate_local_base_url(&self.parler_base_url).map_err(|_| {
            AppError::Configuration(
                "Parler base URL must be an HTTP loopback URL with an explicit port".into(),
            )
        })?;
        if !crate::tts::parler::is_supported_speaker(&self.parler_speaker) {
            return Err(AppError::Configuration(
                "select a supported Parler Mini speaker".into(),
            ));
        }
        if let Some(contact) = self.musicbrainz_contact.as_deref() {
            crate::music_facts::musicbrainz::validate_contact(contact).map_err(|_| {
                AppError::Configuration(
                    "MusicBrainz contact must be an email address or HTTPS URL".into(),
                )
            })?;
        }
        if self.spotify_client_id.as_ref().is_some_and(|client_id| {
            client_id.is_empty()
                || client_id.len() > 128
                || !client_id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        }) {
            return Err(AppError::Configuration(
                "Spotify Client ID must contain only ASCII letters and numbers".into(),
            ));
        }
        crate::llm::ollama::validate_local_base_url(&self.ollama_base_url).map_err(|_| {
            AppError::Configuration(
                "Ollama base URL must be an HTTP loopback URL with an explicit port".into(),
            )
        })?;
        if self.ollama_model.as_ref().is_some_and(|model| {
            let model = model.trim();
            model.is_empty() || model.len() > 200 || model.chars().any(char::is_control)
        }) {
            return Err(AppError::Configuration(
                "Ollama model name is invalid".into(),
            ));
        }
        if self.use_ollama && self.ollama_model.is_none() {
            return Err(AppError::Configuration(
                "select an installed Ollama model before enabling Ollama mode".into(),
            ));
        }
        let redirect = url::Url::parse(&self.spotify_redirect_uri)
            .map_err(|_| AppError::Configuration("Spotify redirect URI is invalid".into()))?;
        if redirect.scheme() != "http"
            || redirect.host_str() != Some("127.0.0.1")
            || redirect.port().is_none()
            || redirect.path() != "/callback"
            || redirect.query().is_some()
            || redirect.fragment().is_some()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
        {
            return Err(AppError::Configuration(
                "Spotify redirect URI must be an exact IPv4 loopback callback".into(),
            ));
        }
        Ok(())
    }
}

const fn default_tts_speed_percent() -> u16 {
    100
}

fn default_parler_base_url() -> String {
    "http://127.0.0.1:43822".into()
}

fn default_parler_speaker() -> String {
    "Jon".into()
}

#[cfg(test)]
mod tests {
    use super::AppSettings;

    #[test]
    fn rejects_non_loopback_spotify_redirects() {
        let settings = AppSettings {
            spotify_redirect_uri: "http://localhost:43821/callback".into(),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn accepts_public_spotify_client_id() {
        let settings = AppSettings {
            spotify_client_id: Some("0123456789abcdefABCDEF".into()),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn migrates_the_previous_callback_path() {
        let mut settings = AppSettings {
            spotify_redirect_uri: "http://127.0.0.1:43821/oauth/callback".into(),
            ..AppSettings::default()
        };
        settings.normalize_legacy_values();
        assert_eq!(settings.spotify_redirect_uri, super::SPOTIFY_REDIRECT_URI);
    }

    #[test]
    fn rejects_remote_ollama_urls() {
        let settings = AppSettings {
            ollama_base_url: "https://example.com:11434".into(),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn ollama_mode_requires_a_model() {
        let settings = AppSettings {
            use_ollama: true,
            ..AppSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn rejects_invalid_musicbrainz_contact() {
        let settings = AppSettings {
            musicbrainz_contact: Some("not a contact".into()),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn sherpa_tts_requires_an_absolute_model_directory() {
        let settings = AppSettings {
            tts_provider: super::TtsProviderSetting::SherpaKokoro,
            tts_model_directory: Some("relative/model".into()),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn normalizes_a_windows_copy_as_path_directory() {
        let mut settings = AppSettings {
            tts_model_directory: Some(r#"  "C:\models\kokoro-en-v0_19"  "#.into()),
            ..AppSettings::default()
        };
        settings.normalize_user_input();
        assert_eq!(
            settings.tts_model_directory.as_deref(),
            Some(r"C:\models\kokoro-en-v0_19")
        );
    }

    #[test]
    fn rejects_remote_parler_urls_and_unknown_speakers() {
        let remote = AppSettings {
            parler_base_url: "http://example.com:43822".into(),
            ..AppSettings::default()
        };
        assert!(remote.validate().is_err());

        let unknown = AppSettings {
            parler_speaker: "Unknown".into(),
            ..AppSettings::default()
        };
        assert!(unknown.validate().is_err());
    }
}
