use async_trait::async_trait;
use keyring::{Entry, Error as KeyringError};

use super::{Credential, CredentialStore, CredentialStoreError};

const SERVICE: &str = "com.sanymar.desktop";
const USERNAME_PREFIX: &str = "oauth";

#[derive(Default)]
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    fn username(provider: &str) -> String {
        format!("{USERNAME_PREFIX}.{provider}")
    }

    fn map_error(error: KeyringError) -> CredentialStoreError {
        match error {
            KeyringError::NoEntry => CredentialStoreError::NotFound,
            _ => CredentialStoreError::Unavailable,
        }
    }
}

#[async_trait]
impl CredentialStore for WindowsCredentialStore {
    async fn save(
        &self,
        provider: &str,
        credential: Credential,
    ) -> Result<(), CredentialStoreError> {
        let username = Self::username(provider);
        let serialized =
            serde_json::to_string(&credential).map_err(|_| CredentialStoreError::Invalid)?;
        tokio::task::spawn_blocking(move || {
            Entry::new(SERVICE, &username)
                .map_err(Self::map_error)?
                .set_password(&serialized)
                .map_err(Self::map_error)
        })
        .await
        .map_err(|_| CredentialStoreError::Unavailable)?
    }

    async fn load(&self, provider: &str) -> Result<Credential, CredentialStoreError> {
        let username = Self::username(provider);
        let serialized = tokio::task::spawn_blocking(move || {
            Entry::new(SERVICE, &username)
                .map_err(Self::map_error)?
                .get_password()
                .map_err(Self::map_error)
        })
        .await
        .map_err(|_| CredentialStoreError::Unavailable)??;
        serde_json::from_str(&serialized).map_err(|_| CredentialStoreError::Invalid)
    }

    async fn delete(&self, provider: &str) -> Result<(), CredentialStoreError> {
        let username = Self::username(provider);
        let result = tokio::task::spawn_blocking(move || {
            Entry::new(SERVICE, &username)
                .map_err(Self::map_error)?
                .delete_credential()
                .map_err(Self::map_error)
        })
        .await
        .map_err(|_| CredentialStoreError::Unavailable)?;
        match result {
            Err(CredentialStoreError::NotFound) | Ok(()) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
