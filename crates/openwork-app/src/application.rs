use std::sync::Arc;

use openwork_persistence::{DatabaseConfig, PostgresPersistence, PostgresPersistenceError};
use openwork_protocol::provider::ProviderRepository;
use openwork_providers::ProviderFactory;
use thiserror::Error;

use crate::{
    ChatRuntime, ProviderApplicationService, ThreadApplicationService, TurnApplicationService,
};

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    database: DatabaseConfig,
}

impl ApplicationConfig {
    pub fn from_env_or_local() -> Self {
        Self {
            database: DatabaseConfig::from_env_or_local(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ApplicationBootstrapError {
    #[error("persistence bootstrap failed: {0}")]
    Persistence(#[from] PostgresPersistenceError),
}

/// The single in-process application entry point owned by a host such as Tauri.
pub struct OpenWorkApplication {
    providers: ProviderApplicationService,
    threads: ThreadApplicationService,
    turns: TurnApplicationService,
}

impl OpenWorkApplication {
    pub async fn bootstrap(config: ApplicationConfig) -> Result<Self, ApplicationBootstrapError> {
        let persistence = PostgresPersistence::connect(config.database).await?;
        let provider_repository: Arc<dyn ProviderRepository> =
            Arc::new(persistence.provider_repository());
        let session_store = persistence.session_store();
        let provider_factory = ProviderFactory::default();

        let chat_runtime = ChatRuntime::new(
            Arc::clone(&provider_repository),
            session_store.clone(),
            provider_factory.clone(),
        );

        Ok(Self {
            providers: ProviderApplicationService::new(provider_repository, provider_factory),
            threads: ThreadApplicationService::new(session_store),
            turns: TurnApplicationService::new(chat_runtime),
        })
    }

    pub fn providers(&self) -> &ProviderApplicationService {
        &self.providers
    }

    pub fn threads(&self) -> &ThreadApplicationService {
        &self.threads
    }

    pub fn turns(&self) -> &TurnApplicationService {
        &self.turns
    }
}
