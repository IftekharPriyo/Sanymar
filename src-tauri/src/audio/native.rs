use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use rodio::{Decoder, DeviceSinkBuilder, Player};
use tokio_util::sync::CancellationToken;

use super::{AudioPlayer, AudioPlayerError};
use crate::tts::AudioArtifact;

const MAX_WAV_BYTES: u64 = 250_000_000;
const CANCELLATION_POLL: Duration = Duration::from_millis(10);

trait PlaybackBackend: Send + Sync {
    fn play_file(
        &self,
        path: &Path,
        cancellation: CancellationToken,
    ) -> Result<(), AudioPlayerError>;
    fn stop(&self) -> Result<(), AudioPlayerError>;
    fn is_playing(&self) -> Result<bool, AudioPlayerError>;
}

#[derive(Default)]
struct RodioBackend {
    active_player: Mutex<Option<Arc<Player>>>,
}

impl RodioBackend {
    fn replace_active(&self, player: Arc<Player>) -> Result<(), AudioPlayerError> {
        let mut active = self
            .active_player
            .lock()
            .map_err(|_| AudioPlayerError::Unavailable)?;
        if let Some(previous) = active.replace(player) {
            previous.stop();
        }
        Ok(())
    }

    fn clear_if_active(&self, player: &Arc<Player>) -> Result<(), AudioPlayerError> {
        let mut active = self
            .active_player
            .lock()
            .map_err(|_| AudioPlayerError::Unavailable)?;
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, player))
        {
            *active = None;
        }
        Ok(())
    }
}

impl PlaybackBackend for RodioBackend {
    fn play_file(
        &self,
        path: &Path,
        cancellation: CancellationToken,
    ) -> Result<(), AudioPlayerError> {
        if cancellation.is_cancelled() {
            return Err(AudioPlayerError::Cancelled);
        }
        let file = File::open(path).map_err(|_| AudioPlayerError::InvalidArtifact)?;
        let source = Decoder::try_from(file).map_err(|_| AudioPlayerError::InvalidArtifact)?;
        let device =
            DeviceSinkBuilder::open_default_sink().map_err(|_| AudioPlayerError::Unavailable)?;
        let player = Arc::new(Player::connect_new(device.mixer()));
        self.replace_active(player.clone())?;
        if cancellation.is_cancelled() {
            player.stop();
            self.clear_if_active(&player)?;
            return Err(AudioPlayerError::Cancelled);
        }
        player.append(source);
        while !player.empty() {
            if cancellation.is_cancelled() {
                player.stop();
                self.clear_if_active(&player)?;
                return Err(AudioPlayerError::Cancelled);
            }
            std::thread::sleep(CANCELLATION_POLL);
        }
        self.clear_if_active(&player)?;
        Ok(())
    }

    fn stop(&self) -> Result<(), AudioPlayerError> {
        let mut active = self
            .active_player
            .lock()
            .map_err(|_| AudioPlayerError::Unavailable)?;
        if let Some(player) = active.take() {
            player.stop();
        }
        Ok(())
    }

    fn is_playing(&self) -> Result<bool, AudioPlayerError> {
        let active = self
            .active_player
            .lock()
            .map_err(|_| AudioPlayerError::Unavailable)?;
        Ok(active.as_ref().is_some_and(|player| !player.empty()))
    }
}

pub struct NativeAudioPlayer {
    backend: Arc<dyn PlaybackBackend>,
}

impl Default for NativeAudioPlayer {
    fn default() -> Self {
        Self {
            backend: Arc::new(RodioBackend::default()),
        }
    }
}

impl NativeAudioPlayer {
    #[cfg(test)]
    fn from_backend(backend: Arc<dyn PlaybackBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl AudioPlayer for NativeAudioPlayer {
    async fn play(
        &self,
        artifact: &AudioArtifact,
        cancellation: CancellationToken,
    ) -> Result<(), AudioPlayerError> {
        let path = validate_artifact(artifact)?;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || backend.play_file(&path, cancellation))
            .await
            .map_err(|_| AudioPlayerError::PlaybackFailed)?
    }

    async fn stop(&self) -> Result<(), AudioPlayerError> {
        self.backend.stop()
    }

    async fn is_playing(&self) -> Result<bool, AudioPlayerError> {
        self.backend.is_playing()
    }
}

fn validate_artifact(artifact: &AudioArtifact) -> Result<PathBuf, AudioPlayerError> {
    if artifact.is_mock {
        return Err(AudioPlayerError::InvalidArtifact);
    }
    let path = artifact
        .local_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .ok_or(AudioPlayerError::InvalidArtifact)?
        .canonicalize()
        .map_err(|_| AudioPlayerError::InvalidArtifact)?;
    let is_wav = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"));
    let length = path
        .metadata()
        .map_err(|_| AudioPlayerError::InvalidArtifact)?
        .len();
    if !path.is_file() || !is_wav || !(44..=MAX_WAV_BYTES).contains(&length) {
        return Err(AudioPlayerError::InvalidArtifact);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use tempfile::TempDir;

    use super::*;

    struct FakeBackend {
        playing: AtomicBool,
        wait_for_cancellation: bool,
    }

    impl PlaybackBackend for FakeBackend {
        fn play_file(
            &self,
            _path: &Path,
            cancellation: CancellationToken,
        ) -> Result<(), AudioPlayerError> {
            self.playing.store(true, Ordering::SeqCst);
            if self.wait_for_cancellation {
                while !cancellation.is_cancelled() {
                    std::thread::sleep(CANCELLATION_POLL);
                }
                self.playing.store(false, Ordering::SeqCst);
                return Err(AudioPlayerError::Cancelled);
            }
            self.playing.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn stop(&self) -> Result<(), AudioPlayerError> {
            self.playing.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn is_playing(&self) -> Result<bool, AudioPlayerError> {
            Ok(self.playing.load(Ordering::SeqCst))
        }
    }

    fn wav_artifact(directory: &TempDir) -> AudioArtifact {
        let path = directory.path().join("voice.wav");
        std::fs::write(&path, [0_u8; 44])
            .unwrap_or_else(|error| panic!("WAV fixture failed: {error}"));
        AudioArtifact {
            artifact_id: "test-artifact".into(),
            local_path: Some(path.to_string_lossy().into_owned()),
            duration_ms: Some(1_000),
            is_mock: false,
        }
    }

    #[test]
    fn rejects_mock_missing_and_non_wav_artifacts() {
        let directory =
            TempDir::new().unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let mut artifact = wav_artifact(&directory);
        artifact.is_mock = true;
        assert!(matches!(
            validate_artifact(&artifact),
            Err(AudioPlayerError::InvalidArtifact)
        ));
        artifact.is_mock = false;
        artifact.local_path = None;
        assert!(matches!(
            validate_artifact(&artifact),
            Err(AudioPlayerError::InvalidArtifact)
        ));
    }

    #[tokio::test]
    async fn cancellation_stops_blocking_playback() {
        let directory =
            TempDir::new().unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let backend = Arc::new(FakeBackend {
            playing: AtomicBool::new(false),
            wait_for_cancellation: true,
        });
        let player = NativeAudioPlayer::from_backend(backend.clone());
        let cancellation = CancellationToken::new();
        let token = cancellation.clone();
        let artifact = wav_artifact(&directory);
        let task = tokio::spawn(async move { player.play(&artifact, token).await });
        while !backend.playing.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        cancellation.cancel();
        let result = task
            .await
            .unwrap_or_else(|error| panic!("playback task failed: {error}"));
        assert!(matches!(result, Err(AudioPlayerError::Cancelled)));
        assert!(!backend.playing.load(Ordering::SeqCst));
    }
}
