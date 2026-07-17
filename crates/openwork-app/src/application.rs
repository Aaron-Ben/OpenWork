use std::sync::Arc;

use async_trait::async_trait;
use openwork_core::{
    ApiKeyCipherError, CredentialResolver, ModelCredential, OpenWorkCore, OpenWorkCoreError,
    PostgresProviderRepository, PostgresStorage, StorageError,
};
use openwork_models::{ProviderFactory, provider::ProviderRepository};
use thiserror::Error;

use crate::{ProviderApplicationService, RuntimeApplicationService};

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    database_url: Option<String>,
}

impl ApplicationConfig {
    pub fn from_env_or_local() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").ok(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ApplicationBootstrapError {
    #[error("database bootstrap failed: {0}")]
    Storage(#[from] StorageError),
    #[error("provider credential bootstrap failed: {0}")]
    Credential(#[from] ApiKeyCipherError),
    #[error("runtime core bootstrap failed: {0}")]
    RuntimeCore(#[from] OpenWorkCoreError),
}

/// The single in-process application entry point owned by a host such as Tauri.
pub struct OpenWorkApplication {
    providers: ProviderApplicationService,
    runtime: RuntimeApplicationService,
}

impl OpenWorkApplication {
    pub async fn bootstrap(config: ApplicationConfig) -> Result<Self, ApplicationBootstrapError> {
        let runtime_storage =
            Arc::new(PostgresStorage::connect(config.database_url.as_deref()).await?);
        let provider_repository: Arc<dyn ProviderRepository> = Arc::new(
            PostgresProviderRepository::from_env(runtime_storage.pool().clone())?,
        );
        let credential_resolver: Arc<dyn CredentialResolver> =
            Arc::new(ApplicationCredentialResolver {
                providers: Arc::clone(&provider_repository),
            });
        let runtime =
            OpenWorkCore::from_storage_with_credentials(runtime_storage, credential_resolver)
                .await?;
        let provider_factory = ProviderFactory::default();

        Ok(Self {
            providers: ProviderApplicationService::new(provider_repository, provider_factory),
            runtime: RuntimeApplicationService::new(runtime),
        })
    }

    pub fn providers(&self) -> &ProviderApplicationService {
        &self.providers
    }

    pub fn runtime(&self) -> &RuntimeApplicationService {
        &self.runtime
    }
}

struct ApplicationCredentialResolver {
    providers: Arc<dyn ProviderRepository>,
}

#[async_trait]
impl CredentialResolver for ApplicationCredentialResolver {
    async fn resolve(&self, reference: &str) -> Result<ModelCredential, String> {
        if let Some(provider_id) = reference.strip_prefix("provider:") {
            return self
                .providers
                .load_runtime(provider_id)
                .await
                .map_err(|_| "provider credential is unavailable".to_string())?
                .map(|runtime| runtime.credential)
                .ok_or_else(|| "provider credential is unavailable".to_string());
        }
        std::env::var(reference)
            .map(ModelCredential::new)
            .map_err(|_| "environment credential is unavailable".to_string())
    }
}
