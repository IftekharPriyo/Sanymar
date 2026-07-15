use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::{Credential, CredentialStore, CredentialStoreError};

#[derive(Default)]
pub struct InMemoryCredentialStore {
    credentials: RwLock<HashMap<String, Credential>>,
}

#[async_trait]
impl CredentialStore for InMemoryCredentialStore {
    async fn save(
        &self,
        provider: &str,
        credential: Credential,
    ) -> Result<(), CredentialStoreError> {
        self.credentials
            .write()
            .await
            .insert(provider.to_owned(), credential);
        Ok(())
    }

    async fn load(&self, provider: &str) -> Result<Credential, CredentialStoreError> {
        self.credentials
            .read()
            .await
            .get(provider)
            .cloned()
            .ok_or(CredentialStoreError::NotFound)
    }

    async fn delete(&self, provider: &str) -> Result<(), CredentialStoreError> {
        self.credentials.write().await.remove(provider);
        Ok(())
    }
}
