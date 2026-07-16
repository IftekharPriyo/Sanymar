use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use reqwest::{header::CONTENT_LENGTH, Method, StatusCode};
use serde::Deserialize;

use crate::{
    music_provider::{
        Album, Artist, AuthenticationStatus, MusicProvider, MusicProviderError, PlaybackDevice,
        PlaybackState, Track, TrackVariant,
    },
    security::{Credential, CredentialStore},
};

use super::SpotifyConfiguration;

const PROVIDER: &str = "spotify";
const API_BASE: &str = "https://api.spotify.com/v1";
const TOKEN_ENDPOINT: &str = "https://accounts.spotify.com/api/token";

pub struct SpotifyProvider {
    configuration: SpotifyConfiguration,
    credential_store: Arc<dyn CredentialStore>,
    client: reqwest::Client,
    api_base: String,
}

#[derive(Deserialize)]
struct SpotifyPlayback {
    progress_ms: Option<u64>,
    is_playing: bool,
    item: Option<SpotifyItem>,
    device: Option<SpotifyDevice>,
}

#[derive(Deserialize)]
struct SpotifyQueue {
    queue: Vec<SpotifyItem>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SpotifyItem {
    Track {
        id: String,
        name: String,
        artists: Vec<SpotifyArtist>,
        album: Box<SpotifyAlbum>,
        duration_ms: u64,
        explicit: bool,
        external_ids: Option<SpotifyExternalIds>,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Deserialize)]
struct SpotifyArtist {
    id: Option<String>,
    name: String,
}

#[derive(Deserialize)]
struct SpotifyAlbum {
    id: Option<String>,
    name: String,
    release_date: Option<String>,
    images: Vec<SpotifyImage>,
}

#[derive(Deserialize)]
struct SpotifyImage {
    url: String,
}

#[derive(Deserialize)]
struct SpotifyExternalIds {
    isrc: Option<String>,
}

#[derive(Deserialize)]
struct SpotifyDevice {
    id: Option<String>,
    name: String,
    is_active: bool,
    volume_percent: Option<u8>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

impl SpotifyProvider {
    pub fn new(
        configuration: SpotifyConfiguration,
        credential_store: Arc<dyn CredentialStore>,
    ) -> Result<Self, MusicProviderError> {
        if configuration.client_id.trim().is_empty() {
            return Err(MusicProviderError::NotConnected);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| MusicProviderError::Unavailable)?;
        Ok(Self {
            configuration,
            credential_store,
            client,
            api_base: API_BASE.into(),
        })
    }

    #[cfg(test)]
    fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = api_base;
        self
    }

    async fn credential(&self) -> Result<Credential, MusicProviderError> {
        let credential = self
            .credential_store
            .load(PROVIDER)
            .await
            .map_err(|_| MusicProviderError::NotConnected)?;
        let should_refresh = credential
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now() + ChronoDuration::seconds(30));
        if should_refresh {
            self.refresh_credential(credential).await
        } else {
            Ok(credential)
        }
    }

    async fn refresh_credential(
        &self,
        current: Credential,
    ) -> Result<Credential, MusicProviderError> {
        let refresh_token = current
            .refresh_token
            .as_deref()
            .ok_or(MusicProviderError::AuthenticationExpired)?;
        let response = self
            .client
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", self.configuration.client_id.as_str()),
            ])
            .send()
            .await
            .map_err(map_transport_error)?;
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        let refreshed: RefreshResponse = response
            .json()
            .await
            .map_err(|_| MusicProviderError::MalformedResponse)?;
        if !refreshed.token_type.eq_ignore_ascii_case("bearer")
            || refreshed.access_token.is_empty()
            || refreshed.expires_in == 0
        {
            return Err(MusicProviderError::MalformedResponse);
        }
        let credential = Credential {
            access_token: refreshed.access_token,
            refresh_token: refreshed.refresh_token.or(current.refresh_token),
            expires_at: Some(Utc::now() + ChronoDuration::seconds(refreshed.expires_in as i64)),
            scopes: refreshed
                .scope
                .map(|scope| scope.split_whitespace().map(str::to_owned).collect())
                .unwrap_or(current.scopes),
        };
        self.credential_store
            .save(PROVIDER, credential.clone())
            .await
            .map_err(|_| MusicProviderError::Unavailable)?;
        Ok(credential)
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<reqwest::Response, MusicProviderError> {
        let credential = self.credential().await?;
        let response = self
            .send_with_read_retry(&method, path, &credential.access_token)
            .await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let refreshed = self.refresh_credential(credential).await?;
        self.send_authorized(&method, path, &refreshed.access_token)
            .await
    }

    async fn send_authorized(
        &self,
        method: &Method,
        path: &str,
        access_token: &str,
    ) -> Result<reqwest::Response, MusicProviderError> {
        let request = self
            .client
            .request(method.clone(), format!("{}{path}", self.api_base))
            .bearer_auth(access_token);
        let request = if method == Method::GET {
            request
        } else {
            request.header(CONTENT_LENGTH, 0).body(Vec::new())
        };
        request.send().await.map_err(map_transport_error)
    }

    async fn send_with_read_retry(
        &self,
        method: &Method,
        path: &str,
        access_token: &str,
    ) -> Result<reqwest::Response, MusicProviderError> {
        let first = self.send_authorized(method, path, access_token).await;
        let retry = match &first {
            Ok(response) => should_retry_read(method, Some(response.status()), None),
            Err(error) => should_retry_read(method, None, Some(error)),
        };
        if !retry {
            return first;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.send_authorized(method, path, access_token).await
    }

    async fn control(&self, method: Method, path: &str) -> Result<(), MusicProviderError> {
        let response = self.request(method, path).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            tracing::warn!(
                status = response.status().as_u16(),
                "Spotify playback control request was rejected"
            );
            Err(map_status(response.status()))
        }
    }

    fn player_control_path(
        operation: &str,
        parameters: &[(&str, String)],
        device_id: Option<&str>,
    ) -> Result<String, MusicProviderError> {
        if device_id.is_some_and(str::is_empty) {
            return Err(MusicProviderError::ControlUnavailable);
        }
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in parameters {
            query.append_pair(key, value);
        }
        if let Some(device_id) = device_id {
            query.append_pair("device_id", device_id);
        }
        let query = query.finish();
        Ok(if query.is_empty() {
            format!("/me/player/{operation}")
        } else {
            format!("/me/player/{operation}?{query}")
        })
    }
}

#[async_trait]
impl MusicProvider for SpotifyProvider {
    async fn authenticate(&self) -> Result<(), MusicProviderError> {
        self.credential().await.map(|_| ())
    }

    async fn authentication_status(&self) -> Result<AuthenticationStatus, MusicProviderError> {
        match self.credential_store.load(PROVIDER).await {
            Ok(credential)
                if credential
                    .expires_at
                    .is_some_and(|value| value <= Utc::now()) =>
            {
                Ok(AuthenticationStatus::Expired)
            }
            Ok(_) => Ok(AuthenticationStatus::Connected),
            Err(_) => Ok(AuthenticationStatus::Disconnected),
        }
    }

    async fn playback_state(&self) -> Result<PlaybackState, MusicProviderError> {
        let playback_response = self.request(Method::GET, "/me/player").await?;
        if playback_response.status() == StatusCode::NO_CONTENT {
            return Err(MusicProviderError::NoActivePlayback);
        }
        if !playback_response.status().is_success() {
            return Err(map_status(playback_response.status()));
        }
        let playback: SpotifyPlayback = playback_response
            .json()
            .await
            .map_err(|_| MusicProviderError::MalformedResponse)?;
        let current_track = playback.item.map(normalize_track).transpose()?.flatten();
        let queue_response = self.request(Method::GET, "/me/player/queue").await?;
        if !queue_response.status().is_success() {
            return Err(map_status(queue_response.status()));
        }
        let queue: SpotifyQueue = queue_response
            .json()
            .await
            .map_err(|_| MusicProviderError::MalformedResponse)?;
        let next_track = queue
            .queue
            .into_iter()
            .find_map(|item| normalize_track(item).transpose())
            .transpose()?;
        Ok(PlaybackState {
            current_track,
            next_track,
            progress_ms: playback.progress_ms.unwrap_or(0),
            is_playing: playback.is_playing,
            device: playback.device.map(|device| PlaybackDevice {
                id: device.id,
                name: device.name,
                is_active: device.is_active,
                volume_percent: device.volume_percent,
            }),
        })
    }

    async fn pause(&self, device_id: Option<&str>) -> Result<(), MusicProviderError> {
        let path = Self::player_control_path("pause", &[], device_id)?;
        self.control(Method::PUT, &path).await
    }

    async fn resume(&self, device_id: Option<&str>) -> Result<(), MusicProviderError> {
        let path = Self::player_control_path("play", &[], device_id)?;
        self.control(Method::PUT, &path).await
    }

    async fn seek(
        &self,
        position_ms: u64,
        device_id: Option<&str>,
    ) -> Result<(), MusicProviderError> {
        let path = Self::player_control_path(
            "seek",
            &[("position_ms", position_ms.to_string())],
            device_id,
        )?;
        self.control(Method::PUT, &path).await
    }

    async fn skip(&self, device_id: Option<&str>) -> Result<(), MusicProviderError> {
        let path = Self::player_control_path("next", &[], device_id)?;
        self.control(Method::POST, &path).await
    }

    async fn refresh_authentication(&self) -> Result<(), MusicProviderError> {
        let current = self
            .credential_store
            .load(PROVIDER)
            .await
            .map_err(|_| MusicProviderError::NotConnected)?;
        self.refresh_credential(current).await.map(|_| ())
    }
}

fn normalize_track(item: SpotifyItem) -> Result<Option<Track>, MusicProviderError> {
    let SpotifyItem::Track {
        id,
        name,
        artists,
        album,
        duration_ms,
        explicit,
        external_ids,
    } = item
    else {
        return Ok(None);
    };
    if id.is_empty() || name.is_empty() || artists.is_empty() || duration_ms == 0 {
        return Err(MusicProviderError::MalformedResponse);
    }
    let artwork_url = album
        .images
        .first()
        .map(|image| validate_artwork_url(&image.url))
        .transpose()?;
    let release_date = album
        .release_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok());
    Ok(Some(Track {
        provider_id: id,
        variant: TrackVariant::infer(&name),
        title: name,
        artists: artists
            .into_iter()
            .map(|artist| Artist {
                provider_id: artist.id,
                name: artist.name,
            })
            .collect(),
        album: Some(Album {
            provider_id: album.id,
            title: album.name,
            release_date,
            artwork_url: artwork_url.clone(),
        }),
        duration_ms,
        isrc: external_ids.and_then(|ids| ids.isrc),
        release_date,
        explicit,
        artwork_url,
    }))
}

fn validate_artwork_url(value: &str) -> Result<String, MusicProviderError> {
    let url = url::Url::parse(value).map_err(|_| MusicProviderError::MalformedResponse)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
    {
        return Err(MusicProviderError::MalformedResponse);
    }
    Ok(url.into())
}

fn map_transport_error(error: reqwest::Error) -> MusicProviderError {
    if error.is_timeout() {
        MusicProviderError::Timeout
    } else {
        MusicProviderError::Unavailable
    }
}

fn map_status(status: StatusCode) -> MusicProviderError {
    match status {
        StatusCode::UNAUTHORIZED => MusicProviderError::AuthenticationExpired,
        StatusCode::BAD_REQUEST
        | StatusCode::FORBIDDEN
        | StatusCode::NOT_FOUND
        | StatusCode::LENGTH_REQUIRED => MusicProviderError::ControlUnavailable,
        StatusCode::TOO_MANY_REQUESTS => MusicProviderError::RateLimited,
        status if status.is_server_error() => MusicProviderError::Unavailable,
        _ => MusicProviderError::MalformedResponse,
    }
}

fn should_retry_read(
    method: &Method,
    status: Option<StatusCode>,
    error: Option<&MusicProviderError>,
) -> bool {
    method == Method::GET
        && (status.is_some_and(|status| status.is_server_error())
            || error.is_some_and(|error| {
                matches!(
                    error,
                    MusicProviderError::Timeout | MusicProviderError::Unavailable
                )
            }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration as ChronoDuration, Utc};
    use reqwest::StatusCode;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    use super::{map_status, normalize_track, should_retry_read, SpotifyItem, SpotifyProvider};
    use crate::{
        music_provider::{MusicProvider, MusicProviderError, TrackVariant},
        security::{mock::InMemoryCredentialStore, Credential, CredentialStore},
        spotify::SpotifyConfiguration,
    };

    #[test]
    fn ignores_non_track_queue_items() {
        assert!(normalize_track(SpotifyItem::Unsupported).is_ok_and(|track| track.is_none()));
    }

    #[test]
    fn normalizes_a_sanitized_spotify_track() {
        let item: SpotifyItem = serde_json::from_str(
            r#"{
                "type":"track",
                "id":"track-1",
                "name":"Signal Room (Live)",
                "artists":[{"id":"artist-1","name":"Mira"}],
                "album":{"id":"album-1","name":"Night Shift","release_date":"2026-04-02","images":[{"url":"https://i.scdn.co/image/test"}]},
                "duration_ms":210000,
                "explicit":false,
                "external_ids":{"isrc":"TEST123"}
            }"#,
        )
        .expect("sanitized fixture must deserialize");
        let track = normalize_track(item)
            .expect("fixture must normalize")
            .expect("fixture is a track");
        assert_eq!(track.provider_id, "track-1");
        assert_eq!(track.variant, TrackVariant::Live);
        assert_eq!(track.artists[0].name, "Mira");
        assert_eq!(track.isrc.as_deref(), Some("TEST123"));
    }

    #[test]
    fn rejects_non_https_artwork_at_the_adapter_boundary() {
        let item: SpotifyItem = serde_json::from_str(
            r#"{
                "type":"track","id":"track-1","name":"Signal Room",
                "artists":[{"id":null,"name":"Mira"}],
                "album":{"id":null,"name":"Night Shift","release_date":null,"images":[{"url":"file:///secret"}]},
                "duration_ms":1,"explicit":false,"external_ids":null
            }"#,
        )
        .expect("sanitized fixture must deserialize");
        assert!(matches!(
            normalize_track(item),
            Err(MusicProviderError::MalformedResponse)
        ));
    }

    #[test]
    fn maps_rate_limits_without_exposing_response_bodies() {
        assert!(matches!(
            map_status(StatusCode::TOO_MANY_REQUESTS),
            MusicProviderError::RateLimited
        ));
    }

    #[test]
    fn retries_only_safe_transient_reads() {
        assert!(should_retry_read(
            &reqwest::Method::GET,
            Some(StatusCode::BAD_GATEWAY),
            None
        ));
        assert!(!should_retry_read(
            &reqwest::Method::POST,
            Some(StatusCode::BAD_GATEWAY),
            None
        ));
        assert!(!should_retry_read(
            &reqwest::Method::GET,
            Some(StatusCode::TOO_MANY_REQUESTS),
            None
        ));
    }

    #[tokio::test]
    async fn sends_device_targeted_pause_seek_and_resume_commands() {
        let server = MockServer::start().await;
        for operation in ["pause", "play"] {
            Mock::given(method("PUT"))
                .and(path(format!("/me/player/{operation}")))
                .and(query_param("device_id", "studio device"))
                .and(header("authorization", "Bearer test-token"))
                .and(header("content-length", "0"))
                .respond_with(ResponseTemplate::new(204))
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/me/player/next"))
            .and(query_param("device_id", "studio device"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("content-length", "0"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/me/player/seek"))
            .and(query_param("position_ms", "0"))
            .and(query_param("device_id", "studio device"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("content-length", "0"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let store = Arc::new(InMemoryCredentialStore::default());
        store
            .save(
                "spotify",
                Credential {
                    access_token: "test-token".into(),
                    refresh_token: None,
                    expires_at: Some(Utc::now() + ChronoDuration::hours(1)),
                    scopes: vec!["user-modify-playback-state".into()],
                },
            )
            .await
            .unwrap_or_else(|error| panic!("credential fixture failed: {error}"));
        let provider = SpotifyProvider::new(
            SpotifyConfiguration {
                client_id: "client".into(),
                redirect_uri: "http://127.0.0.1:43821/callback".into(),
            },
            store,
        )
        .unwrap_or_else(|error| panic!("provider fixture failed: {error}"))
        .with_api_base(server.uri());

        assert!(provider.pause(Some("studio device")).await.is_ok());
        assert!(provider.skip(Some("studio device")).await.is_ok());
        assert!(provider.seek(0, Some("studio device")).await.is_ok());
        assert!(provider.resume(Some("studio device")).await.is_ok());
    }
}
