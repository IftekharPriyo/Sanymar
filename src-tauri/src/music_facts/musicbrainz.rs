use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{Days, Utc};
use reqwest::{redirect::Policy, Client, StatusCode};
use serde::Deserialize;
use tokio::{sync::Mutex, time::Instant};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{database::SanymarRepository, music_provider::Track};

use super::{FactCategory, MusicFact, MusicFactError, MusicFactProvider, VerificationMethod};

const DEFAULT_BASE_URL: &str = "https://musicbrainz.org/ws/2/";
const MINIMUM_SCORE: u16 = 95;
const MAXIMUM_DURATION_DIFFERENCE_MS: u64 = 5_000;
const REQUEST_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct MusicBrainzRateLimiter {
    next_request: Mutex<Option<Instant>>,
}

pub struct MusicBrainzFactProvider {
    client: Client,
    base_url: Url,
    repository: SanymarRepository,
    cache_retention_days: u16,
    rate_limiter: Arc<MusicBrainzRateLimiter>,
}

impl MusicBrainzFactProvider {
    pub fn new(
        contact: &str,
        repository: SanymarRepository,
        cache_retention_days: u16,
        rate_limiter: Arc<MusicBrainzRateLimiter>,
    ) -> Result<Self, MusicFactError> {
        Self::with_base_url(
            contact,
            DEFAULT_BASE_URL,
            repository,
            cache_retention_days,
            rate_limiter,
            Duration::from_secs(10),
        )
    }

    fn with_base_url(
        contact: &str,
        base_url: &str,
        repository: SanymarRepository,
        cache_retention_days: u16,
        rate_limiter: Arc<MusicBrainzRateLimiter>,
        timeout: Duration,
    ) -> Result<Self, MusicFactError> {
        validate_contact(contact)?;
        let base_url = Url::parse(base_url).map_err(|_| MusicFactError::InvalidConfiguration)?;
        let allowed_test_url = base_url.scheme() == "http"
            && base_url
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|address| address.is_loopback());
        if (base_url.scheme() != "https" && !allowed_test_url)
            || base_url.path() != "/ws/2/"
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
        {
            return Err(MusicFactError::InvalidConfiguration);
        }
        let user_agent = format!("Sanymar/{} ({})", env!("CARGO_PKG_VERSION"), contact.trim());
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(timeout)
            .user_agent(user_agent)
            .build()
            .map_err(|_| MusicFactError::InvalidConfiguration)?;
        Ok(Self {
            client,
            base_url,
            repository,
            cache_retention_days,
            rate_limiter,
        })
    }

    async fn cached(&self, track: &Track) -> Result<Option<Vec<MusicFact>>, MusicFactError> {
        let fresh_after = Utc::now()
            .checked_sub_days(Days::new(self.cache_retention_days.into()))
            .ok_or(MusicFactError::InvalidConfiguration)?;
        self.repository
            .load_cached_facts("spotify", &track.provider_id, fresh_after)
            .await
            .map_err(|_| MusicFactError::Unavailable)
    }

    async fn wait_for_rate_limit(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<(), MusicFactError> {
        let mut next_request = self.rate_limiter.next_request.lock().await;
        if let Some(next) = *next_request {
            tokio::select! {
                _ = cancellation.cancelled() => return Err(MusicFactError::Cancelled),
                _ = tokio::time::sleep_until(next) => {}
            }
        }
        *next_request = Some(Instant::now() + REQUEST_INTERVAL);
        Ok(())
    }

    async fn lookup(
        &self,
        track: &Track,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MusicFact>, MusicFactError> {
        self.wait_for_rate_limit(cancellation).await?;
        let query = search_query(track)?;
        let endpoint = self
            .base_url
            .join("recording/")
            .map_err(|_| MusicFactError::InvalidConfiguration)?;
        let request = self.client.get(endpoint).query(&[
            ("query", query.as_str()),
            ("fmt", "json"),
            ("limit", "5"),
        ]);
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(MusicFactError::Cancelled),
            response = request.send() => response.map_err(map_request_error)?,
        };
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        let search: RecordingSearch = tokio::select! {
            _ = cancellation.cancelled() => return Err(MusicFactError::Cancelled),
            search = response.json() => search.map_err(|_| MusicFactError::MalformedResponse)?,
        };
        let Some(recording) = select_recording(track, &search.recordings) else {
            return Ok(Vec::new());
        };
        Ok(facts_from_recording(track, recording))
    }
}

#[async_trait]
impl MusicFactProvider for MusicBrainzFactProvider {
    async fn facts_for(
        &self,
        track: &Track,
        cancellation: CancellationToken,
    ) -> Result<Vec<MusicFact>, MusicFactError> {
        if cancellation.is_cancelled() {
            return Err(MusicFactError::Cancelled);
        }
        if let Some(facts) = self.cached(track).await? {
            return Ok(facts);
        }
        let facts = self.lookup(track, &cancellation).await?;
        self.repository
            .save_fact_lookup("spotify", track, &facts)
            .await
            .map_err(|_| MusicFactError::Unavailable)?;
        Ok(facts)
    }
}

pub fn validate_contact(contact: &str) -> Result<(), MusicFactError> {
    let contact = contact.trim();
    if contact.is_empty() || contact.len() > 200 || contact.chars().any(char::is_control) {
        return Err(MusicFactError::InvalidConfiguration);
    }
    let is_email = contact.contains('@')
        && !contact.contains(char::is_whitespace)
        && !contact.starts_with('@')
        && !contact.ends_with('@');
    let is_https_url = Url::parse(contact).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    });
    if is_email || is_https_url {
        Ok(())
    } else {
        Err(MusicFactError::InvalidConfiguration)
    }
}

#[derive(Deserialize)]
struct RecordingSearch {
    #[serde(default)]
    recordings: Vec<RecordingResult>,
}

#[derive(Deserialize)]
struct RecordingResult {
    id: String,
    #[serde(default)]
    score: u16,
    title: String,
    length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

#[derive(Deserialize)]
struct ArtistCredit {
    name: String,
}

fn select_recording<'a>(
    track: &Track,
    recordings: &'a [RecordingResult],
) -> Option<&'a RecordingResult> {
    let mut eligible = recordings
        .iter()
        .filter(|recording| recording_matches(track, recording));
    let best = eligible.next()?;
    if eligible.any(|other| best.score.saturating_sub(other.score) <= 2) {
        None
    } else {
        Some(best)
    }
}

fn recording_matches(track: &Track, recording: &RecordingResult) -> bool {
    if recording.score < MINIMUM_SCORE || normalize(&recording.title) != normalize(&track.title) {
        return false;
    }
    let artist_matches = recording.artist_credit.iter().any(|credit| {
        track
            .artists
            .iter()
            .any(|artist| normalize(&credit.name) == normalize(&artist.name))
    });
    if !artist_matches {
        return false;
    }
    recording
        .length
        .is_none_or(|length| length.abs_diff(track.duration_ms) <= MAXIMUM_DURATION_DIFFERENCE_MS)
}

fn facts_from_recording(track: &Track, recording: &RecordingResult) -> Vec<MusicFact> {
    let Some(date) = recording
        .first_release_date
        .as_deref()
        .filter(|value| valid_partial_date(value))
    else {
        return Vec::new();
    };
    let Ok(mbid) = uuid::Uuid::parse_str(&recording.id) else {
        return Vec::new();
    };
    let now = Utc::now();
    vec![MusicFact {
        id: format!("musicbrainz:{mbid}:first-release-date"),
        text: format!("MusicBrainz lists this recording's first release date as {date}."),
        category: FactCategory::Release,
        source_name: "MusicBrainz".into(),
        source_url: Some(format!("https://musicbrainz.org/recording/{mbid}")),
        confidence: f32::from(recording.score.min(100)) / 100.0,
        human_reviewed: false,
        verification_method: VerificationMethod::AuthoritativeMetadata,
        created_at: now,
        last_verified_at: Some(now),
        track_id: Some(track.provider_id.clone()),
        album_id: None,
        artist_id: None,
    }]
}

fn search_query(track: &Track) -> Result<String, MusicFactError> {
    if let Some(isrc) = track.isrc.as_deref().filter(|value| {
        !value.is_empty()
            && value.len() <= 20
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
    }) {
        return Ok(format!("isrc:{}", escape_lucene(isrc)));
    }
    let artist = track
        .artists
        .first()
        .map(|artist| artist.name.trim())
        .filter(|name| !name.is_empty())
        .ok_or(MusicFactError::InvalidConfiguration)?;
    Ok(format!(
        "recording:\"{}\" AND artistname:\"{}\"",
        escape_lucene(&track.title),
        escape_lucene(artist)
    ))
}

fn escape_lucene(value: &str) -> String {
    const SPECIAL: &str = "+-&|!(){}[]^\"~*?:\\/";
    value.chars().fold(String::new(), |mut escaped, character| {
        if SPECIAL.contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
        escaped
    })
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn valid_partial_date(value: &str) -> bool {
    match value.len() {
        4 => value.parse::<u16>().is_ok(),
        7 => chrono::NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d").is_ok(),
        10 => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
        _ => false,
    }
}

fn map_request_error(error: reqwest::Error) -> MusicFactError {
    if error.is_timeout() {
        MusicFactError::Timeout
    } else {
        MusicFactError::Unavailable
    }
}

fn map_status(status: StatusCode) -> MusicFactError {
    if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
        MusicFactError::RateLimited
    } else {
        MusicFactError::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use serde_json::json;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::{
        database::Database,
        music_provider::{Artist, TrackVariant},
    };

    use super::*;

    fn track(provider_id: &str) -> Track {
        Track {
            provider_id: provider_id.into(),
            title: "Night Signal".into(),
            artists: vec![Artist {
                provider_id: Some("spotify-artist-secret".into()),
                name: "Harbour Static".into(),
            }],
            album: None,
            duration_ms: 210_000,
            isrc: Some("GBABC1234567".into()),
            release_date: NaiveDate::from_ymd_opt(2020, 1, 1),
            explicit: false,
            variant: TrackVariant::Studio,
            artwork_url: Some("https://image.invalid/secret.jpg".into()),
        }
    }

    async fn provider(
        server: &MockServer,
        timeout: Duration,
    ) -> Result<MusicBrainzFactProvider, crate::errors::AppError> {
        let database = Database::connect("sqlite::memory:").await?;
        MusicBrainzFactProvider::with_base_url(
            "maintainer@example.com",
            &format!("{}/ws/2/", server.uri()),
            database.repository(),
            90,
            Arc::new(MusicBrainzRateLimiter::default()),
            timeout,
        )
        .map_err(|error| crate::errors::AppError::Provider(error.to_string()))
    }

    fn recording(score: u16, id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "score": score,
            "title": "Night Signal",
            "length": 210000,
            "artist-credit": [{"name": "Harbour Static"}],
            "first-release-date": "2020-04-03"
        })
    }

    #[tokio::test]
    async fn exact_match_becomes_authoritative_fact_and_is_cached() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ws/2/recording/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "recordings": [recording(100, "123e4567-e89b-12d3-a456-426614174000")]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let provider = provider(&server, Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("provider setup failed: {error}"));
        let input = track("spotify-track-one");

        let first = provider
            .facts_for(&input, CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("lookup failed: {error}"));
        let second = provider
            .facts_for(&input, CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("cached lookup failed: {error}"));

        assert_eq!(first.len(), 1);
        assert_eq!(first, second);
        assert!(!first[0].human_reviewed);
        assert_eq!(
            first[0].verification_method,
            VerificationMethod::AuthoritativeMetadata
        );
        assert!(first[0].is_verified());
        let requests = server
            .received_requests()
            .await
            .unwrap_or_else(|| panic!("requests were not retained"));
        let user_agent = requests[0]
            .headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_default();
        assert!(user_agent.contains("maintainer@example.com"));
        let request_url = requests[0].url.to_string();
        assert!(request_url.contains("isrc%3AGBABC1234567"));
        assert!(!request_url.contains("spotify-artist-secret"));
        assert!(!request_url.contains("image.invalid"));
    }

    #[tokio::test]
    async fn ambiguous_matches_are_rejected_and_negative_cached() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ws/2/recording/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "recordings": [
                    recording(100, "123e4567-e89b-12d3-a456-426614174000"),
                    recording(99, "123e4567-e89b-12d3-a456-426614174001")
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let provider = provider(&server, Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("provider setup failed: {error}"));
        let input = track("spotify-track-ambiguous");

        assert!(provider
            .facts_for(&input, CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("lookup failed: {error}"))
            .is_empty());
        assert!(provider
            .facts_for(&input, CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("cached lookup failed: {error}"))
            .is_empty());
    }

    #[tokio::test]
    async fn request_timeout_is_typed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ws/2/recording/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(100))
                    .set_body_json(json!({"recordings": []})),
            )
            .mount(&server)
            .await;
        let provider = provider(&server, Duration::from_millis(10))
            .await
            .unwrap_or_else(|error| panic!("provider setup failed: {error}"));

        let result = provider
            .facts_for(&track("spotify-track-timeout"), CancellationToken::new())
            .await;
        assert!(matches!(result, Err(MusicFactError::Timeout)));
    }

    #[tokio::test]
    async fn cancellation_aborts_an_in_flight_lookup() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ws/2/recording/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .set_body_json(json!({"recordings": []})),
            )
            .mount(&server)
            .await;
        let provider = provider(&server, Duration::from_secs(2))
            .await
            .unwrap_or_else(|error| panic!("provider setup failed: {error}"));
        let cancellation = CancellationToken::new();
        let token = cancellation.clone();
        let task = tokio::spawn(async move {
            provider
                .facts_for(&track("spotify-track-cancel"), token)
                .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancellation.cancel();

        let result = task
            .await
            .unwrap_or_else(|error| panic!("lookup task failed: {error}"));
        assert!(matches!(result, Err(MusicFactError::Cancelled)));
    }

    #[test]
    fn contact_rejects_header_injection_and_non_https_urls() {
        assert!(validate_contact("person@example.com\r\nInjected: yes").is_err());
        assert!(validate_contact("http://example.com/contact").is_err());
        assert!(validate_contact("https://example.com/contact").is_ok());
    }
}
