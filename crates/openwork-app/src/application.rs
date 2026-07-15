use std::sync::Arc;

use openwork_observability::TraceRuntime;
use openwork_persistence::{DatabaseConfig, PostgresPersistence, PostgresPersistenceError};
use openwork_protocol::{provider::ProviderRepository, trace::TraceRepository};
use openwork_providers::ProviderFactory;
use thiserror::Error;

use crate::{
    ChatRuntime, ProviderApplicationService, SessionApplicationService, TraceApplicationService,
    TurnApplicationService,
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
    sessions: SessionApplicationService,
    turns: TurnApplicationService,
    traces: TraceApplicationService,
}

impl OpenWorkApplication {
    pub async fn bootstrap(config: ApplicationConfig) -> Result<Self, ApplicationBootstrapError> {
        let persistence = PostgresPersistence::connect(config.database).await?;
        let provider_repository: Arc<dyn ProviderRepository> =
            Arc::new(persistence.provider_repository());
        let session_store = persistence.session_store();
        let trace_repository: Arc<dyn TraceRepository> = Arc::new(persistence.trace_repository());
        let trace_runtime = TraceRuntime::new(Arc::clone(&trace_repository));
        let provider_factory = ProviderFactory::default();

        let chat_runtime = ChatRuntime::new(
            Arc::clone(&provider_repository),
            session_store.clone(),
            provider_factory.clone(),
            trace_runtime,
        );

        Ok(Self {
            providers: ProviderApplicationService::new(provider_repository, provider_factory),
            sessions: SessionApplicationService::new(session_store),
            turns: TurnApplicationService::new(chat_runtime),
            traces: TraceApplicationService::new(trace_repository),
        })
    }

    pub fn providers(&self) -> &ProviderApplicationService {
        &self.providers
    }

    pub fn sessions(&self) -> &SessionApplicationService {
        &self.sessions
    }

    pub fn turns(&self) -> &TurnApplicationService {
        &self.turns
    }

    pub fn traces(&self) -> &TraceApplicationService {
        &self.traces
    }
}
