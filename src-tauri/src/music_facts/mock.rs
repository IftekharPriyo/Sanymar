use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use super::{FactCategory, MusicFact, MusicFactError, MusicFactProvider};
use crate::music_provider::Track;

#[derive(Default)]
pub struct MockMusicFactProvider;

#[async_trait]
impl MusicFactProvider for MockMusicFactProvider {
    async fn facts_for(
        &self,
        track: &Track,
        cancellation: CancellationToken,
    ) -> Result<Vec<MusicFact>, MusicFactError> {
        if cancellation.is_cancelled() {
            return Err(MusicFactError::Cancelled);
        }
        Ok(vec![MusicFact {
            id: format!("mock-fact-{}", track.provider_id),
            text: "The arrangement leaves deliberate space around its central rhythm.".into(),
            category: FactCategory::TechnicalMusic,
            source_name: "Sanymar curated development fixture".into(),
            source_url: None,
            confidence: 1.0,
            human_reviewed: true,
            verification_method: super::VerificationMethod::HumanReviewed,
            created_at: Utc::now(),
            last_verified_at: Some(Utc::now()),
            track_id: Some(track.provider_id.clone()),
            album_id: None,
            artist_id: None,
        }])
    }
}
