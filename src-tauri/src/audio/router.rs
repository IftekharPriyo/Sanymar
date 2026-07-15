use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{mock::MockAudioPlayer, native::NativeAudioPlayer, AudioPlayer, AudioPlayerError};
use crate::tts::AudioArtifact;

#[derive(Default)]
pub struct LocalAudioRouter {
    mock: MockAudioPlayer,
    native: NativeAudioPlayer,
}

#[async_trait]
impl AudioPlayer for LocalAudioRouter {
    async fn play(
        &self,
        artifact: &AudioArtifact,
        cancellation: CancellationToken,
    ) -> Result<(), AudioPlayerError> {
        if artifact.is_mock {
            self.mock.play(artifact, cancellation).await
        } else {
            self.native.play(artifact, cancellation).await
        }
    }

    async fn stop(&self) -> Result<(), AudioPlayerError> {
        self.mock.stop().await?;
        self.native.stop().await
    }

    async fn is_playing(&self) -> Result<bool, AudioPlayerError> {
        Ok(self.mock.is_playing().await? || self.native.is_playing().await?)
    }
}
