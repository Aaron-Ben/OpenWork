use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use openwork_agent::{Agent, AgentBuilder, AgentDefinition};
use openwork_chat_state::{ChatStateHandle, ConversationView};
use openwork_models::ProviderFactory;
use openwork_models::model::{ContentBlock, Message, Role};
use openwork_models::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderProfile,
    ProviderRepository, ProviderRepositoryError, ProviderRuntimeConfig,
};
use openwork_tools::{
    FileChangeArtifact, FileChangeReapplyError, FileChangeUndoError, FinalizedToolset,
    PermissionProfile, ReapplyFileChangesResult, ToolSessionContext, UndoFileChangesResult,
    builtin_registry, reapply_file_changes as reapply_workspace_file_changes,
    undo_file_changes as undo_workspace_file_changes,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock, broadcast};

use crate::context::{
    CONTEXT_WINDOW_INSPECTION_SCHEMA_VERSION, ContextInspectionBudget, ContextInspectionMessage,
    ContextInspectionSystemPart, ContextWindowInspection, SystemContextBuilder,
};
use crate::model_call::{ModelRequestBuilder, ModelRequestInput};
use crate::provider::{BUILTIN_PRESETS, ProviderIndex, ProviderPreset, ProviderTestResult};
use crate::session::{
    ClientRequestId, PermissionDecision, ResolvedModel, SessionError, SessionHandle, SessionId,
    SessionRuntimeConfig, SessionSnapshot, SessionUpdateEnvelope, ToolCallId, TurnAccepted, TurnId,
};
use crate::storage::{
    ApiKeyCipherError, ModelInput, ModelRecord, PostgresProviderRepository, PostgresStorage,
    PostgresTraceRecorder, SessionInput, SessionRecord, StorageError, StoredMessageRecord,
    TraceTurnSummary, TurnTrace,
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
    #[error("file change not found: {0}")]
    FileChangeNotFound(String),
    #[error("file change has already been undone: {0}")]
    FileChangeAlreadyUndone(String),
    #[error("file change has not been undone: {0}")]
    FileChangeNotUndone(String),
    #[error(transparent)]
    FileChangeUndo(#[from] FileChangeUndoError),
    #[error(transparent)]
    FileChangeReapply(#[from] FileChangeReapplyError),
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
    workspace_operations: Mutex<HashMap<SessionId, Weak<Mutex<()>>>>,
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
            workspace_operations: Mutex::new(HashMap::new()),
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

    /// Resolves a read-only preview of the three input regions that would be
    /// assembled from the session's current authoritative sources.
    ///
    /// The preview reads live System Context sources, persisted Conversation,
    /// and the current Tool Surface. It does not call a provider or persist a
    /// duplicate request snapshot. An already-running Turn may continue using
    /// the System Context it resolved at Turn start.
    pub async fn inspect_context_window(
        &self,
        session_id: &SessionId,
    ) -> Result<ContextWindowInspection, OpenWorkCoreError> {
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
        let working_directory = PathBuf::from(&loaded.session.working_directory);
        let (agent, tools) = build_default_agent_and_tools(&working_directory)?;
        let system_context = SystemContextBuilder::new(&working_directory)
            .build(agent.system_prompt())
            .await
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;

        let current_turn_id = loaded
            .messages
            .iter()
            .rev()
            .find_map(|message| message.turn_id.clone());
        let mut conversation = Vec::with_capacity(loaded.messages.len());
        let mut inspected_messages = Vec::with_capacity(loaded.messages.len());
        for message in loaded.messages {
            if message.role == Role::System {
                return Err(OpenWorkCoreError::RuntimeComponent(
                    "persisted system messages are not valid Conversation input".to_string(),
                ));
            }
            conversation.push(Message {
                role: message.role,
                content: message.content.clone(),
            });
            inspected_messages.push(ContextInspectionMessage {
                message_id: message.id,
                turn_id: message.turn_id,
                role: message.role,
                content: message.content,
            });
        }

        let prepared = ModelRequestBuilder::build(ModelRequestInput::new(
            &model.model_name,
            &system_context,
            ConversationView {
                messages: conversation,
            },
            tools.definitions(),
        ))
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let budget = prepared.context_budget;

        Ok(ContextWindowInspection {
            schema_version: CONTEXT_WINDOW_INSPECTION_SCHEMA_VERSION,
            session_id: loaded.session.id,
            current_turn_id,
            resolved_model_name: model.model_name,
            system_context: system_context
                .parts()
                .iter()
                .map(|part| ContextInspectionSystemPart {
                    source_key: part.key.clone(),
                    content: part.content.clone(),
                })
                .collect(),
            conversation: inspected_messages,
            tool_surface: prepared.request.tools,
            budget: ContextInspectionBudget {
                system_context_tokens: budget.system_context_tokens,
                conversation_tokens: budget.conversation_tokens,
                tool_surface_tokens: budget.tool_surface_tokens,
                estimated_input_tokens: budget.estimated_input_tokens,
                reserved_output_tokens: budget.reserved_output_tokens,
            },
        })
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
        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        let handle = self.session_handle(session_id).await?;
        Ok(handle.start_turn(client_request_id, input).await?)
    }

    pub async fn undo_file_changes(
        &self,
        session_id: &SessionId,
        change_ids: Vec<String>,
    ) -> Result<UndoFileChangesResult, OpenWorkCoreError> {
        if change_ids.is_empty() {
            return Err(StorageError::InvalidInput(
                "at least one file change id is required".to_string(),
            )
            .into());
        }
        let unique = change_ids.iter().collect::<HashSet<_>>();
        if unique.len() != change_ids.len() {
            return Err(
                StorageError::InvalidInput("file change ids must be unique".to_string()).into(),
            );
        }

        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned()
            && matches!(
                handle.snapshot().await?.runtime,
                crate::session::SessionRuntimeSnapshot::Running { .. }
            )
        {
            return Err(OpenWorkCoreError::SessionActive(session_id.to_string()));
        }

        let session = self
            .storage
            .load_session(session_id)
            .await?
            .ok_or_else(|| OpenWorkCoreError::SessionNotFound(session_id.to_string()))?;
        let mut records = self.storage.load_message_records(session_id).await?;
        let changes = select_file_changes(&records, &change_ids)?;
        let context = ToolSessionContext::local(
            PathBuf::from(&session.working_directory),
            PermissionProfile::workspace_write(PathBuf::from(&session.working_directory)),
        );
        let result = undo_workspace_file_changes(&context, &changes).await?;
        let updates = mark_file_changes_undone(&mut records, &change_ids)?;
        self.storage
            .replace_message_contents(session_id, &updates)
            .await?;
        Ok(result)
    }

    pub async fn reapply_file_changes(
        &self,
        session_id: &SessionId,
        change_ids: Vec<String>,
    ) -> Result<ReapplyFileChangesResult, OpenWorkCoreError> {
        if change_ids.is_empty() {
            return Err(StorageError::InvalidInput(
                "at least one file change id is required".to_string(),
            )
            .into());
        }
        let unique = change_ids.iter().collect::<HashSet<_>>();
        if unique.len() != change_ids.len() {
            return Err(
                StorageError::InvalidInput("file change ids must be unique".to_string()).into(),
            );
        }

        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned()
            && matches!(
                handle.snapshot().await?.runtime,
                crate::session::SessionRuntimeSnapshot::Running { .. }
            )
        {
            return Err(OpenWorkCoreError::SessionActive(session_id.to_string()));
        }

        let session = self
            .storage
            .load_session(session_id)
            .await?
            .ok_or_else(|| OpenWorkCoreError::SessionNotFound(session_id.to_string()))?;
        let mut records = self.storage.load_message_records(session_id).await?;
        let changes = select_undone_file_changes(&records, &change_ids)?;
        let context = ToolSessionContext::local(
            PathBuf::from(&session.working_directory),
            PermissionProfile::workspace_write(PathBuf::from(&session.working_directory)),
        );
        let result = reapply_workspace_file_changes(&context, &changes).await?;
        let updates = mark_file_changes_reapplied(&mut records, &change_ids)?;
        self.storage
            .replace_message_contents(session_id, &updates)
            .await?;
        Ok(result)
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

    pub async fn get_trace(&self, turn_id: &TurnId) -> Result<TurnTrace, OpenWorkCoreError> {
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

    async fn workspace_operation_guard(&self, session_id: &SessionId) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.workspace_operations.lock().await;
            if let Some(lock) = locks.get(session_id).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(session_id.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
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
        let conversation = self.storage.load_messages(session_id).await?;
        let chat = ChatStateHandle::spawn(conversation)
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let working_directory = PathBuf::from(&loaded.session.working_directory);
        let (agent, tools) = build_default_agent_and_tools(&working_directory)?;

        Ok(SessionHandle::spawn_with_global_updates(
            SessionRuntimeConfig {
                session_id: session_id.clone(),
                working_directory,
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

fn build_default_agent_and_tools(
    working_directory: &Path,
) -> Result<(Agent, FinalizedToolset), OpenWorkCoreError> {
    let agent = AgentBuilder::new(AgentDefinition::default())
        .build()
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
    let tools = builtin_registry()
        .finalize(
            agent.toolset_config(),
            ToolSessionContext::local(
                working_directory.to_path_buf(),
                PermissionProfile::workspace_write(working_directory.to_path_buf()),
            ),
        )
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
    Ok((agent, tools))
}

type MessageContentUpdate = (String, Vec<ContentBlock>);

fn select_file_changes(
    records: &[StoredMessageRecord],
    change_ids: &[String],
) -> Result<Vec<FileChangeArtifact>, OpenWorkCoreError> {
    select_file_changes_in_state(records, change_ids, false)
}

fn select_undone_file_changes(
    records: &[StoredMessageRecord],
    change_ids: &[String],
) -> Result<Vec<FileChangeArtifact>, OpenWorkCoreError> {
    select_file_changes_in_state(records, change_ids, true)
}

fn select_file_changes_in_state(
    records: &[StoredMessageRecord],
    change_ids: &[String],
    expected_undone: bool,
) -> Result<Vec<FileChangeArtifact>, OpenWorkCoreError> {
    let requested = change_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut found = HashSet::new();
    let mut changes = Vec::with_capacity(change_ids.len());

    for record in records {
        for block in &record.content {
            let ContentBlock::ToolResult(result) = block else {
                continue;
            };
            for artifact in &result.artifacts {
                if artifact.kind != "file_change" {
                    continue;
                }
                let change =
                    FileChangeArtifact::from_result_artifact(artifact).map_err(|error| {
                        OpenWorkCoreError::RuntimeComponent(format!(
                            "stored file change artifact is invalid: {error}"
                        ))
                    })?;
                if !requested.contains(change.change_id.as_str()) {
                    continue;
                }
                if !found.insert(change.change_id.clone()) {
                    return Err(OpenWorkCoreError::RuntimeComponent(format!(
                        "duplicate stored file change id: {}",
                        change.change_id
                    )));
                }
                if !expected_undone && change.undone {
                    return Err(OpenWorkCoreError::FileChangeAlreadyUndone(change.change_id));
                }
                if expected_undone && !change.undone {
                    return Err(OpenWorkCoreError::FileChangeNotUndone(change.change_id));
                }
                changes.push(change);
            }
        }
    }

    for change_id in change_ids {
        if !found.contains(change_id) {
            return Err(OpenWorkCoreError::FileChangeNotFound(change_id.clone()));
        }
    }
    Ok(changes)
}

fn mark_file_changes_undone(
    records: &mut [StoredMessageRecord],
    change_ids: &[String],
) -> Result<Vec<MessageContentUpdate>, OpenWorkCoreError> {
    mark_file_changes_state(records, change_ids, true)
}

fn mark_file_changes_reapplied(
    records: &mut [StoredMessageRecord],
    change_ids: &[String],
) -> Result<Vec<MessageContentUpdate>, OpenWorkCoreError> {
    mark_file_changes_state(records, change_ids, false)
}

fn mark_file_changes_state(
    records: &mut [StoredMessageRecord],
    change_ids: &[String],
    undone: bool,
) -> Result<Vec<MessageContentUpdate>, OpenWorkCoreError> {
    let requested = change_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut updates = Vec::new();

    for record in records {
        let mut changed = false;
        for block in &mut record.content {
            let ContentBlock::ToolResult(result) = block else {
                continue;
            };
            for artifact in &mut result.artifacts {
                if artifact.kind != "file_change" {
                    continue;
                }
                let mut change =
                    FileChangeArtifact::from_result_artifact(artifact).map_err(|error| {
                        OpenWorkCoreError::RuntimeComponent(format!(
                            "stored file change artifact is invalid: {error}"
                        ))
                    })?;
                if requested.contains(change.change_id.as_str()) {
                    change.undone = undone;
                    *artifact = change.to_result_artifact().map_err(|error| {
                        OpenWorkCoreError::RuntimeComponent(format!(
                            "failed to encode updated file change artifact: {error}"
                        ))
                    })?;
                    changed = true;
                }
            }
        }
        if changed {
            updates.push((record.id.clone(), record.content.clone()));
        }
    }
    Ok(updates)
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

#[cfg(test)]
mod tests {
    use openwork_models::model::{ContentBlock, Role, ToolResultBlock, ToolResultState};
    use openwork_tools::{FileChangeArtifact, FileChangeKind, FileDiffHunk};

    use super::*;

    fn change(change_id: &str) -> FileChangeArtifact {
        FileChangeArtifact {
            change_id: change_id.to_string(),
            path: "README.md".to_string(),
            kind: FileChangeKind::Modified,
            additions: 1,
            deletions: 1,
            hunks: Vec::<FileDiffHunk>::new(),
            before_hash: Some("before".to_string()),
            after_hash: "after".to_string(),
            before_content: Some("before".to_string()),
            after_content: Some("after".to_string()),
            undone: false,
        }
    }

    fn record(changes: &[FileChangeArtifact]) -> StoredMessageRecord {
        StoredMessageRecord {
            id: "message-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            sequence: 3,
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: "provider-call-1".to_string(),
                name: "write".to_string(),
                output: vec![ContentBlock::text("edited")],
                state: ToolResultState::Success,
                artifacts: changes
                    .iter()
                    .map(FileChangeArtifact::to_result_artifact)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("encode changes"),
            })],
            created_at: "2026-07-19T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn selects_requested_file_changes_and_marks_only_them_undone() {
        let mut records = vec![record(&[change("change-1"), change("change-2")])];
        let selected =
            select_file_changes(&records, &["change-1".to_string()]).expect("select change");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].change_id, "change-1");

        let updates =
            mark_file_changes_undone(&mut records, &["change-1".to_string()]).expect("mark undone");
        assert_eq!(updates.len(), 1);

        let ContentBlock::ToolResult(result) = &records[0].content[0] else {
            panic!("tool result");
        };
        let states = result
            .artifacts
            .iter()
            .map(|artifact| {
                FileChangeArtifact::from_result_artifact(artifact).expect("decode change")
            })
            .map(|change| (change.change_id, change.undone))
            .collect::<HashMap<_, _>>();
        assert!(states["change-1"]);
        assert!(!states["change-2"]);
    }

    #[test]
    fn rejects_an_unknown_file_change_id() {
        let records = vec![record(&[change("change-1")])];

        let error =
            select_file_changes(&records, &["missing".to_string()]).expect_err("missing change");

        assert!(matches!(error, OpenWorkCoreError::FileChangeNotFound(id) if id == "missing"));
    }

    #[test]
    fn selects_undone_changes_and_marks_them_reapplied() {
        let mut undone = change("change-1");
        undone.undone = true;
        let mut records = vec![record(&[undone, change("change-2")])];

        let selected = select_undone_file_changes(&records, &["change-1".to_string()])
            .expect("select undone change");
        assert_eq!(selected.len(), 1);
        assert!(selected[0].undone);

        let updates = mark_file_changes_reapplied(&mut records, &["change-1".to_string()])
            .expect("mark reapplied");
        assert_eq!(updates.len(), 1);

        let ContentBlock::ToolResult(result) = &records[0].content[0] else {
            panic!("tool result");
        };
        let states = result
            .artifacts
            .iter()
            .map(|artifact| {
                FileChangeArtifact::from_result_artifact(artifact).expect("decode change")
            })
            .map(|change| (change.change_id, change.undone))
            .collect::<HashMap<_, _>>();
        assert!(!states["change-1"]);
        assert!(!states["change-2"]);
    }

    #[test]
    fn rejects_a_file_change_that_was_not_undone_for_reapply() {
        let records = vec![record(&[change("change-1")])];

        let error = select_undone_file_changes(&records, &["change-1".to_string()])
            .expect_err("applied change cannot be reapplied");

        assert!(matches!(error, OpenWorkCoreError::FileChangeNotUndone(id) if id == "change-1"));
    }
}
