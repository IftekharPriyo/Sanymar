use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::music_provider::Track;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactCategory {
    Release,
    RecordingStory,
    ArtistStory,
    LyricsOrTheme,
    CulturalImpact,
    ChartHistory,
    ScreenAppearance,
    LivePerformance,
    TechnicalMusic,
    FanTrivia,
    RelatedEvent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MusicFact {
    pub id: String,
    pub text: String,
    pub category: FactCategory,
    pub source_name: String,
    pub source_url: Option<String>,
    pub confidence: f32,
    pub human_reviewed: bool,
    #[serde(default)]
    pub verification_method: VerificationMethod,
    pub created_at: DateTime<Utc>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub track_id: Option<String>,
    pub album_id: Option<String>,
    pub artist_id: Option<String>,
}

impl MusicFact {
    pub fn is_verified(&self) -> bool {
        self.last_verified_at.is_some()
            && (self.human_reviewed
                || matches!(
                    self.verification_method,
                    VerificationMethod::HumanReviewed | VerificationMethod::AuthoritativeMetadata
                ))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    #[default]
    Unverified,
    HumanReviewed,
    AuthoritativeMetadata,
}

#[derive(Debug, Error)]
pub enum MusicFactError {
    #[error("fact provider is unavailable")]
    Unavailable,
    #[error("fact provider timed out")]
    Timeout,
    #[error("fact provider returned malformed data")]
    MalformedResponse,
    #[error("fact provider rate limit was reached")]
    RateLimited,
    #[error("fact lookup was cancelled")]
    Cancelled,
    #[error("fact provider configuration is invalid")]
    InvalidConfiguration,
}

#[async_trait]
pub trait MusicFactProvider: Send + Sync {
    async fn facts_for(
        &self,
        track: &Track,
        cancellation: CancellationToken,
    ) -> Result<Vec<MusicFact>, MusicFactError>;
}

pub mod mock;
pub mod musicbrainz;
