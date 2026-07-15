use std::{net::Ipv4Addr, sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};
use url::Url;

use crate::security::{Credential, CredentialStore, CredentialStoreError};

use super::SpotifyConfiguration;

const PROVIDER: &str = "spotify";
const AUTHORIZATION_ENDPOINT: &str = "https://accounts.spotify.com/authorize";
const TOKEN_ENDPOINT: &str = "https://accounts.spotify.com/api/token";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CALLBACK_BYTES: usize = 8 * 1024;
const SCOPES: [&str; 3] = [
    "user-read-currently-playing",
    "user-read-playback-state",
    "user-modify-playback-state",
];

#[derive(Clone)]
pub struct SpotifyAuthService {
    credential_store: Arc<dyn CredentialStore>,
    http_client: reqwest::Client,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyConnectionStatus {
    pub configured: bool,
    pub connected: bool,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub granted_scopes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum SpotifyAuthError {
    #[error("Spotify Client ID is required")]
    MissingClientId,
    #[error("Spotify redirect URI is invalid")]
    InvalidRedirect,
    #[error("Spotify callback port is unavailable")]
    CallbackUnavailable,
    #[error("Spotify authorization timed out")]
    CallbackTimeout,
    #[error("Spotify authorization callback was invalid")]
    InvalidCallback,
    #[error("Spotify authorization was denied")]
    AuthorizationDenied,
    #[error("Spotify authorization could not open the system browser")]
    BrowserUnavailable,
    #[error("Spotify rejected the authorization response")]
    TokenExchangeFailed,
    #[error("Windows Credential Manager is unavailable")]
    CredentialStoreUnavailable,
}

struct CallbackResult {
    code: AuthorizationCode,
    state: CsrfToken,
}

impl SpotifyAuthService {
    pub fn new(credential_store: Arc<dyn CredentialStore>) -> Result<Self, SpotifyAuthError> {
        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| SpotifyAuthError::TokenExchangeFailed)?;
        Ok(Self {
            credential_store,
            http_client,
        })
    }

    pub async fn connect(
        &self,
        app: &AppHandle,
        configuration: &SpotifyConfiguration,
    ) -> Result<SpotifyConnectionStatus, SpotifyAuthError> {
        let redirect = validate_redirect_uri(&configuration.redirect_uri)?;
        if configuration.client_id.trim().is_empty() {
            return Err(SpotifyAuthError::MissingClientId);
        }
        let port = redirect.port().ok_or(SpotifyAuthError::InvalidRedirect)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| SpotifyAuthError::CallbackUnavailable)?;

        let client = BasicClient::new(ClientId::new(configuration.client_id.clone()))
            .set_auth_uri(
                AuthUrl::new(AUTHORIZATION_ENDPOINT.to_owned())
                    .map_err(|_| SpotifyAuthError::TokenExchangeFailed)?,
            )
            .set_token_uri(
                TokenUrl::new(TOKEN_ENDPOINT.to_owned())
                    .map_err(|_| SpotifyAuthError::TokenExchangeFailed)?,
            )
            .set_redirect_uri(
                RedirectUrl::new(configuration.redirect_uri.clone())
                    .map_err(|_| SpotifyAuthError::InvalidRedirect)?,
            );
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge);
        for scope in SCOPES {
            request = request.add_scope(Scope::new(scope.to_owned()));
        }
        let (authorization_url, expected_state) = request.url();

        app.opener()
            .open_url(authorization_url.as_str(), None::<&str>)
            .map_err(|_| SpotifyAuthError::BrowserUnavailable)?;
        let callback = timeout(
            CALLBACK_TIMEOUT,
            receive_callback(listener, redirect.path()),
        )
        .await
        .map_err(|_| SpotifyAuthError::CallbackTimeout)??;
        if callback.state.secret() != expected_state.secret() {
            return Err(SpotifyAuthError::InvalidCallback);
        }

        let token = client
            .exchange_code(callback.code)
            .set_pkce_verifier(verifier)
            .request_async(&self.http_client)
            .await
            .map_err(|_| SpotifyAuthError::TokenExchangeFailed)?;
        let expires_at = token
            .expires_in()
            .and_then(|duration| ChronoDuration::from_std(duration).ok())
            .map(|duration| Utc::now() + duration);
        let granted_scopes: Vec<String> = token
            .scopes()
            .map(|scopes| {
                scopes
                    .iter()
                    .map(|scope| scope.as_ref().to_owned())
                    .collect()
            })
            .unwrap_or_else(|| SCOPES.iter().map(|scope| (*scope).to_owned()).collect());
        let credential = Credential {
            access_token: token.access_token().secret().to_owned(),
            refresh_token: token.refresh_token().map(|token| token.secret().to_owned()),
            expires_at,
            scopes: granted_scopes.clone(),
        };
        self.credential_store
            .save(PROVIDER, credential)
            .await
            .map_err(map_store_error)?;
        Ok(SpotifyConnectionStatus {
            configured: true,
            connected: true,
            expires_at,
            granted_scopes,
        })
    }

    pub async fn status(&self, configured: bool) -> SpotifyConnectionStatus {
        match self.credential_store.load(PROVIDER).await {
            Ok(credential) => SpotifyConnectionStatus {
                configured,
                connected: credential
                    .expires_at
                    .is_none_or(|expires_at| expires_at > Utc::now()),
                expires_at: credential.expires_at,
                granted_scopes: credential.scopes,
            },
            Err(_) => SpotifyConnectionStatus {
                configured,
                connected: false,
                expires_at: None,
                granted_scopes: Vec::new(),
            },
        }
    }

    pub async fn disconnect(
        &self,
        configured: bool,
    ) -> Result<SpotifyConnectionStatus, SpotifyAuthError> {
        self.credential_store
            .delete(PROVIDER)
            .await
            .map_err(map_store_error)?;
        Ok(SpotifyConnectionStatus {
            configured,
            connected: false,
            expires_at: None,
            granted_scopes: Vec::new(),
        })
    }
}

fn map_store_error(_: CredentialStoreError) -> SpotifyAuthError {
    SpotifyAuthError::CredentialStoreUnavailable
}

fn validate_redirect_uri(value: &str) -> Result<Url, SpotifyAuthError> {
    let url = Url::parse(value).map_err(|_| SpotifyAuthError::InvalidRedirect)?;
    let valid = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/callback"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    valid
        .then_some(url)
        .ok_or(SpotifyAuthError::InvalidRedirect)
}

async fn receive_callback(
    listener: TcpListener,
    expected_path: &str,
) -> Result<CallbackResult, SpotifyAuthError> {
    let (mut stream, peer) = listener
        .accept()
        .await
        .map_err(|_| SpotifyAuthError::InvalidCallback)?;
    if !peer.ip().is_loopback() {
        return Err(SpotifyAuthError::InvalidCallback);
    }
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|_| SpotifyAuthError::InvalidCallback)?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() >= MAX_CALLBACK_BYTES {
            return Err(SpotifyAuthError::InvalidCallback);
        }
    }
    let result = parse_callback_request(&request, expected_path);
    let (status, body) = if result.is_ok() {
        ("200 OK", "Spotify connected. You can return to Sanymar.")
    } else {
        (
            "400 Bad Request",
            "Spotify connection failed. Return to Sanymar and try again.",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    result
}

fn parse_callback_request(
    request: &[u8],
    expected_path: &str,
) -> Result<CallbackResult, SpotifyAuthError> {
    let request = std::str::from_utf8(request).map_err(|_| SpotifyAuthError::InvalidCallback)?;
    let request_line = request
        .lines()
        .next()
        .ok_or(SpotifyAuthError::InvalidCallback)?;
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(SpotifyAuthError::InvalidCallback);
    }
    let target = parts.next().ok_or(SpotifyAuthError::InvalidCallback)?;
    let url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| SpotifyAuthError::InvalidCallback)?;
    if url.path() != expected_path || url.fragment().is_some() {
        return Err(SpotifyAuthError::InvalidCallback);
    }
    if url.query_pairs().any(|(key, _)| key == "error") {
        return Err(SpotifyAuthError::AuthorizationDenied);
    }
    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| AuthorizationCode::new(value.into_owned()))
        .ok_or(SpotifyAuthError::InvalidCallback)?;
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| CsrfToken::new(value.into_owned()))
        .ok_or(SpotifyAuthError::InvalidCallback)?;
    Ok(CallbackResult { code, state })
}

#[cfg(test)]
mod tests {
    use super::{parse_callback_request, validate_redirect_uri, SpotifyAuthError};

    #[test]
    fn accepts_only_fixed_ipv4_loopback_callback() {
        assert!(validate_redirect_uri("http://127.0.0.1:43821/callback").is_ok());
        assert!(validate_redirect_uri("http://localhost:43821/callback").is_err());
        assert!(validate_redirect_uri("http://127.0.0.1:43821/other").is_err());
        assert!(validate_redirect_uri("https://127.0.0.1:43821/callback").is_err());
    }

    #[test]
    fn parses_code_and_state_without_accepting_other_paths() {
        let request = b"GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let callback = parse_callback_request(request, "/callback");
        assert!(callback.is_ok());
        let wrong_path = b"GET /other?code=abc&state=xyz HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_callback_request(wrong_path, "/callback"),
            Err(SpotifyAuthError::InvalidCallback)
        ));
    }

    #[test]
    fn maps_oauth_denial_to_typed_error() {
        let request = b"GET /callback?error=access_denied&state=xyz HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_callback_request(request, "/callback"),
            Err(SpotifyAuthError::AuthorizationDenied)
        ));
    }
}
