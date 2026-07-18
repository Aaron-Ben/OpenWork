use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use openwork_agent::{AgentBuilder, AgentDefinition};
use openwork_chat_state::ChatStateHandle;
use openwork_models::ProviderFactory;
use openwork_models::model::ContentBlock;
use openwork_models::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderProfile,
    ProviderRepository, ProviderRepositoryError, ProviderRuntimeConfig,
};
use openwork_tools::{PermissionProfile, ToolSessionContext, builtin_registry};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::provider::{BUILTIN_PRESETS, ProviderIndex, ProviderPreset, ProviderTestResult};
use crate::session::{
    ClientRequestId, PermissionDecision, ResolvedModel, SessionError, SessionHandle, SessionId,
    SessionRuntimeConfig, SessionSnapshot, SessionUpdateEnvelope, ToolCallId, TurnAccepted, TurnId,
};
use crate::storage::{
    ApiKeyCipherError, ModelInput, ModelRecord, PostgresProviderRepository, PostgresStorage,
    PostgresTraceRecorder, SessionInput, SessionRecord, StorageError, StoredMessageRecord,
    TraceSpanRecord, TraceTurnSummary,
};

const CORE_UPDATE_BROADCAST_CAPACITY: usize = 4096;

#[derive(Debug, Clone)]
pub struct OpenWorkCoreConfig {
    pub database_url: Option<String>,
}

impl OpenWorkCoreConfig {
    pub fn from_env_or_local() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").ok(),
        }
    }
}

#[async_trait]
pub trait CredentialResolver: Send + Sync {
    async fn resolve(&self, reference: &str) -> Result<ApiCredential, String>;
}

#[derive(Debug, Default)]
pub struct EnvironmentCredentialResolver;

#[async_trait]
impl CredentialResolver for EnvironmentCredentialResolver {
    async fn resolve(&self, reference: &str) -> Result<ApiCredential, String> {
        std::env::var(reference)
            .map(ApiCredential::new)
            .map_err(|_| format!("environment variable is unavailable: {reference}"))
    }
}

struct ProviderCredentialResolver {
    providers: Arc<dyn ProviderRepository>,
}

#[async_trait]
impl CredentialResolver for ProviderCredentialResolver {
    async fn resolve(&self, reference: &str) -> Result<ApiCredential, String> {
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
            .map(ApiCredential::new)
            .map_err(|_| "environment credential is unavailable".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSession {
    pub session: SessionRecord,
    pub messages: Vec<StoredMessageRecord>,
}

#[derive(Debug, Error)]
pub enum OpenWorkCoreError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("session has no default model: {0}")]
    DefaultModelMissing(String),
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("session has an active turn and cannot be changed: {0}")]
    SessionActive(String),
    #[error("model is disabled: {0}")]
    ModelDisabled(String),
    #[error("model credential reference is missing: {0}")]
    CredentialReferenceMissing(String),
    #[error("credential environment variable is unavailable: {0}")]
    CredentialUnavailable(String),
    #[error("unsupported provider kind: {0}")]
    UnsupportedProvider(String),
    #[error("runtime component failed: {0}")]
    RuntimeComponent(String),
    #[error(transparent)]
    Provider(#[from] ProviderRepositoryError),
    #[error("provider credential bootstrap failed: {0}")]
    CredentialBootstrap(#[from] ApiKeyCipherError),
    #[error("provider repository is unavailable in this core configuration")]
    ProviderRepositoryUnavailable,
}

pub struct OpenWorkCore {
    storage: Arc<PostgresStorage>,
    provider_factory: ProviderFactory,
    trace: Arc<PostgresTraceRecorder>,
    credentials: Arc<dyn CredentialResolver>,
    providers: Option<Arc<dyn ProviderRepository>>,
    update_tx: broadcast::Sender<SessionUpdateEnvelope>,
    sessions: RwLock<HashMap<SessionId, SessionHandle>>,
    session_creation: Mutex<()>,
}

impl OpenWorkCore {
    pub async fn bootstrap(config: OpenWorkCoreConfig) -> Result<Self, OpenWorkCoreError> {
        let storage = Arc::new(
            PostgresStorage::connect(config.database_url.as_deref())
                .await
                .map_err(OpenWorkCoreError::Storage)?,
        );
        let providers: Arc<dyn ProviderRepository> = Arc::new(
            PostgresProviderRepository::from_env(storage.pool().clone())?,
        );
        let credentials: Arc<dyn CredentialResolver> = Arc::new(ProviderCredentialResolver {
            providers: Arc::clone(&providers),
        });
        Self::from_storage_parts(storage, credentials, Some(providers)).await
    }

    pub async fn from_storage(storage: Arc<PostgresStorage>) -> Result<Self, OpenWorkCoreError> {
        Self::from_storage_parts(storage, Arc::new(EnvironmentCredentialResolver), None).await
    }

    pub async fn from_storage_with_credentials(
        storage: Arc<PostgresStorage>,
        credentials: Arc<dyn CredentialResolver>,
    ) -> Result<Self, OpenWorkCoreError> {
        Self::from_storage_parts(storage, credentials, None).await
    }

    async fn from_storage_parts(
        storage: Arc<PostgresStorage>,
        credentials: Arc<dyn CredentialResolver>,
        providers: Option<Arc<dyn ProviderRepository>>,
    ) -> Result<Self, OpenWorkCoreError> {
        storage.migrate().await?;
        storage.mark_running_interrupted().await?;
        let trace = Arc::new(PostgresTraceRecorder::spawn(storage.pool().clone()));
        let (update_tx, _) = broadcast::channel(CORE_UPDATE_BROADCAST_CAPACITY);
        Ok(Self {
            storage,
            provider_factory: ProviderFactory::default(),
            trace,
            credentials,
            providers,
            update_tx,
            sessions: RwLock::new(HashMap::new()),
            session_creation: Mutex::new(()),
        })
    }

    pub fn storage(&self) -> &PostgresStorage {
        &self.storage
    }

    pub async fn list_providers(&self) -> Result<ProviderIndex, OpenWorkCoreError> {
        let providers = self.provider_repository()?;
        Ok(ProviderIndex {
            providers: providers.list_profiles().await?,
        })
    }

    pub fn provider_presets(&self) -> Vec<ProviderPreset> {
        BUILTIN_PRESETS.to_vec()
    }

    pub async fn create_provider(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderProfile, OpenWorkCoreError> {
        Ok(self.provider_repository()?.create(input).await?)
    }

    pub async fn update_provider(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, OpenWorkCoreError> {
        Ok(self.provider_repository()?.update(id, input).await?)
    }

    pub async fn delete_provider(&self, id: &str) -> Result<(), OpenWorkCoreError> {
        Ok(self.provider_repository()?.delete(id).await?)
    }

    pub async fn test_provider(
        &self,
        id: Option<String>,
        input: Option<ProviderInput>,
        model: &str,
    ) -> Result<ProviderTestResult, OpenWorkCoreError> {
        let config = if let Some(id) = id {
            self.provider_repository()?
                .load_runtime(&id)
                .await?
                .ok_or(ProviderRepositoryError::NotFound { id })?
        } else if let Some(input) = input {
            ProviderRuntimeConfig {
                profile: ProviderProfile {
                    id: "draft".to_string(),
                    name: input.name,
                    base_url: input.base_url,
                    kind: input.kind,
                    models: input.models,
                    enabled: input.enabled,
                },
                credential: ApiCredential::new(input.api_key),
                adapter_options: input.extra_body,
            }
        } else {
            return Err(ProviderRepositoryError::InvalidInput { field: "id/input" }.into());
        };

        match self.provider_factory.test(&config, model).await {
            Ok(()) => Ok(ProviderTestResult {
                success: true,
                message: "Connectivity OK".to_string(),
            }),
            Err(error) => Ok(ProviderTestResult::failed(error.to_string())),
        }
    }

    fn provider_repository(&self) -> Result<&dyn ProviderRepository, OpenWorkCoreError> {
        self.providers
            .as_deref()
            .ok_or(OpenWorkCoreError::ProviderRepositoryUnavailable)
    }

    pub async fn register_model(&self, input: &ModelInput) -> Result<(), OpenWorkCoreError> {
        self.storage.upsert_model(input).await?;
        Ok(())
    }

    pub async fn create_session(
        &self,
        input: &SessionInput,
    ) -> Result<SessionRecord, OpenWorkCoreError> {
        Ok(self.storage.create_session(input).await?)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, OpenWorkCoreError> {
        Ok(self.storage.list_sessions().await?)
    }

    pub async fn load_session(
        &self,
        session_id: &SessionId,
    ) -> Result<LoadedSession, OpenWorkCoreError> {
        let session = self
            .storage
            .load_session(session_id)
            .await?
            .ok_or_else(|| OpenWorkCoreError::SessionNotFound(session_id.to_string()))?;
        let messages = self.storage.load_message_records(session_id).await?;
        Ok(LoadedSession { session, messages })
    }

    pub async fn rename_session(
        &self,
        session_id: &SessionId,
        title: &str,
    ) -> Result<SessionRecord, OpenWorkCoreError> {
        Ok(self.storage.rename_session(session_id, title).await?)
    }

    pub async fn delete_session(&self, session_id: &SessionId) -> Result<(), OpenWorkCoreError> {
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned()
            && matches!(
                handle.snapshot().await?.runtime,
                crate::session::SessionRuntimeSnapshot::Running { .. }
            )
        {
            return Err(OpenWorkCoreError::SessionActive(session_id.to_string()));
        }
        self.sessions.write().await.remove(session_id);
        self.storage.delete_session(session_id).await?;
        Ok(())
    }

    pub async fn start_turn(
        &self,
        session_id: &SessionId,
        client_request_id: ClientRequestId,
        input: Vec<ContentBlock>,
    ) -> Result<TurnAccepted, OpenWorkCoreError> {
        let handle = self.session_handle(session_id).await?;
        Ok(handle.start_turn(client_request_id, input).await?)
    }

    pub async fn cancel_turn(
        &self,
        session_id: &SessionId,
        turn_id: TurnId,
    ) -> Result<bool, OpenWorkCoreError> {
        let handle = self.session_handle(session_id).await?;
        Ok(handle.cancel_turn(turn_id).await?)
    }

    pub async fn resolve_permission(
        &self,
        session_id: &SessionId,
        turn_id: TurnId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    ) -> Result<(), OpenWorkCoreError> {
        let handle = self.session_handle(session_id).await?;
        handle
            .resolve_permission(turn_id, tool_call_id, decision)
            .await?;
        Ok(())
    }

    pub fn subscribe_updates(&self) -> broadcast::Receiver<SessionUpdateEnvelope> {
        self.update_tx.subscribe()
    }

    pub async fn get_session_snapshot(
        &self,
        session_id: &SessionId,
    ) -> Result<SessionSnapshot, OpenWorkCoreError> {
        Ok(self.session_handle(session_id).await?.snapshot().await?)
    }

    pub async fn replay_updates(
        &self,
        session_id: &SessionId,
        after_sequence: u64,
    ) -> Result<Vec<SessionUpdateEnvelope>, OpenWorkCoreError> {
        Ok(self
            .session_handle(session_id)
            .await?
            .replay_updates(after_sequence)
            .await?)
    }

    pub async fn list_traces(
        &self,
        session_id: Option<&SessionId>,
        limit: i64,
    ) -> Result<Vec<TraceTurnSummary>, OpenWorkCoreError> {
        Ok(self.storage.list_traces(session_id, limit).await?)
    }

    pub async fn get_trace(
        &self,
        turn_id: &TurnId,
    ) -> Result<Vec<TraceSpanRecord>, OpenWorkCoreError> {
        Ok(self.storage.get_trace(turn_id).await?)
    }

    async fn session_handle(
        &self,
        session_id: &SessionId,
    ) -> Result<SessionHandle, OpenWorkCoreError> {
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned() {
            return Ok(handle);
        }

        let _creation = self.session_creation.lock().await;
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned() {
            return Ok(handle);
        }

        let handle = self.build_session_handle(session_id).await?;
        self.sessions
            .write()
            .await
            .insert(session_id.clone(), handle.clone());
        Ok(handle)
    }

    async fn build_session_handle(
        &self,
        session_id: &SessionId,
    ) -> Result<SessionHandle, OpenWorkCoreError> {
        let loaded = self.load_session(session_id).await?;
        let model_id = loaded
            .session
            .default_model_id
            .as_deref()
            .ok_or_else(|| OpenWorkCoreError::DefaultModelMissing(session_id.to_string()))?;
        let model = self
            .storage
            .load_model(model_id)
            .await?
            .ok_or_else(|| OpenWorkCoreError::ModelNotFound(model_id.to_string()))?;
        if !model.enabled {
            return Err(OpenWorkCoreError::ModelDisabled(model.id));
        }
        let runtime = provider_runtime(&model, self.credentials.as_ref()).await?;
        let model_port = Arc::from(self.provider_factory.build(&runtime));
        let agent = AgentBuilder::new(AgentDefinition::default())
            .build()
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let conversation = self.storage.load_messages(session_id).await?;
        let chat = ChatStateHandle::spawn(conversation)
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let working_directory = PathBuf::from(&loaded.session.working_directory);
        let tools = builtin_registry()
            .finalize(
                agent.toolset_config(),
                ToolSessionContext::local(
                    working_directory.clone(),
                    PermissionProfile::workspace_write(working_directory),
                ),
            )
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;

        Ok(SessionHandle::spawn_with_global_updates(
            SessionRuntimeConfig {
                session_id: session_id.clone(),
                resolved_model: ResolvedModel::new(
                    Some(model.id),
                    model.provider_kind,
                    model.model_name,
                ),
                agent,
                chat,
                model: model_port,
                tools: Arc::new(tools),
                storage: self.storage.clone(),
                trace: self.trace.clone(),
            },
            self.update_tx.clone(),
        ))
    }
}

async fn provider_runtime(
    model: &ModelRecord,
    credentials: &dyn CredentialResolver,
) -> Result<ProviderRuntimeConfig, OpenWorkCoreError> {
    let provider_kind = parse_provider_kind(&model.provider_kind)?;
    let credential_ref = model
        .credential_ref
        .as_deref()
        .ok_or_else(|| OpenWorkCoreError::CredentialReferenceMissing(model.id.clone()))?;
    let credential = credentials
        .resolve(credential_ref)
        .await
        .map_err(|_| OpenWorkCoreError::CredentialUnavailable(credential_ref.to_string()))?;
    let adapter_options = model
        .config
        .get("extraBody")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .filter(|map| !map.is_empty());
    Ok(ProviderRuntimeConfig {
        profile: ProviderProfile {
            id: model.id.clone(),
            name: model.display_name.clone(),
            base_url: model.base_url.clone(),
            kind: provider_kind,
            models: vec![ProviderModel {
                model_id: model.model_name.clone(),
                display_name: Some(model.display_name.clone()),
                model_tier: ModelTier::Plus,
                enabled: model.enabled,
            }],
            enabled: model.enabled,
        },
        credential,
        adapter_options,
    })
}

fn parse_provider_kind(value: &str) -> Result<ProviderKind, OpenWorkCoreError> {
    match value {
        "openai" | "openai_responses" => Ok(ProviderKind::Openai),
        "anthropic" | "anthropic_messages" => Ok(ProviderKind::Anthropic),
        "deepseek" | "openai_chat_deepseek" => Ok(ProviderKind::Deepseek),
        "kimi" | "openai_chat_kimi" => Ok(ProviderKind::Kimi),
        "qwen" | "openai_chat_qwen" => Ok(ProviderKind::Qwen),
        "glm" | "openai_chat_glm" => Ok(ProviderKind::Glm),
        other => Err(OpenWorkCoreError::UnsupportedProvider(other.to_string())),
    }
}
