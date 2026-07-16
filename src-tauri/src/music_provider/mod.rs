use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub provider_id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub provider_id: Option<String>,
    pub title: String,
    pub release_date: Option<NaiveDate>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackVariant {
    Studio,
    Live,
    Remix,
    Acoustic,
    Remaster,
    Unknown,
}

impl TrackVariant {
    pub fn infer(title: &str) -> Self {
        let normalized = title.to_ascii_lowercase();
        if normalized.contains("live") {
            Self::Live
        } else if normalized.contains("remix") || normalized.contains("mix)") {
            Self::Remix
        } else if normalized.contains("acoustic") || normalized.contains("unplugged") {
            Self::Acoustic
        } else if normalized.contains("remaster") {
            Self::Remaster
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub provider_id: String,
    pub title: String,
    pub artists: Vec<Artist>,
    pub album: Option<Album>,
    pub duration_ms: u64,
    pub isrc: Option<String>,
    pub release_date: Option<NaiveDate>,
    pub explicit: bool,
    pub variant: TrackVariant,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDevice {
    pub id: Option<String>,
    pub name: String,
    pub is_active: bool,
    pub volume_percent: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub current_track: Option<Track>,
    pub next_track: Option<Track>,
    pub progress_ms: u64,
    pub is_playing: bool,
    pub device: Option<PlaybackDevice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaybackInterruption {
    pub device_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationStatus {
    Disconnected,
    Connected,
    Expired,
}

#[derive(Debug, Error)]
pub enum MusicProviderError {
    #[error("provider is not connected")]
    NotConnected,
    #[error("no active playback")]
    NoActivePlayback,
    #[error("playback control is unavailable for this account or device")]
    ControlUnavailable,
    #[error("provider request timed out")]
    Timeout,
    #[error("provider authentication expired")]
    AuthenticationExpired,
    #[error("provider rate limit was reached")]
    RateLimited,
    #[error("provider returned malformed data")]
    MalformedResponse,
    #[error("provider is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait MusicProvider: Send + Sync {
    async fn authenticate(&self) -> Result<(), MusicProviderError>;
    async fn authentication_status(&self) -> Result<AuthenticationStatus, MusicProviderError>;
    async fn playback_state(&self) -> Result<PlaybackState, MusicProviderError>;
    async fn pause(&self, device_id: Option<&str>) -> Result<(), MusicProviderError>;
    async fn resume(&self, device_id: Option<&str>) -> Result<(), MusicProviderError>;
    async fn seek(
        &self,
        position_ms: u64,
        device_id: Option<&str>,
    ) -> Result<(), MusicProviderError>;
    async fn skip(&self, device_id: Option<&str>) -> Result<(), MusicProviderError>;
    async fn refresh_authentication(&self) -> Result<(), MusicProviderError>;
}

pub mod mock;

#[cfg(test)]
mod tests {
    use super::TrackVariant;

    #[test]
    fn distinguishes_live_and_remix_versions() {
        assert_eq!(
            TrackVariant::infer("Night Drive (Live at the Forum)"),
            TrackVariant::Live
        );
        assert_eq!(
            TrackVariant::infer("Night Drive - Harbour Remix"),
            TrackVariant::Remix
        );
    }
}
