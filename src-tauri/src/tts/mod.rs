use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStyle {
    Neutral,
    Warm,
    Energetic,
    Playful,
    Reflective,
    Authoritative,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSettings {
    pub voice_id: String,
    pub rate: f32,
    pub volume: f32,
    pub delivery_style: DeliveryStyle,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioArtifact {
    pub artifact_id: String,
    pub local_path: Option<String>,
    pub duration_ms: Option<u64>,
    pub is_mock: bool,
}

#[derive(Debug, Error)]
pub enum TtsError {
    #[error("text-to-speech configuration is invalid")]
    InvalidConfiguration,
    #[error("text-to-speech provider is unavailable")]
    Unavailable,
    #[error("synthesis was cancelled")]
    Cancelled,
    #[error("synthesis failed")]
    SynthesisFailed,
    #[error("generated audio artifact is invalid")]
    InvalidArtifact,
}

#[async_trait]
pub trait TextToSpeechProvider: Send + Sync {
    async fn synthesize(
        &self,
        text: &str,
        settings: &VoiceSettings,
        cancellation: CancellationToken,
    ) -> Result<AudioArtifact, TtsError>;
    async fn cancel(&self) -> Result<(), TtsError>;
}

pub mod mock;
pub mod parler;
pub mod sherpa;
