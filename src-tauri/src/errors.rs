use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("music provider is not connected")]
    NotConnected,
    #[error("no active playback")]
    NoActivePlayback,
    #[error("provider operation failed: {0}")]
    Provider(String),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("no usable fact was found")]
    NoUsableFact,
    #[error("script generation failed: {0}")]
    ScriptGeneration(String),
    #[error("script validation failed: {0}")]
    Validation(String),
    #[error("text-to-speech is unavailable: {0}")]
    Tts(String),
    #[error("audio playback failed: {0}")]
    Audio(String),
    #[error("database operation failed: {0}")]
    Database(String),
    #[error("configuration is invalid: {0}")]
    Configuration(String),
    #[error("operation was cancelled")]
    Cancelled,
    #[error("result belongs to a stale broadcast job")]
    StaleJob,
    #[error("internal application error: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Internal(value.to_string())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        let code = match &value {
            AppError::NotConnected => "not_connected",
            AppError::NoActivePlayback => "no_active_playback",
            AppError::Provider(_) => "provider_unavailable",
            AppError::Authentication(_) => "authentication_failed",
            AppError::NoUsableFact => "no_usable_fact",
            AppError::ScriptGeneration(_) => "script_generation_failed",
            AppError::Validation(_) => "validation_failed",
            AppError::Tts(_) => "tts_unavailable",
            AppError::Audio(_) => "audio_failed",
            AppError::Database(_) => "database_failed",
            AppError::Configuration(_) => "configuration_invalid",
            AppError::Cancelled => "cancelled",
            AppError::StaleJob => "stale_job",
            AppError::Internal(_) => "internal",
        };
        Self {
            code,
            message: redact_sensitive(&value.to_string()),
        }
    }
}

pub fn redact_sensitive(input: &str) -> String {
    const MARKERS: [&str; 4] = [
        "access_token",
        "refresh_token",
        "authorization_code",
        "client_secret",
    ];
    let mut redacted = input.to_owned();
    for marker in MARKERS {
        if let Some(index) = redacted.to_ascii_lowercase().find(marker) {
            redacted.truncate(index);
            redacted.push_str("[REDACTED]");
        }
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::redact_sensitive;

    #[test]
    fn redacts_token_material() {
        let result = redact_sensitive("request failed access_token=very-secret");
        assert_eq!(result, "request failed [REDACTED]");
        assert!(!result.contains("very-secret"));
    }
}
