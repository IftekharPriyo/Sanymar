use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::{AuthenticationStatus, MusicProvider, MusicProviderError, PlaybackState};

pub struct MockMusicProvider {
    status: RwLock<AuthenticationStatus>,
    playback: RwLock<PlaybackState>,
}

impl MockMusicProvider {
    pub fn new(playback: PlaybackState) -> Arc<Self> {
        Arc::new(Self {
            status: RwLock::new(AuthenticationStatus::Connected),
            playback: RwLock::new(playback),
        })
    }
}

#[async_trait]
impl MusicProvider for MockMusicProvider {
    async fn authenticate(&self) -> Result<(), MusicProviderError> {
        *self.status.write().await = AuthenticationStatus::Connected;
        Ok(())
    }

    async fn authentication_status(&self) -> Result<AuthenticationStatus, MusicProviderError> {
        Ok(self.status.read().await.clone())
    }

    async fn playback_state(&self) -> Result<PlaybackState, MusicProviderError> {
        Ok(self.playback.read().await.clone())
    }

    async fn pause(&self, _device_id: Option<&str>) -> Result<(), MusicProviderError> {
        self.playback.write().await.is_playing = false;
        Ok(())
    }

    async fn resume(&self, _device_id: Option<&str>) -> Result<(), MusicProviderError> {
        self.playback.write().await.is_playing = true;
        Ok(())
    }

    async fn seek(
        &self,
        position_ms: u64,
        _device_id: Option<&str>,
    ) -> Result<(), MusicProviderError> {
        self.playback.write().await.progress_ms = position_ms;
        Ok(())
    }

    async fn skip(&self, _device_id: Option<&str>) -> Result<(), MusicProviderError> {
        let mut playback = self.playback.write().await;
        playback.current_track = playback.next_track.take();
        playback.progress_ms = 0;
        Ok(())
    }

    async fn refresh_authentication(&self) -> Result<(), MusicProviderError> {
        self.authenticate().await
    }
}
