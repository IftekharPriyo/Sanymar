use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credential {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum CredentialStoreError {
    #[error("credential was not found")]
    NotFound,
    #[error("operating-system credential storage is unavailable")]
    Unavailable,
    #[error("stored credential is invalid")]
    Invalid,
}

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn save(
        &self,
        provider: &str,
        credential: Credential,
    ) -> Result<(), CredentialStoreError>;
    async fn load(&self, provider: &str) -> Result<Credential, CredentialStoreError>;
    async fn delete(&self, provider: &str) -> Result<(), CredentialStoreError>;
}

pub mod mock;
#[cfg(target_os = "windows")]
pub mod windows;
