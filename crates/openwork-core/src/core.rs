use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use openwork_agent::{Agent, AgentBuilder, AgentDefinition, explorer_definition};
use openwork_chat_state::{ChatStateHandle, ConversationContextView, ConversationItem};
use openwork_models::ProviderFactory;
use openwork_models::model::{ContentBlock, Message, Role};
use openwork_models::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderProfile,
    ProviderRepository, ProviderRepositoryError, ProviderRuntimeConfig,
};
use openwork_tools::{
    FileChangeArtifact, FileChangeReapplyError, FileChangeUndoError, FinalizedToolset,
    PermissionMode, PermissionProfile, ReapplyFileChangesResult, ToolSessionContext,
    UndoFileChangesResult, builtin_registry,
    reapply_file_changes as reapply_workspace_file_changes,
    undo_file_changes as undo_workspace_file_changes,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock, broadcast};

use crate::context::{
    CONTEXT_WINDOW_INSPECTION_SCHEMA_VERSION, ContextEngine, ContextInspectionBudget,
    ContextInspectionMessage, ContextInspectionSystemPart, ContextWindowInspection,
    ModelContextLimits, PlannedSpill, PrepareContextInput, ProjectedMessageOrigin,
    SystemContextBuilder, list_skills, plan_user_input_admission,
};
use crate::plan::{TurnPlan, TurnPlanRecord};
use crate::provider::{BUILTIN_PRESETS, ProviderIndex, ProviderPreset, ProviderTestResult};
use crate::session::{
    AgentMessageKind, COMPACTION_TRANSCRIPT_TOOL_NAME, ClientRequestId, CompactionError,
    CompactionStateCollector, ControlToolSurface, ConversationCompaction,
    ConversationTranscriptTool, ParentLink, PermissionDecision, PreparedTurnInput, ResolvedModel,
    SessionApproval, SessionError, SessionHandle, SessionId, SessionRuntimeConfig, SessionSnapshot,
    SessionStorage, SessionUpdateEnvelope, ToolCallId, TraceContentConfig, TracePayloadSlot,
    TurnAccepted, TurnId, TurnToolset,
};
use crate::skills::{SkillRoots, resolve_selected_skills};
use crate::storage::{
    ApiKeyCipherError, ModelInput, ModelRecord, PostgresProviderRepository, PostgresStorage,
    PostgresTraceRecorder, SessionInput, SessionRecord, StorageError, StoredMessageRecord,
    SubAgentSessionInput, TraceSpanPayloadRecord, TraceSpanRecord, TraceTurnSummary, TurnTrace,
};
use crate::{AgentControl, SubAgentHost, SubAgentSpec, TurnSlot};

const CORE_UPDATE_BROADCAST_CAPACITY: usize = 4096;

async fn write_user_input_spills(
    working_directory: &Path,
    spills: &[PlannedSpill],
) -> Result<(), OpenWorkCoreError> {
    let workspace = tokio::fs::canonicalize(working_directory)
        .await
        .map_err(|error| {
            OpenWorkCoreError::RuntimeComponent(format!(
                "cannot resolve working directory {}: {error}",
                working_directory.display()
            ))
        })?;
    if !workspace.is_dir() {
        return Err(OpenWorkCoreError::RuntimeComponent(format!(
            "working directory is not a directory: {}",
            workspace.display()
        )));
    }

    for spill in spills {
        let relative_path = Path::new(&spill.relative_path);
        if relative_path.as_os_str().is_empty()
            || relative_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(OpenWorkCoreError::RuntimeComponent(format!(
                "spill path must be a normal relative path: {}",
                spill.relative_path
            )));
        }

        let target = workspace.join(relative_path);
        let parent = target.parent().ok_or_else(|| {
            OpenWorkCoreError::RuntimeComponent(format!(
                "spill path has no parent: {}",
                spill.relative_path
            ))
        })?;
        let resolved_ancestor = canonical_existing_ancestor(parent).await.map_err(|error| {
            OpenWorkCoreError::RuntimeComponent(format!(
                "cannot resolve spill path {}: {error}",
                spill.relative_path
            ))
        })?;
        if !resolved_ancestor.starts_with(&workspace) {
            return Err(OpenWorkCoreError::RuntimeComponent(format!(
                "spill path escapes working directory: {}",
                spill.relative_path
            )));
        }

        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            OpenWorkCoreError::RuntimeComponent(format!(
                "cannot create parent directory for {}: {error}",
                spill.relative_path
            ))
        })?;
        let resolved_parent = tokio::fs::canonicalize(parent).await.map_err(|error| {
            OpenWorkCoreError::RuntimeComponent(format!(
                "cannot resolve parent directory for {}: {error}",
                spill.relative_path
            ))
        })?;
        if !resolved_parent.starts_with(&workspace) {
            return Err(OpenWorkCoreError::RuntimeComponent(format!(
                "spill path escapes working directory: {}",
                spill.relative_path
            )));
        }

        match tokio::fs::symlink_metadata(&target).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(OpenWorkCoreError::RuntimeComponent(format!(
                    "spill target must not be a symbolic link: {}",
                    spill.relative_path
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(OpenWorkCoreError::RuntimeComponent(format!(
                    "cannot inspect spill target {}: {error}",
                    spill.relative_path
                )));
            }
        }
        tokio::fs::write(&target, spill.text.as_bytes())
            .await
            .map_err(|error| {
                OpenWorkCoreError::RuntimeComponent(format!(
                    "cannot write {}: {error}",
                    spill.relative_path
                ))
            })?;
    }

    Ok(())
}

async fn canonical_existing_ancestor(path: &Path) -> std::io::Result<PathBuf> {
    let mut candidate = path;
    loop {
        match tokio::fs::canonicalize(candidate).await {
            Ok(resolved) => return Ok(resolved),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                candidate = candidate.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OpenWorkCoreConfig {
    pub database_url: Option<String>,
    pub trace_content: TraceContentConfig,
    pub agents_skills_root: Option<PathBuf>,
}

impl OpenWorkCoreConfig {
    pub fn from_env_or_local() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            database_url: std::env::var("DATABASE_URL").ok(),
            trace_content: TraceContentConfig::default(),
            agents_skills_root: home.map(|home| home.join(".agents/skills")),
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
    /// 每个 Turn 的最终计划。
    ///
    /// 从 `turn_plans` 读，不重新解析历史里的 Tool Call JSON：那些调用记录的是"模型
    /// 提交过什么"，而这里要的是"最后生效的是什么"，中途失败的调用不该出现在界面上。
    pub plans: Vec<TurnPlanRecord>,
}

#[derive(Debug, Error)]
pub enum OpenWorkCoreError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Compaction(#[from] CompactionError),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("session has no default model: {0}")]
    DefaultModelMissing(String),
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("model capabilities are missing for {0}; open Settings > Models and edit its provider")]
    ModelCapabilitiesMissing(String),
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
    #[error("selected skill is unavailable: {0}")]
    SkillUnavailable(String),
    #[error(transparent)]
    SkillRead(#[from] crate::skills::SkillReadError),
    #[error("skill filesystem task failed")]
    SkillFilesystemTask(#[source] tokio::task::JoinError),
    #[error(transparent)]
    Provider(#[from] ProviderRepositoryError),
    #[error("provider credential bootstrap failed: {0}")]
    CredentialBootstrap(#[from] ApiKeyCipherError),
    #[error("provider repository is unavailable in this core configuration")]
    ProviderRepositoryUnavailable,
}

pub struct OpenWorkCore {
    self_weak: Weak<OpenWorkCore>,
    storage: Arc<PostgresStorage>,
    provider_factory: ProviderFactory,
    trace: Arc<PostgresTraceRecorder>,
    credentials: Arc<dyn CredentialResolver>,
    providers: Option<Arc<dyn ProviderRepository>>,
    skill_roots: SkillRoots,
    skill_permission_roots: Vec<PathBuf>,
    disabled_skill_names: RwLock<BTreeSet<String>>,
    skill_status_update: Mutex<()>,
    update_tx: broadcast::Sender<SessionUpdateEnvelope>,
    sessions: RwLock<HashMap<SessionId, SessionHandle>>,
    agent_controls: RwLock<HashMap<SessionId, AgentControl>>,
    session_creation: Mutex<()>,
    workspace_operations: Mutex<HashMap<SessionId, Weak<Mutex<()>>>>,
}

impl OpenWorkCore {
    pub async fn bootstrap(config: OpenWorkCoreConfig) -> Result<Arc<Self>, OpenWorkCoreError> {
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
        Self::from_storage_parts(
            storage,
            credentials,
            Some(providers),
            config.trace_content,
            SkillRoots {
                agents: config.agents_skills_root,
            },
        )
        .await
    }

    pub async fn from_storage(
        storage: Arc<PostgresStorage>,
    ) -> Result<Arc<Self>, OpenWorkCoreError> {
        Self::from_storage_parts(
            storage,
            Arc::new(EnvironmentCredentialResolver),
            None,
            TraceContentConfig::default(),
            SkillRoots::default(),
        )
        .await
    }

    pub async fn from_storage_with_credentials(
        storage: Arc<PostgresStorage>,
        credentials: Arc<dyn CredentialResolver>,
    ) -> Result<Arc<Self>, OpenWorkCoreError> {
        Self::from_storage_parts(
            storage,
            credentials,
            None,
            TraceContentConfig::default(),
            SkillRoots::default(),
        )
        .await
    }

    async fn from_storage_parts(
        storage: Arc<PostgresStorage>,
        credentials: Arc<dyn CredentialResolver>,
        providers: Option<Arc<dyn ProviderRepository>>,
        trace_content: TraceContentConfig,
        skill_roots: SkillRoots,
    ) -> Result<Arc<Self>, OpenWorkCoreError> {
        storage.migrate().await?;
        let disabled_skill_names = RwLock::new(storage.disabled_skill_names().await?);
        let permission_root_source = skill_roots.clone();
        let skill_permission_roots = spawn_skill_filesystem_task(move || {
            materialize_skill_permission_roots(&permission_root_source)
        })
        .await?;
        storage.mark_running_interrupted().await?;
        storage
            .purge_expired_trace_payloads(trace_content.retention_days())
            .await?;
        let trace = Arc::new(PostgresTraceRecorder::spawn_with_content_config(
            storage.pool().clone(),
            trace_content,
        ));
        let (update_tx, _) = broadcast::channel(CORE_UPDATE_BROADCAST_CAPACITY);
        Ok(Arc::new_cyclic(|self_weak| Self {
            self_weak: self_weak.clone(),
            storage,
            provider_factory: ProviderFactory::default(),
            trace,
            credentials,
            providers,
            skill_roots,
            skill_permission_roots,
            disabled_skill_names,
            skill_status_update: Mutex::new(()),
            update_tx,
            sessions: RwLock::new(HashMap::new()),
            agent_controls: RwLock::new(HashMap::new()),
            session_creation: Mutex::new(()),
            workspace_operations: Mutex::new(HashMap::new()),
        }))
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
        if let Some(model_id) = input.default_model_id.as_deref() {
            let model = self
                .storage
                .load_model(model_id)
                .await?
                .ok_or_else(|| OpenWorkCoreError::ModelNotFound(model_id.to_string()))?;
            require_model_capabilities(&model)?;
        }
        Ok(self.storage.create_session(input).await?)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, OpenWorkCoreError> {
        Ok(self.storage.list_sessions().await?)
    }

    pub async fn list_sub_agent_sessions(
        &self,
        parent_session_id: &SessionId,
    ) -> Result<Vec<SessionRecord>, OpenWorkCoreError> {
        Ok(self
            .storage
            .list_sub_agent_sessions(parent_session_id)
            .await?)
    }

    pub async fn list_skills(&self) -> Result<crate::skills::SkillDiscovery, OpenWorkCoreError> {
        let skill_roots = self.skill_roots.clone();
        let disabled_names = self.disabled_skill_names.read().await.clone();
        spawn_skill_filesystem_task(move || list_skills(&skill_roots, &disabled_names)).await
    }

    pub async fn set_skill_disabled(
        &self,
        name: &str,
        disabled: bool,
    ) -> Result<crate::skills::SkillDiscovery, OpenWorkCoreError> {
        let _update = self.skill_status_update.lock().await;
        self.storage.set_skill_disabled(name, disabled).await?;
        {
            let mut names = self.disabled_skill_names.write().await;
            if disabled {
                names.insert(name.to_string());
            } else {
                names.remove(name);
            }
        }
        self.list_skills().await
    }

    pub async fn read_skill(
        &self,
        path: &str,
    ) -> Result<crate::skills::SkillDetail, OpenWorkCoreError> {
        let skill_roots = self.skill_roots.clone();
        let path = path.to_string();
        spawn_skill_filesystem_task(move || crate::skills::read_skill(&skill_roots, &path))
            .await?
            .map_err(OpenWorkCoreError::from)
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
        let plans = self
            .storage
            .load_session_turn_plans(session_id)
            .await
            .map_err(OpenWorkCoreError::RuntimeComponent)?
            .iter()
            .map(TurnPlan::to_record)
            .collect();
        Ok(LoadedSession {
            session,
            messages,
            plans,
        })
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
        let capabilities = require_model_capabilities(&model)?;
        let working_directory = PathBuf::from(&loaded.session.working_directory);
        let (agent, tools, control_surface) = if loaded.session.is_sub_agent() {
            let (agent, tools) =
                build_explorer_agent_and_tools(&working_directory, &self.skill_permission_roots)?;
            (agent, tools, ControlToolSurface::SubAgent)
        } else {
            let control = self.agent_control_for_root(session_id).await?;
            let (agent, tools) = build_default_agent_and_tools(
                session_id,
                &working_directory,
                &self.skill_permission_roots,
                self.storage.clone(),
            )?;
            (
                agent,
                tools,
                ControlToolSurface::Root {
                    max_active_sub_agent_turns: control.max_active_turns(),
                },
            )
        };
        let tools = TurnToolset::new(Arc::new(tools), control_surface)
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let system_prompt = tools.system_prompt(agent.system_prompt());
        let system_context =
            SystemContextBuilder::new(&working_directory, self.skill_roots.clone())
                .with_disabled_skills(self.disabled_skill_names.read().await.clone())
                .build(&system_prompt)
                .await
                .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;

        let conversation_records = self.storage.load_conversation_records(session_id).await?;
        let current_turn_id = conversation_records
            .iter()
            .rev()
            .find_map(|message| message.turn_id.clone());
        let mut conversation_items = Vec::with_capacity(conversation_records.len());
        for message in &conversation_records {
            if message.role == Role::System {
                return Err(OpenWorkCoreError::RuntimeComponent(
                    "persisted system messages are not valid Conversation input".to_string(),
                ));
            }
            conversation_items.push(ConversationItem::persisted_with_kind(
                message.id.clone(),
                message.sequence,
                message.message_kind,
                Message {
                    role: message.role,
                    content: message.content.clone(),
                },
            ));
        }

        let turn_ids_by_message_id = conversation_records
            .iter()
            .map(|record| (record.id.as_str(), record.turn_id.clone()))
            .collect::<HashMap<_, _>>();
        let context_engine =
            ContextEngine::new(ModelContextLimits::from_capabilities(capabilities));
        let prepared = context_engine
            .prepare(PrepareContextInput::new(
                &model.model_name,
                &system_context,
                ConversationContextView {
                    items: conversation_items,
                },
                tools.definitions(),
            ))
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let inspected_messages = prepared
            .conversation_messages()
            .iter()
            .zip(prepared.conversation_provenance())
            .enumerate()
            .map(|(index, (message, provenance))| {
                let (message_id, turn_id) = match provenance {
                    ProjectedMessageOrigin::Persisted { message_id } => (
                        message_id.clone(),
                        turn_ids_by_message_id
                            .get(message_id.as_str())
                            .cloned()
                            .flatten(),
                    ),
                    ProjectedMessageOrigin::Synthesized => {
                        (format!("context-synthesized-{index}"), None)
                    }
                };
                ContextInspectionMessage {
                    message_id,
                    turn_id,
                    role: message.role,
                    content: message.content.clone(),
                }
            })
            .collect();
        let budget = prepared.context_budget;
        let tool_surface = prepared.request.tools;

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
            tool_surface,
            budget: ContextInspectionBudget {
                context_window_tokens: capabilities.context_window_tokens,
                system_context_tokens: budget.system_context_tokens,
                conversation_tokens: budget.conversation_tokens,
                tool_surface_tokens: budget.tool_surface_tokens,
                estimated_input_tokens: budget.estimated_input_tokens,
                reserved_output_tokens: budget.reserved_output_tokens,
                auto_compaction_threshold_percent: crate::context::AUTO_COMPACT_THRESHOLD_PERCENT,
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
        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned()
            && matches!(
                handle.snapshot().await?.runtime,
                crate::session::SessionRuntimeSnapshot::Running { .. }
            )
        {
            return Err(OpenWorkCoreError::SessionActive(session_id.to_string()));
        }
        let children = self.storage.list_sub_agent_sessions(session_id).await?;
        self.storage.delete_session(session_id).await?;
        self.unload_runtime_session_tree(session_id, &children)
            .await;
        Ok(())
    }

    pub async fn start_turn(
        &self,
        session_id: &SessionId,
        client_request_id: ClientRequestId,
        input: Vec<crate::UserInput>,
    ) -> Result<TurnAccepted, OpenWorkCoreError> {
        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        let handle = self.session_handle(session_id).await?;
        if let Some(accepted) = handle.accepted_turn(client_request_id.clone()).await? {
            return Ok(accepted);
        }
        if let crate::session::SessionRuntimeSnapshot::Running { turn_id, .. } =
            handle.snapshot().await?.runtime
        {
            return Err(SessionError::Busy(turn_id).into());
        }
        let user_content = input
            .iter()
            .filter_map(|item| match item {
                crate::UserInput::Text { text } => Some(ContentBlock::text(text)),
                crate::UserInput::Skill { .. } => None,
            })
            .collect::<Vec<_>>();
        if user_content.is_empty() {
            return Err(SessionError::EmptyInput.into());
        }
        let limits = ModelContextLimits::from_capabilities(handle.model_capabilities());
        let admission =
            plan_user_input_admission(&user_content, &limits, client_request_id.as_str());
        if !admission.spills.is_empty() {
            let session = self
                .storage
                .load_session(session_id)
                .await?
                .ok_or_else(|| OpenWorkCoreError::SessionNotFound(session_id.to_string()))?;
            write_user_input_spills(Path::new(&session.working_directory), &admission.spills)
                .await?;
        }
        let user_content = admission.content;

        let disabled_skill_names = self.disabled_skill_names.read().await.clone();
        let has_skill_input = input
            .iter()
            .any(|item| matches!(item, crate::UserInput::Skill { .. }));
        let skills = if has_skill_input {
            let selection_disabled_skill_names = disabled_skill_names.clone();
            let skill_roots = self.skill_roots.clone();
            spawn_skill_filesystem_task(move || {
                resolve_selected_skills(&skill_roots, &selection_disabled_skill_names, &input)
            })
            .await?
            .map_err(|error| OpenWorkCoreError::SkillUnavailable(error.name().to_string()))?
        } else {
            Vec::new()
        };
        let prepared_input = PreparedTurnInput::new(
            skills
                .into_iter()
                .map(|skill| skill.into_message())
                .collect(),
            user_content,
        );

        // The parent workspace guard is held and no parent Turn is active at
        // this point, so no legitimate spawn can be in progress. A zero-Turn
        // child is therefore an abandoned spawn and is safe to delete. Revisit
        // this invariant if parent operations become concurrent in the future.
        self.reconcile_sub_agent_sessions(session_id, &handle)
            .await?;

        let accepted = handle
            .start_turn(client_request_id, prepared_input, disabled_skill_names)
            .await?;
        Ok(accepted)
    }

    async fn reconcile_sub_agent_sessions(
        &self,
        parent_session_id: &SessionId,
        parent_handle: &SessionHandle,
    ) -> Result<(), OpenWorkCoreError> {
        let reconciliation = self
            .storage
            .reconcile_sub_agent_sessions(parent_session_id)
            .await?;

        let mut orphan_session_ids = Vec::with_capacity(reconciliation.deleted_orphans.len());
        let agent_control = self
            .agent_controls
            .read()
            .await
            .get(parent_session_id)
            .cloned();
        for orphan in reconciliation.deleted_orphans {
            tracing::warn!(
                parent_session_id = %parent_session_id,
                child_session_id = %orphan.session_id,
                task_name = %orphan.task_name,
                "removed zero-turn sub-agent session during parent turn reconciliation"
            );
            let orphan_session_id = SessionId::new(orphan.session_id);
            if let Some(control) = &agent_control {
                control.forget_agent(&orphan.task_name, &orphan_session_id);
            }
            orphan_session_ids.push(orphan_session_id);
        }
        self.shutdown_runtime_sessions(&orphan_session_ids).await;

        for result in reconciliation.undelivered {
            let (kind, body) = match result.status.as_str() {
                "completed" => (
                    AgentMessageKind::FinalAnswer,
                    result.final_text.unwrap_or_else(|| {
                        "Sub-agent completed without a persisted final response.".to_string()
                    }),
                ),
                "interrupted" => (
                    AgentMessageKind::Interrupted,
                    "Sub-agent was interrupted by a process restart and is no longer available. Spawn a new explorer if this result is still needed."
                        .to_string(),
                ),
                "cancelled" => (
                    AgentMessageKind::Failed,
                    "cancelled: Sub-agent turn was cancelled.".to_string(),
                ),
                _ => {
                    let code = result.error_code.as_deref().unwrap_or(&result.status);
                    let message = result.error_message.as_deref().unwrap_or(
                        "Sub-agent terminated without a persisted error message.",
                    );
                    (AgentMessageKind::Failed, format!("{code}: {message}"))
                }
            };
            parent_handle
                .deliver_agent_message(
                    result.child_session_id,
                    result.child_turn_id,
                    result.task_name,
                    kind,
                    body,
                )
                .await?;
        }
        Ok(())
    }

    pub async fn compact_conversation(
        &self,
        session_id: &SessionId,
    ) -> Result<ConversationCompaction, OpenWorkCoreError> {
        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        let handle = self.session_handle(session_id).await?;
        Ok(handle
            .compact_conversation(self.disabled_skill_names.read().await.clone())
            .await?)
    }

    pub async fn list_conversation_compactions(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ConversationCompaction>, OpenWorkCoreError> {
        Ok(self
            .storage
            .list_conversation_compactions(session_id)
            .await?)
    }

    pub async fn replay_conversation(
        &self,
        session_id: &SessionId,
        selector: crate::session::ConversationProjectionSelector,
    ) -> Result<crate::storage::ConversationProjectionRecord, OpenWorkCoreError> {
        Ok(self
            .storage
            .replay_conversation(session_id, selector)
            .await?)
    }

    pub async fn read_compaction_transcript(
        &self,
        session_id: &SessionId,
        query: crate::storage::ConversationTranscriptQuery,
    ) -> Result<crate::storage::ConversationTranscriptPage, OpenWorkCoreError> {
        Ok(self
            .storage
            .read_compaction_transcript(session_id, query)
            .await?)
    }

    pub async fn rewind_conversation(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<ConversationCompaction, OpenWorkCoreError> {
        let _workspace_operation = self.workspace_operation_guard(session_id).await;
        let handle = self.session_handle(session_id).await?;
        Ok(handle
            .rewind_conversation(compaction_id.to_string())
            .await?)
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
        let working_directory = PathBuf::from(&session.working_directory);
        let context = ToolSessionContext::local(
            working_directory.clone(),
            skill_permission_profile(&working_directory, &self.skill_permission_roots),
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
        let working_directory = PathBuf::from(&session.working_directory);
        let context = ToolSessionContext::local(
            working_directory.clone(),
            skill_permission_profile(&working_directory, &self.skill_permission_roots),
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

    pub async fn set_permission_mode(
        &self,
        session_id: &SessionId,
        mode: PermissionMode,
    ) -> Result<PermissionMode, OpenWorkCoreError> {
        let handle = self.session_handle(session_id).await?;
        Ok(handle.set_permission_mode(mode).await?)
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

    pub async fn get_trace_by_id(&self, trace_id: &str) -> Result<TurnTrace, OpenWorkCoreError> {
        Ok(self.storage.get_trace_by_id(trace_id).await?)
    }

    pub async fn get_span_payload(
        &self,
        span_id: &str,
        slot: TracePayloadSlot,
    ) -> Result<Option<TraceSpanPayloadRecord>, OpenWorkCoreError> {
        Ok(self.storage.get_span_payload(span_id, slot).await?)
    }

    /// Compaction Spans for a Session, newest first. Manual compactions have no
    /// Turn and are only reachable here.
    pub async fn list_compaction_spans(
        &self,
        session_id: &SessionId,
        limit: i64,
    ) -> Result<Vec<TraceSpanRecord>, OpenWorkCoreError> {
        Ok(self
            .storage
            .list_compaction_spans(session_id, limit)
            .await?)
    }

    async fn session_handle(
        &self,
        session_id: &SessionId,
    ) -> Result<SessionHandle, OpenWorkCoreError> {
        let existing = self.sessions.read().await.get(session_id).cloned();
        if let Some(handle) = existing {
            if !handle.requires_reload() {
                return Ok(handle);
            }
            if matches!(
                handle.snapshot().await,
                Ok(SessionSnapshot {
                    runtime: crate::session::SessionRuntimeSnapshot::Running { .. },
                    ..
                })
            ) {
                return Ok(handle);
            }
        }

        let _creation = self.session_creation.lock().await;
        let existing = self.sessions.read().await.get(session_id).cloned();
        let mut permission_mode = PermissionMode::Default;
        if let Some(handle) = existing {
            if !handle.requires_reload() {
                return Ok(handle);
            }
            if let Ok(snapshot) = handle.snapshot().await {
                if matches!(
                    snapshot,
                    SessionSnapshot {
                        runtime: crate::session::SessionRuntimeSnapshot::Running { .. },
                        ..
                    }
                ) {
                    return Ok(handle);
                }
                permission_mode = snapshot.permission_mode;
            }
            let children = self.storage.list_sub_agent_sessions(session_id).await?;
            self.unload_runtime_session_tree(session_id, &children)
                .await;
        }

        let handle = self
            .build_session_handle(session_id, permission_mode)
            .await?;
        self.sessions
            .write()
            .await
            .insert(session_id.clone(), handle.clone());
        Ok(handle)
    }

    async fn unload_runtime_session_tree(
        &self,
        root_session_id: &SessionId,
        direct_children: &[SessionRecord],
    ) {
        let mut session_ids = direct_children
            .iter()
            .map(|child| SessionId::new(child.id.clone()))
            .collect::<Vec<_>>();
        session_ids.push(root_session_id.clone());

        self.agent_controls.write().await.remove(root_session_id);
        self.shutdown_runtime_sessions(&session_ids).await;
    }

    async fn shutdown_runtime_sessions(&self, session_ids: &[SessionId]) {
        let handles = {
            let mut sessions = self.sessions.write().await;
            session_ids
                .iter()
                .filter_map(|session_id| sessions.remove(session_id))
                .collect::<Vec<_>>()
        };

        for handle in handles {
            let _ = handle.shutdown().await;
        }
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
        permission_mode: PermissionMode,
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
        let capabilities = require_model_capabilities(&model)?;
        let runtime = provider_runtime(&model, capabilities, self.credentials.as_ref()).await?;
        let model_port = Arc::from(self.provider_factory.build(&runtime));
        let conversation = self.storage.load_conversation_items(session_id).await?;
        let chat = ChatStateHandle::spawn_items(conversation)
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let working_directory = PathBuf::from(&loaded.session.working_directory);
        let (agent, tools, control_surface, approval, parent_link, agent_control) = if let (
            Some(parent_session_id),
            Some(task_name),
        ) = (
            loaded.session.parent_session_id.as_deref(),
            loaded.session.task_name.as_deref(),
        ) {
            let parent_session_id = SessionId::new(parent_session_id);
            let agent_control = self.agent_control_for_root(&parent_session_id).await?;
            let (agent, tools) =
                build_explorer_agent_and_tools(&working_directory, &self.skill_permission_roots)?;
            (
                agent,
                tools,
                ControlToolSurface::SubAgent,
                SessionApproval::NonInteractive,
                Some(ParentLink {
                    parent_session_id,
                    task_name: task_name.to_string(),
                    agent_control: agent_control.clone(),
                }),
                agent_control,
            )
        } else {
            let agent_control = self.agent_control_for_root(session_id).await?;
            let (agent, tools) = build_default_agent_and_tools(
                session_id,
                &working_directory,
                &self.skill_permission_roots,
                self.storage.clone(),
            )?;
            (
                agent,
                tools,
                ControlToolSurface::Root {
                    max_active_sub_agent_turns: agent_control.max_active_turns(),
                },
                SessionApproval::Interactive,
                None,
                agent_control,
            )
        };

        Ok(SessionHandle::spawn_with_global_updates(
            SessionRuntimeConfig {
                session_id: session_id.clone(),
                working_directory,
                skill_roots: self.skill_roots.clone(),
                resolved_model: ResolvedModel::new(
                    Some(model.id),
                    model.provider_kind,
                    model.model_name,
                    capabilities,
                ),
                agent,
                chat,
                model: model_port,
                // Default mode 广告 update_plan。重名在这里确定性失败，而不是留到模型
                // 某次调用时才暴露成分派歧义。
                tools: Arc::new(
                    TurnToolset::new(Arc::new(tools), control_surface)
                        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?,
                ),
                storage: self.storage.clone(),
                compaction_state: Arc::new(CompactionStateCollector::default()),
                trace: self.trace.clone(),
                permission_mode,
                // A Session opened from the Desktop always has a user behind it.
                // Sub-agent Sessions are started elsewhere and are the only
                // `NonInteractive` ones.
                approval,
                parent_link,
                agent_control: Some(agent_control),
            },
            self.update_tx.clone(),
        ))
    }

    async fn agent_control_for_root(
        &self,
        session_id: &SessionId,
    ) -> Result<AgentControl, OpenWorkCoreError> {
        if let Some(control) = self.agent_controls.read().await.get(session_id).cloned() {
            return Ok(control);
        }
        let children = self.storage.list_sub_agent_sessions(session_id).await?;
        let host: Weak<dyn SubAgentHost> = self.self_weak.clone();
        let restored = children
            .into_iter()
            .map(|child| {
                let task_name = child.task_name.ok_or_else(|| {
                    OpenWorkCoreError::RuntimeComponent(format!(
                        "sub-agent session {} has no task_name",
                        child.id
                    ))
                })?;
                let agent_role = child.agent_role.ok_or_else(|| {
                    OpenWorkCoreError::RuntimeComponent(format!(
                        "sub-agent session {} has no agent_role",
                        child.id
                    ))
                })?;
                Ok(crate::SubAgent {
                    task_name,
                    session_id: SessionId::new(child.id),
                    agent_role,
                    started_at: child.created_at,
                })
            })
            .collect::<Result<Vec<_>, OpenWorkCoreError>>()?;
        let mut controls = self.agent_controls.write().await;
        if let Some(control) = controls.get(session_id).cloned() {
            return Ok(control);
        }
        let control = AgentControl::new(session_id.clone(), host);
        control
            .restore_agents(restored)
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        controls.insert(session_id.clone(), control.clone());
        Ok(control)
    }
}

#[async_trait]
impl SubAgentHost for OpenWorkCore {
    async fn start_sub_agent(&self, spec: SubAgentSpec) -> Result<(), String> {
        let _creation = self.session_creation.lock().await;
        if self.sessions.read().await.contains_key(&spec.session_id) {
            return Err(format!("session {} is already live", spec.session_id));
        }
        let parent = self
            .storage
            .load_session(&spec.parent_session_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("parent session {} was not found", spec.parent_session_id))?;
        self.storage
            .create_sub_agent_session(&SubAgentSessionInput {
                id: spec.session_id.clone(),
                parent_session_id: spec.parent_session_id,
                task_name: spec.task_name,
                agent_role: spec.agent_role,
                working_directory: parent.working_directory,
                default_model_id: parent.default_model_id,
                spawn_span_id: spec.spawn_span_id,
            })
            .await
            .map_err(|error| error.to_string())?;
        let handle = self
            .build_session_handle(&spec.session_id, PermissionMode::Default)
            .await
            .map_err(|error| error.to_string())?;
        self.sessions.write().await.insert(spec.session_id, handle);
        Ok(())
    }

    async fn start_sub_agent_turn(
        &self,
        session_id: &SessionId,
        message: String,
        turn_slot: TurnSlot,
    ) -> Result<(), String> {
        let handle = OpenWorkCore::session_handle(self, session_id)
            .await
            .map_err(|error| error.to_string())?;
        handle
            .start_sub_agent_turn(
                message,
                self.disabled_skill_names.read().await.clone(),
                turn_slot,
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String> {
        OpenWorkCore::session_handle(self, session_id)
            .await
            .map_err(|error| error.to_string())
    }
}

async fn spawn_skill_filesystem_task<T>(
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, OpenWorkCoreError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(OpenWorkCoreError::SkillFilesystemTask)
}

fn build_default_agent_and_tools(
    session_id: &SessionId,
    working_directory: &Path,
    skill_permission_roots: &[PathBuf],
    storage: Arc<dyn SessionStorage>,
) -> Result<(Agent, FinalizedToolset), OpenWorkCoreError> {
    let mut definition = AgentDefinition::default();
    definition
        .tool_names
        .push(COMPACTION_TRANSCRIPT_TOOL_NAME.to_string());
    let agent = AgentBuilder::new(definition)
        .build()
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
    let tools = builtin_registry()
        .register(ConversationTranscriptTool::new(session_id.clone(), storage))
        .finalize(
            agent.toolset_config(),
            ToolSessionContext::local(
                working_directory.to_path_buf(),
                skill_permission_profile(working_directory, skill_permission_roots),
            ),
        )
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
    Ok((agent, tools))
}

fn build_explorer_agent_and_tools(
    working_directory: &Path,
    skill_permission_roots: &[PathBuf],
) -> Result<(Agent, FinalizedToolset), OpenWorkCoreError> {
    let agent = AgentBuilder::new(explorer_definition())
        .build()
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
    let tools = builtin_registry()
        .finalize(
            agent.toolset_config(),
            ToolSessionContext::local(
                working_directory.to_path_buf(),
                skill_permission_profile(working_directory, skill_permission_roots),
            ),
        )
        .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
    Ok((agent, tools))
}

fn skill_permission_profile(
    working_directory: &Path,
    skill_permission_roots: &[PathBuf],
) -> PermissionProfile {
    PermissionProfile::for_workspace_and_skill_roots(
        working_directory.to_path_buf(),
        skill_permission_roots.iter().cloned(),
    )
}

fn materialize_skill_permission_roots(skill_roots: &SkillRoots) -> Vec<PathBuf> {
    skill_roots
        .agents
        .iter()
        .flat_map(|root| std::iter::once(root.clone()).chain(std::fs::canonicalize(root).ok()))
        .collect()
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
    capabilities: openwork_models::model::ModelCapabilities,
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
                capabilities: Some(capabilities),
            }],
            enabled: model.enabled,
        },
        credential,
        adapter_options,
    })
}

fn require_model_capabilities(
    model: &ModelRecord,
) -> Result<openwork_models::model::ModelCapabilities, OpenWorkCoreError> {
    model
        .capabilities()?
        .ok_or_else(|| OpenWorkCoreError::ModelCapabilitiesMissing(model.id.clone()))
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
    use openwork_chat_state::MessageKind;
    use openwork_models::model::{ContentBlock, Role, ToolResultBlock, ToolResultState};
    use openwork_tools::{
        Authorization, FileChangeArtifact, FileChangeKind, FileDiffHunk, PermissionMode,
        ToolInvocation, ToolsetConfig,
    };
    use serde_json::json;

    use super::*;

    fn test_capabilities() -> openwork_models::model::ModelCapabilities {
        openwork_models::model::ModelCapabilities {
            context_window_tokens: 200_000,
            max_output_tokens: 32_768,
            max_reasoning_tokens: None,
            accepts_data_blocks: true,
        }
    }

    struct FixedTestCredential;

    #[async_trait::async_trait]
    impl CredentialResolver for FixedTestCredential {
        async fn resolve(&self, _reference: &str) -> Result<ApiCredential, String> {
            Ok(ApiCredential::new("test-credential"))
        }
    }

    async fn insert_completed_test_turn(
        storage: &PostgresStorage,
        session_id: &SessionId,
        turn_id: &TurnId,
        client_request_id: &str,
    ) {
        // 并行测试会创建 Core 并中断全库 running Turn；直接构造重启前的终态，
        // 避免夹具在 begin/finish 两步之间被其他测试改成 interrupted。
        sqlx::query(
            "INSERT INTO turns (
                 id, session_id, client_request_id, sequence, model_id,
                 resolved_provider_kind, resolved_model_name, app_version,
                 status, ended_at
             ) VALUES (
                 $1, $2, $3, 1, NULL, 'test', 'test-model', $4,
                 'completed', CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             )",
        )
        .bind(turn_id.as_str())
        .bind(session_id.as_str())
        .bind(client_request_id)
        .bind(env!("CARGO_PKG_VERSION"))
        .execute(storage.pool())
        .await
        .unwrap();
    }

    #[test]
    fn production_explorer_builder_exposes_only_the_readonly_role_surface() {
        let workspace = std::env::temp_dir().join(format!(
            "openwork-explorer-toolset-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&workspace).expect("workspace");

        let (agent, tools) =
            build_explorer_agent_and_tools(&workspace, &[]).expect("explorer toolset");

        assert_eq!(agent.definition().name, "explorer");
        assert_eq!(
            tools
                .definitions()
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["read", "grep", "glob", "list", "bash"]
        );
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[cfg(unix)]
    #[test]
    fn canonical_path_from_a_symlinked_skill_root_is_readable_without_approval() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "openwork-core-symlinked-skill-root-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let workspace = root.join("workspace");
        let actual_root = root.join("actual-skills");
        let configured_root = root.join("configured-skills");
        let skill_path = actual_root.join("review/SKILL.md");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(skill_path.parent().expect("skill directory"))
            .expect("skill directory");
        std::fs::write(&skill_path, "Body\n").expect("skill");
        symlink(&actual_root, &configured_root).expect("skill root symlink");

        let skill_roots = SkillRoots {
            agents: Some(configured_root),
        };
        let permission_roots = materialize_skill_permission_roots(&skill_roots);
        let permissions = skill_permission_profile(&workspace, &permission_roots);
        let tools = builtin_registry()
            .finalize(
                &ToolsetConfig::from_names(["read"]),
                ToolSessionContext::local(workspace, permissions),
            )
            .expect("read toolset");
        let canonical_skill_path = std::fs::canonicalize(&skill_path).expect("canonical skill");
        let authorization = tools.authorize(
            &ToolInvocation::new("read", json!({ "path": canonical_skill_path })),
            PermissionMode::Default,
            &[],
        );

        assert!(matches!(authorization, Authorization::Allow { .. }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn core_skill_status_updates_listing_and_survives_restart() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let root = std::env::temp_dir().join(format!(
            "openwork-core-skill-status-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let agents = root.join(".agents/skills");
        let skill_name = format!("review-{}", uuid::Uuid::new_v4().simple());
        let skill_directory = agents.join(&skill_name);
        std::fs::create_dir_all(&skill_directory).expect("skill directory");
        std::fs::write(
            skill_directory.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: Review changes.\n---\nBody\n"),
        )
        .expect("skill");
        let roots = SkillRoots {
            agents: Some(agents),
        };
        let storage = Arc::new(PostgresStorage::connect(Some(&database_url)).await.unwrap());
        let core = OpenWorkCore::from_storage_parts(
            Arc::clone(&storage),
            Arc::new(EnvironmentCredentialResolver),
            None,
            TraceContentConfig::default(),
            roots.clone(),
        )
        .await
        .unwrap();

        assert!(!core.list_skills().await.unwrap().skills[0].disabled);
        core.set_skill_disabled(&skill_name, true).await.unwrap();
        assert!(core.list_skills().await.unwrap().skills[0].disabled);

        drop(core);
        let restarted = OpenWorkCore::from_storage_parts(
            Arc::new(PostgresStorage::connect(Some(&database_url)).await.unwrap()),
            Arc::new(EnvironmentCredentialResolver),
            None,
            TraceContentConfig::default(),
            roots,
        )
        .await
        .unwrap();
        assert!(restarted.list_skills().await.unwrap().skills[0].disabled);

        restarted
            .set_skill_disabled(&skill_name, false)
            .await
            .unwrap();
        assert!(!restarted.list_skills().await.unwrap().skills[0].disabled);
        sqlx::query("DELETE FROM skill_status WHERE name = $1")
            .bind(&skill_name)
            .execute(restarted.storage().pool())
            .await
            .unwrap();
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn restarted_core_restores_persisted_sub_agent_identities_without_starting_turns() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let storage = Arc::new(PostgresStorage::connect(Some(&database_url)).await.unwrap());
        storage.migrate().await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple();
        let parent_session_id = SessionId::new(format!("session-agent-restore-parent-{suffix}"));
        let child_session_id = SessionId::new(format!("session-agent-restore-child-{suffix}"));
        let child_turn_id = TurnId::new(format!("turn-agent-restore-child-{suffix}"));
        storage
            .create_session(&SessionInput {
                id: parent_session_id.clone(),
                title: Some("Agent restore parent".to_string()),
                working_directory: "/tmp/openwork-agent-restore".to_string(),
                default_model_id: None,
            })
            .await
            .unwrap();
        storage
            .create_sub_agent_session(&SubAgentSessionInput {
                id: child_session_id.clone(),
                parent_session_id: parent_session_id.clone(),
                task_name: "inspect_runtime".to_string(),
                agent_role: "explorer".to_string(),
                working_directory: "/tmp/openwork-agent-restore".to_string(),
                default_model_id: None,
                spawn_span_id: Some("span-agent-restore".to_string()),
            })
            .await
            .unwrap();
        insert_completed_test_turn(
            &storage,
            &child_session_id,
            &child_turn_id,
            &format!("request-agent-restore-child-{suffix}"),
        )
        .await;

        let restarted = OpenWorkCore::from_storage(Arc::clone(&storage))
            .await
            .unwrap();
        let control = restarted
            .agent_control_for_root(&parent_session_id)
            .await
            .unwrap();

        let restored = control.get("inspect_runtime").unwrap();
        assert_eq!(restored.session_id, child_session_id);
        assert_eq!(restored.agent_role, "explorer");
        assert_eq!(control.list(), vec![restored]);
        assert_eq!(control.active_turns(), 0);
        let parent_turns: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE session_id = $1")
                .bind(parent_session_id.as_str())
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert_eq!(
            parent_turns, 0,
            "restoring identities must not start a Turn"
        );

        restarted.delete_session(&parent_session_id).await.unwrap();
    }

    #[tokio::test]
    async fn restored_sub_agent_can_be_inspected_and_followed_up_after_restart() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let storage = Arc::new(PostgresStorage::connect(Some(&database_url)).await.unwrap());
        storage.migrate().await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple();
        let model_id = format!("model-agent-followup-{suffix}");
        storage
            .upsert_model(&ModelInput {
                id: model_id.clone(),
                display_name: "Agent follow-up test".to_string(),
                provider_kind: "deepseek".to_string(),
                model_name: format!("agent-followup-model-{suffix}"),
                base_url: "http://127.0.0.1:9".to_string(),
                credential_ref: Some("test:credential".to_string()),
                enabled: true,
                capabilities: test_capabilities(),
                config: serde_json::json!({}),
            })
            .await
            .unwrap();
        let parent_session_id = SessionId::new(format!("session-agent-followup-parent-{suffix}"));
        let child_session_id = SessionId::new(format!("session-agent-followup-child-{suffix}"));
        let child_turn_id = TurnId::new(format!("turn-agent-followup-child-{suffix}"));
        storage
            .create_session(&SessionInput {
                id: parent_session_id.clone(),
                title: Some("Agent follow-up parent".to_string()),
                working_directory: "/tmp/openwork-agent-followup".to_string(),
                default_model_id: Some(model_id.clone()),
            })
            .await
            .unwrap();
        storage
            .create_sub_agent_session(&SubAgentSessionInput {
                id: child_session_id.clone(),
                parent_session_id: parent_session_id.clone(),
                task_name: "inspect_runtime".to_string(),
                agent_role: "explorer".to_string(),
                working_directory: "/tmp/openwork-agent-followup".to_string(),
                default_model_id: Some(model_id),
                spawn_span_id: None,
            })
            .await
            .unwrap();
        insert_completed_test_turn(
            &storage,
            &child_session_id,
            &child_turn_id,
            &format!("request-agent-followup-child-{suffix}"),
        )
        .await;

        let restarted = OpenWorkCore::from_storage_with_credentials(
            Arc::clone(&storage),
            Arc::new(FixedTestCredential),
        )
        .await
        .unwrap();
        let control = restarted
            .agent_control_for_root(&parent_session_id)
            .await
            .unwrap();

        control
            .followup("inspect_runtime", "inspect one more detail".to_string())
            .await
            .unwrap();
        assert!(
            SubAgentHost::session_handle(restarted.as_ref(), &child_session_id)
                .await
                .is_ok(),
            "follow-up must reopen the persisted child Session on demand"
        );
        let statuses = control.list_statuses().await.unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].task_name, "inspect_runtime");
        let parent_turns: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE session_id = $1")
                .bind(parent_session_id.as_str())
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert_eq!(parent_turns, 0, "follow-up must not start a parent Turn");

        restarted.delete_session(&parent_session_id).await.unwrap();
    }

    #[tokio::test]
    async fn restart_reconciliation_releases_a_restored_zero_turn_orphan_name() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let storage = Arc::new(PostgresStorage::connect(Some(&database_url)).await.unwrap());
        storage.migrate().await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple();
        let model_id = format!("model-agent-orphan-{suffix}");
        storage
            .upsert_model(&ModelInput {
                id: model_id.clone(),
                display_name: "Agent orphan test".to_string(),
                provider_kind: "deepseek".to_string(),
                model_name: format!("agent-orphan-model-{suffix}"),
                base_url: "http://127.0.0.1:9".to_string(),
                credential_ref: Some("test:credential".to_string()),
                enabled: true,
                capabilities: test_capabilities(),
                config: serde_json::json!({}),
            })
            .await
            .unwrap();
        let parent_session_id = SessionId::new(format!("session-agent-orphan-parent-{suffix}"));
        let child_session_id = SessionId::new(format!("session-agent-orphan-child-{suffix}"));
        storage
            .create_session(&SessionInput {
                id: parent_session_id.clone(),
                title: Some("Agent orphan parent".to_string()),
                working_directory: "/tmp/openwork-agent-orphan".to_string(),
                default_model_id: Some(model_id.clone()),
            })
            .await
            .unwrap();
        storage
            .create_sub_agent_session(&SubAgentSessionInput {
                id: child_session_id.clone(),
                parent_session_id: parent_session_id.clone(),
                task_name: "orphan_lookup".to_string(),
                agent_role: "explorer".to_string(),
                working_directory: "/tmp/openwork-agent-orphan".to_string(),
                default_model_id: Some(model_id),
                spawn_span_id: None,
            })
            .await
            .unwrap();

        let restarted = OpenWorkCore::from_storage_with_credentials(
            Arc::clone(&storage),
            Arc::new(FixedTestCredential),
        )
        .await
        .unwrap();
        let control = restarted
            .agent_control_for_root(&parent_session_id)
            .await
            .unwrap();
        assert!(control.get("orphan_lookup").is_ok());
        let parent_handle = restarted.session_handle(&parent_session_id).await.unwrap();

        restarted
            .reconcile_sub_agent_sessions(&parent_session_id, &parent_handle)
            .await
            .unwrap();

        assert!(
            control.get("orphan_lookup").is_err(),
            "a deleted zero-Turn orphan must not remain addressable"
        );
        assert!(
            storage
                .load_session(&child_session_id)
                .await
                .unwrap()
                .is_none()
        );
        let parent_turns: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE session_id = $1")
                .bind(parent_session_id.as_str())
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert_eq!(parent_turns, 0, "reconciliation must not start a Turn");

        restarted.delete_session(&parent_session_id).await.unwrap();
    }

    #[test]
    fn default_runtime_exposes_checkpoint_bounded_history_readback() {
        let (_, tools) = build_default_agent_and_tools(
            &SessionId::new("session-history-tool"),
            Path::new("/tmp"),
            &[],
            Arc::new(crate::session::NoopSessionStorage),
        )
        .expect("default toolset");

        let definition = tools
            .resolve(COMPACTION_TRANSCRIPT_TOOL_NAME)
            .expect("conversation history tool");
        assert_eq!(definition.risk_hint, openwork_tools::ToolRisk::ReadOnly);
    }

    #[test]
    fn default_runtime_protects_only_the_explicit_agents_skill_root() {
        let root = std::env::temp_dir().join(format!(
            "openwork-core-tool-skills-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let workspace = root.join("workspace");
        let agents_skills = root.join(".agents/skills");
        let agents_skill = agents_skills.join("review/SKILL.md");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(agents_skill.parent().expect("skill parent"))
            .expect("skill directory");
        std::fs::write(&agents_skill, "skill body\n").expect("skill");
        let skill_roots = SkillRoots {
            agents: Some(agents_skills),
        };
        let permission_roots = materialize_skill_permission_roots(&skill_roots);
        let (_, tools) = build_default_agent_and_tools(
            &SessionId::new("session-skill-paths"),
            &workspace,
            &permission_roots,
            Arc::new(crate::session::NoopSessionStorage),
        )
        .expect("default toolset");

        let read = ToolInvocation::new("read", json!({ "path": &agents_skill }));
        assert!(matches!(
            tools.authorize(&read, PermissionMode::Default, &[]),
            Authorization::Allow { .. }
        ));
        let write = ToolInvocation::new(
            "write",
            json!({ "path": agents_skill, "content": "changed" }),
        );
        assert!(matches!(
            tools.authorize(&write, PermissionMode::AcceptEdits, &[]),
            Authorization::Deny { .. }
        ));
        let claude_path = root.join(".claude/skills/not-an-authorized-root/SKILL.md");
        let claude_read = ToolInvocation::new("read", json!({ "path": claude_path }));
        assert!(!matches!(
            tools.authorize(&claude_read, PermissionMode::Default, &[]),
            Authorization::Allow { .. }
        ));
        let project_agents_path = workspace.join(".agents/skills/not-a-user-root/SKILL.md");
        let write = ToolInvocation::new(
            "write",
            json!({ "path": project_agents_path, "content": "ordinary workspace file" }),
        );
        assert!(matches!(
            tools.authorize(&write, PermissionMode::AcceptEdits, &[]),
            Authorization::Allow { .. }
        ));

        std::fs::remove_dir_all(root).expect("cleanup");
    }

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
            message_kind: MessageKind::Normal,
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
            std::panic::panic_any("tool result")
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
