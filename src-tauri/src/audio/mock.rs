use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::{AudioPlayer, AudioPlayerError};
use crate::tts::AudioArtifact;

#[derive(Default)]
pub struct MockAudioPlayer {
    playing: RwLock<bool>,
}

#[async_trait]
impl AudioPlayer for MockAudioPlayer {
    async fn play(
        &self,
        _artifact: &AudioArtifact,
        cancellation: CancellationToken,
    ) -> Result<(), AudioPlayerError> {
        if cancellation.is_cancelled() {
            return Err(AudioPlayerError::Cancelled);
        }
        *self.playing.write().await = true;
        Ok(())
    }

    async fn stop(&self) -> Result<(), AudioPlayerError> {
        *self.playing.write().await = false;
        Ok(())
    }

    async fn is_playing(&self) -> Result<bool, AudioPlayerError> {
        Ok(*self.playing.read().await)
    }
}
