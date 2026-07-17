use std::sync::Arc;

use async_trait::async_trait;
use openwork_core::{
    CredentialResolver, ModelCredential, OpenWorkCore, OpenWorkCoreError, PostgresStorage,
};
use openwork_observability::TraceRuntime;
use openwork_persistence::{DatabaseConfig, PostgresPersistence, PostgresPersistenceError};
use openwork_protocol::{provider::ProviderRepository, trace::TraceRepository};
use openwork_providers::ProviderFactory;
use thiserror::Error;

use crate::{
    ChatRuntime, ProviderApplicationService, RuntimeApplicationService, SessionApplicationService,
    TraceApplicationService, TurnApplicationService,
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
    #[error("runtime core bootstrap failed: {0}")]
    RuntimeCore(#[from] OpenWorkCoreError),
}

/// The single in-process application entry point owned by a host such as Tauri.
pub struct OpenWorkApplication {
    providers: ProviderApplicationService,
    sessions: SessionApplicationService,
    turns: TurnApplicationService,
    traces: TraceApplicationService,
    runtime: RuntimeApplicationService,
}

impl OpenWorkApplication {
    pub async fn bootstrap(config: ApplicationConfig) -> Result<Self, ApplicationBootstrapError> {
        let persistence = PostgresPersistence::connect(config.database).await?;
        let provider_repository: Arc<dyn ProviderRepository> =
            Arc::new(persistence.provider_repository());
        let runtime_storage = Arc::new(PostgresStorage::from_pool(persistence.pool().clone()));
        let credential_resolver: Arc<dyn CredentialResolver> =
            Arc::new(ApplicationCredentialResolver {
                providers: Arc::clone(&provider_repository),
            });
        let runtime =
            OpenWorkCore::from_storage_with_credentials(runtime_storage, credential_resolver)
                .await?;
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
            sessions: SessionApplicationService::new(session_store.clone()),
            turns: TurnApplicationService::new(chat_runtime),
            traces: TraceApplicationService::with_session_store(trace_repository, session_store),
            runtime: RuntimeApplicationService::new(runtime),
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
