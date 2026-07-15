use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AudioArtifact, TextToSpeechProvider, TtsError, VoiceSettings};

#[derive(Default)]
pub struct MockTextToSpeechProvider;

#[async_trait]
impl TextToSpeechProvider for MockTextToSpeechProvider {
    async fn synthesize(
        &self,
        text: &str,
        _settings: &VoiceSettings,
        cancellation: CancellationToken,
    ) -> Result<AudioArtifact, TtsError> {
        if cancellation.is_cancelled() {
            return Err(TtsError::Cancelled);
        }
        let word_count = text.split_whitespace().count() as u64;
        Ok(AudioArtifact {
            artifact_id: Uuid::new_v4().to_string(),
            local_path: None,
            duration_ms: Some(word_count.saturating_mul(360)),
            is_mock: true,
        })
    }

    async fn cancel(&self) -> Result<(), TtsError> {
        Ok(())
    }
}
