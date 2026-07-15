use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::tts::AudioArtifact;

#[derive(Debug, Error)]
pub enum AudioPlayerError {
    #[error("audio output is unavailable")]
    Unavailable,
    #[error("audio playback failed")]
    PlaybackFailed,
    #[error("audio artifact is invalid")]
    InvalidArtifact,
    #[error("audio playback was cancelled")]
    Cancelled,
}

#[async_trait]
pub trait AudioPlayer: Send + Sync {
    async fn play(
        &self,
        artifact: &AudioArtifact,
        cancellation: CancellationToken,
    ) -> Result<(), AudioPlayerError>;
    async fn stop(&self) -> Result<(), AudioPlayerError>;
    async fn is_playing(&self) -> Result<bool, AudioPlayerError>;
}

pub mod mock;
pub mod native;
pub mod router;
