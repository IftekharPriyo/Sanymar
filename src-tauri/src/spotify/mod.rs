//! Spotify-specific authentication and API adapters.

pub mod api;
pub mod auth;

/// Provider-specific configuration that contains no secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyConfiguration {
    pub client_id: String,
    pub redirect_uri: String,
}
