use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use futures_util::stream;
use openwork_agent::{AgentBuilder, AgentDefinition, explorer_definition};
use openwork_chat_state::ChatStateHandle;
use openwork_core::plan::{PlanStepStatus, TurnPlan};
use openwork_core::session::{
    AgentMessageKind, ClientRequestId, CompactionError, CompactionRuntimeState,
    CompactionStateCollector, ControlToolSurface, ConversationCompaction,
    ConversationCompactionKind, NewConversationCompaction, ParentLink, PermissionDecision,
    ResolvedModel, SessionApproval, SessionError, SessionHandle, SessionId, SessionRuntimeConfig,
    SessionStorage, SessionUpdate, SessionUpdateEnvelope, ToolCallId, ToolProgressUpdate,
    TraceFlushResult, TraceRecorder, TraceSignal, TraceStatus, TurnId, TurnOutcome, TurnToolset,
};
use openwork_core::skills::SkillRoots;
use openwork_core::{AgentControl, ModelCapabilities, SubAgentHost, SubAgentSpec};
use openwork_models::model::{
    ContentBlock, FinishReason, Message, ModelCallOptions, ModelError, ModelEvent, ModelPort,
    ModelRequest, ModelResponse, ModelStream, ModelTransportSignalKind, Role, ThinkingConfig,
    TokenUsage, ToolCallBlock, ToolCallState, ToolResultArtifact, ToolResultState,
};
use openwork_tools::{
    AnalysisUnit, ApprovalSessionAction, Effect, InvocationAnalysis, PermissionMode,
    PermissionProfile, ReadonlyProof, Tool, ToolCallContext, ToolExecutionError, ToolId,
    ToolInvocation, ToolProgress as RuntimeToolProgress, ToolRegistryBuilder, ToolResult, ToolRisk,
    ToolSessionContext,
};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Default)]
struct ModelState {
    outcomes: Mutex<VecDeque<Result<ModelResponse, ModelError>>>,
    requests: Mutex<Vec<ModelRequest>>,
    model_attempt_ids: Mutex<Vec<String>>,
}

struct FakeModel {
    state: Arc<ModelState>,
}

#[async_trait]
impl ModelPort for FakeModel {
    async fn invoke(
        &self,
        request: ModelRequest,
        options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.state.requests.lock().unwrap().push(request);
        self.state
            .model_attempt_ids
            .lock()
            .unwrap()
            .push(options.model_attempt_id.clone());
        let outcome = self
            .state
            .outcomes
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| ModelError::protocol("fake model has no response"))?;
        options.observe_transport(1, ModelTransportSignalKind::Started);
        let response = match outcome {
            Ok(response) => response,
            Err(error) => {
                options.observe_transport(
                    1,
                    ModelTransportSignalKind::Failed {
                        error: error.clone(),
                        retry_delay_ms: None,
                    },
                );
                return Err(error);
            }
        };
        options.observe_transport(
            1,
            ModelTransportSignalKind::Succeeded {
                provider_request_id: response.provider_request_id.clone(),
            },
        );
        let mut events = Vec::new();
        if !response.text.is_empty() {
            events.push(Ok(ModelEvent::TextDelta {
                index: 0,
                delta: response.text.clone(),
            }));
        }
        events.push(Ok(ModelEvent::ResponseCompleted {
            response: Box::new(response),
        }));
        Ok(Box::pin(stream::iter(events)))
    }
}

#[derive(Default)]
struct ToolState {
    invocations: Mutex<Vec<ToolInvocation>>,
    results: Mutex<VecDeque<ToolResult>>,
}

struct FakeTool {
    id: ToolId,
    risk: ToolRisk,
    state: Arc<ToolState>,
}

#[async_trait]
impl Tool for FakeTool {
    type Input = serde_json::Value;
    type Output = ToolResult;

    fn id(&self) -> ToolId {
        self.id.clone()
    }

    fn description(&self) -> &'static str {
        "Fake tool used by the session runtime integration tests."
    }

    fn risk(&self) -> ToolRisk {
        self.risk
    }

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        let raw = self.id.to_string();
        let path = input
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("fixture");
        if let Some(key) = input
            .get("readonlyProofKey")
            .and_then(serde_json::Value::as_str)
        {
            return InvocationAnalysis::new(
                raw.clone(),
                vec![AnalysisUnit {
                    display: raw.clone(),
                    effects: vec![
                        Effect::Exec {
                            program: raw,
                            args: Vec::new(),
                        },
                        Effect::read(&session.working_directory),
                    ],
                    allow_eligible: true,
                    readonly_proof: Some(ReadonlyProof {
                        key: key.to_string(),
                    }),
                    filesystem_command_proof: false,
                }],
            );
        }
        if input
            .get("filesystemCommandProof")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return InvocationAnalysis::new(
                raw.clone(),
                vec![AnalysisUnit {
                    display: "mkdir src/x".to_string(),
                    effects: vec![
                        Effect::Exec {
                            program: "mkdir".to_string(),
                            args: vec!["src/x".to_string()],
                        },
                        Effect::write(session.normalize_effect_path(path)),
                    ],
                    allow_eligible: true,
                    readonly_proof: None,
                    filesystem_command_proof: true,
                }],
            );
        }
        if self.risk == ToolRisk::ProcessExecution
            && let Some(program) = input.get("program").and_then(serde_json::Value::as_str)
        {
            let args = input
                .get("args")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            return InvocationAnalysis::new(
                raw.clone(),
                vec![AnalysisUnit::new(
                    format!("{} {}", program, args.join(" ")).trim().to_string(),
                    vec![Effect::Exec {
                        program: program.to_string(),
                        args,
                    }],
                )],
            );
        }
        let effect = match self.risk {
            ToolRisk::ReadOnly => Effect::read(session.normalize_effect_path(path)),
            ToolRisk::WorkspaceMutation => Effect::write(session.normalize_effect_path(path)),
            ToolRisk::ProcessExecution => Effect::Exec {
                program: raw.clone(),
                args: Vec::new(),
            },
        };
        InvocationAnalysis::new(raw.clone(), vec![AnalysisUnit::new(raw, vec![effect])])
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolExecutionError> {
        let wait_for_cancel = input["waitForCancel"] == true;
        let progress_message = input["emitProgress"].as_str().map(str::to_string);
        let replacement = input["replaceProjectInstruction"]
            .as_str()
            .map(str::to_string);
        self.state
            .invocations
            .lock()
            .unwrap()
            .push(ToolInvocation::new(self.id.to_string(), input));
        if let Some(message) = progress_message {
            call.report_progress(RuntimeToolProgress::Message { message });
        }
        if wait_for_cancel {
            call.cancel.cancelled().await;
            return Ok(ToolResult::cancelled("fake tool cancelled"));
        }
        if let Some(content) = replacement {
            fs::write(session.working_directory.join("AGENTS.md"), content)
                .map_err(|error| ToolExecutionError::execution(error.to_string()))?;
        }
        Ok(self
            .state
            .results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ToolResult::succeeded("tool ok")))
    }
}

#[derive(Default)]
struct RecordingStorage {
    events: Mutex<Vec<String>>,
    model_submissions: Mutex<Vec<(u32, u8)>>,
    fail_assistant: bool,
    fail_tool_result: AtomicBool,
    fail_compaction: AtomicBool,
    fail_plan_commit: AtomicBool,
    compactions: Mutex<Vec<ConversationCompaction>>,
    plans: Mutex<Vec<TurnPlan>>,
    agent_message_ids: Mutex<HashSet<String>>,
    /// 外层 Option = finish_turn 是否被调用过；内层 = 该 Turn 有没有计划。
    unfinished_plan_steps: Mutex<Option<Option<usize>>>,
}

#[async_trait]
impl SessionStorage for RecordingStorage {
    async fn begin_turn(
        &self,
        _session_id: &SessionId,
        _turn_id: &TurnId,
        _client_request_id: &ClientRequestId,
        _model: &ResolvedModel,
        _contextual_messages: &[Message],
        _user_message: &Message,
    ) -> Result<(), String> {
        self.events.lock().unwrap().push("begin_turn".to_string());
        Ok(())
    }

    async fn begin_model_call(
        &self,
        _turn_id: &TurnId,
        model_call_index: u32,
        submission_attempt: u8,
    ) -> Result<(), String> {
        self.model_submissions
            .lock()
            .unwrap()
            .push((model_call_index, submission_attempt));
        self.events
            .lock()
            .unwrap()
            .push(format!("model_{model_call_index}"));
        Ok(())
    }

    async fn append_assistant_message(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
        _usage: Option<TokenUsage>,
    ) -> Result<String, String> {
        self.events.lock().unwrap().push("assistant".to_string());
        if self.fail_assistant {
            Err("assistant write failed".to_string())
        } else {
            Ok("msg-recording".to_string())
        }
    }

    async fn append_tool_result(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
    ) -> Result<(), String> {
        self.events.lock().unwrap().push("tool_result".to_string());
        if self.fail_tool_result.load(Ordering::Relaxed) {
            Err("tool result write failed".to_string())
        } else {
            Ok(())
        }
    }

    async fn append_agent_message(
        &self,
        _turn_id: &TurnId,
        message_id: &str,
        _message: &Message,
    ) -> Result<bool, String> {
        let inserted = self
            .agent_message_ids
            .lock()
            .unwrap()
            .insert(message_id.to_string());
        if inserted {
            self.events
                .lock()
                .unwrap()
                .push("agent_message".to_string());
        }
        Ok(inserted)
    }

    async fn append_world_state_fragment(
        &self,
        _turn_id: &TurnId,
        message_id: &str,
        _message: &Message,
    ) -> Result<bool, String> {
        let inserted = self
            .agent_message_ids
            .lock()
            .unwrap()
            .insert(message_id.to_string());
        if inserted {
            self.events.lock().unwrap().push("world_state".to_string());
        }
        Ok(inserted)
    }

    async fn finish_turn(
        &self,
        _turn_id: &TurnId,
        _outcome: &TurnOutcome,
        unfinished_plan_steps: Option<usize>,
    ) -> Result<(), String> {
        self.events.lock().unwrap().push("finish_turn".to_string());
        *self.unfinished_plan_steps.lock().unwrap() = Some(unfinished_plan_steps);
        Ok(())
    }

    async fn load_turn_plan(&self, _turn_id: &TurnId) -> Result<Option<TurnPlan>, String> {
        Ok(self.plans.lock().unwrap().last().cloned())
    }

    async fn load_session_turn_plans(
        &self,
        _session_id: &SessionId,
    ) -> Result<Vec<TurnPlan>, String> {
        Ok(self.plans.lock().unwrap().clone())
    }

    async fn commit_plan_update(
        &self,
        _turn_id: &TurnId,
        plan: &TurnPlan,
        _success_tool_result: &Message,
    ) -> Result<(), String> {
        // 一次事件即代表"计划与成功 Tool Result 一起落库"，与真实实现的原子性对应。
        self.events.lock().unwrap().push("plan_commit".to_string());
        if self.fail_plan_commit.load(Ordering::Relaxed) {
            return Err("plan commit failed".to_string());
        }
        self.plans.lock().unwrap().push(plan.clone());
        Ok(())
    }

    async fn save_conversation_compaction(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, String> {
        self.events.lock().unwrap().push("compaction".to_string());
        if self.fail_compaction.load(Ordering::Relaxed) {
            return Err("compaction write failed".to_string());
        }
        let mut compactions = self.compactions.lock().unwrap();
        let sequence = i64::try_from(compactions.len() + 1).unwrap();
        let compaction = ConversationCompaction {
            id: format!("compaction-{sequence}"),
            session_id: session_id.to_string(),
            sequence,
            through_message_sequence: i64::from(input.source_message_count),
            replaced_through_message_sequence: i64::from(input.source_message_count),
            source_message_count: input.source_message_count,
            checkpoint_format_version: 1,
            kind: input.kind,
            summary_format_version: 1,
            last_user_message_id: input
                .last_user_message_id
                .or_else(|| Some("fake-user".to_string())),
            last_user_message_sequence: input.last_user_message_sequence.or(Some(1)),
            resolved_model_name: input.resolved_model_name,
            summary: input.summary,
            runtime_state: input.runtime_state,
            runtime_reminder_format_version: 1,
            runtime_reminder: input.runtime_reminder,
            trigger_turn_id: input.trigger_turn_id.map(|turn_id| turn_id.to_string()),
            parent_compaction_id: None,
            input_tokens: input.input_tokens,
            output_tokens: input.output_tokens,
            created_at: "2026-07-23T00:00:00Z".to_string(),
        };
        compactions.push(compaction.clone());
        Ok(compaction)
    }

    async fn load_compaction_source_messages(
        &self,
        _session_id: &SessionId,
    ) -> Result<Vec<Message>, String> {
        Ok(Vec::new())
    }

    async fn load_latest_compaction_runtime_state(
        &self,
        _session_id: &SessionId,
    ) -> Result<Option<CompactionRuntimeState>, String> {
        Ok(self
            .compactions
            .lock()
            .unwrap()
            .last()
            .map(|compaction| compaction.runtime_state.clone()))
    }

    async fn rewind_conversation_compaction(
        &self,
        _session_id: &SessionId,
        _compaction_id: &str,
        _runtime_state: CompactionRuntimeState,
        _runtime_reminder: String,
    ) -> Result<ConversationCompaction, String> {
        Err("rewind is not supported by this recording storage".to_string())
    }

    async fn load_compaction_last_user_message(
        &self,
        _session_id: &SessionId,
        _compaction_id: &str,
    ) -> Result<Message, String> {
        Err("checkpoint user lookup is not supported by this recording storage".to_string())
    }

    async fn delete_conversation_compaction(
        &self,
        _session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<(), String> {
        let mut compactions = self.compactions.lock().unwrap();
        let Some(index) = compactions
            .iter()
            .position(|compaction| compaction.id == compaction_id)
        else {
            return Err("compaction not found".to_string());
        };
        compactions.remove(index);
        Ok(())
    }
}

#[derive(Default)]
struct RecordingTrace {
    signals: Mutex<Vec<TraceSignal>>,
}

#[async_trait]
impl TraceRecorder for RecordingTrace {
    fn record(&self, signal: TraceSignal) {
        self.signals.lock().unwrap().push(signal);
    }

    async fn flush_turn(&self, _turn_id: &TurnId) -> TraceFlushResult {
        TraceFlushResult {
            flushed: true,
            ..TraceFlushResult::default()
        }
    }
}

struct RuntimeFixture {
    handle: SessionHandle,
    updates: broadcast::Receiver<SessionUpdateEnvelope>,
    global_updates: broadcast::Receiver<SessionUpdateEnvelope>,
    model: Arc<ModelState>,
    tools: Arc<ToolState>,
    storage: Arc<RecordingStorage>,
    trace: Arc<RecordingTrace>,
    chat: ChatStateHandle,
    workspace: TestWorkspace,
}

struct RuntimeOptions {
    session_id: SessionId,
    model_capabilities: ModelCapabilities,
    approval: SessionApproval,
    parent_link: Option<ParentLink>,
    agent_control: Option<AgentControl>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            session_id: SessionId::new("session-test"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::Interactive,
            parent_link: None,
            agent_control: None,
        }
    }
}

fn test_capabilities(window_tokens: u64, max_output_tokens: u32) -> ModelCapabilities {
    ModelCapabilities {
        context_window_tokens: window_tokens,
        max_output_tokens,
        max_reasoning_tokens: None,
        accepts_data_blocks: true,
    }
}

#[derive(Default)]
struct SessionHandleHost {
    handles: Mutex<HashMap<SessionId, SessionHandle>>,
}

#[derive(Default)]
struct SlotHoldingHost {
    specs: Mutex<Vec<SubAgentSpec>>,
    slots: Mutex<HashMap<SessionId, openwork_core::TurnSlot>>,
}

#[derive(Default)]
struct SpawningSessionHost {
    handles: Mutex<HashMap<SessionId, SessionHandle>>,
    workspaces: Mutex<Vec<TestWorkspace>>,
    control: Mutex<Option<AgentControl>>,
}

#[async_trait]
impl SubAgentHost for SlotHoldingHost {
    async fn start_sub_agent(&self, spec: SubAgentSpec) -> Result<(), String> {
        self.specs.lock().unwrap().push(spec);
        Ok(())
    }

    async fn start_sub_agent_turn(
        &self,
        session_id: &SessionId,
        _message: String,
        turn_slot: openwork_core::TurnSlot,
    ) -> Result<(), String> {
        self.slots
            .lock()
            .unwrap()
            .insert(session_id.clone(), turn_slot);
        Ok(())
    }

    async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String> {
        Err(format!(
            "session {session_id} has no actor in this tool dispatch test"
        ))
    }
}

impl SessionHandleHost {
    fn insert(&self, handle: SessionHandle) {
        self.handles
            .lock()
            .unwrap()
            .insert(handle.session_id().clone(), handle);
    }
}

impl SpawningSessionHost {
    fn install_control(&self, control: AgentControl) {
        *self.control.lock().unwrap() = Some(control);
    }

    fn insert(&self, handle: SessionHandle) {
        self.handles
            .lock()
            .unwrap()
            .insert(handle.session_id().clone(), handle);
    }
}

#[async_trait]
impl SubAgentHost for SpawningSessionHost {
    async fn start_sub_agent(&self, spec: SubAgentSpec) -> Result<(), String> {
        let control = self
            .control
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "agent control is not installed".to_string())?;
        let workspace = TestWorkspace::new();
        let child = runtime_with_options(
            vec![Ok(response(
                &format!("finding from {}", spec.task_name),
                Vec::new(),
            ))],
            Vec::new(),
            PermissionMode::Default,
            false,
            workspace,
            SkillRoots::default(),
            RuntimeOptions {
                session_id: spec.session_id,
                model_capabilities: test_capabilities(200_000, 32_768),
                approval: SessionApproval::NonInteractive,
                parent_link: Some(ParentLink {
                    parent_session_id: spec.parent_session_id,
                    task_name: spec.task_name,
                    agent_control: control.clone(),
                }),
                agent_control: Some(control),
            },
        );
        self.insert(child.handle.clone());
        self.workspaces.lock().unwrap().push(child.workspace);
        Ok(())
    }

    async fn start_sub_agent_turn(
        &self,
        session_id: &SessionId,
        message: String,
        turn_slot: openwork_core::TurnSlot,
    ) -> Result<(), String> {
        self.session_handle(session_id)
            .await?
            .start_sub_agent_turn(message, BTreeSet::new(), turn_slot)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String> {
        self.handles
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| format!("session {session_id} is not registered"))
    }
}

#[async_trait]
impl SubAgentHost for SessionHandleHost {
    async fn start_sub_agent(&self, _spec: SubAgentSpec) -> Result<(), String> {
        Err("spawning is outside the P1 test seam".to_string())
    }

    async fn start_sub_agent_turn(
        &self,
        session_id: &SessionId,
        message: String,
        turn_slot: openwork_core::TurnSlot,
    ) -> Result<(), String> {
        self.session_handle(session_id)
            .await?
            .start_sub_agent_turn(message, BTreeSet::new(), turn_slot)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String> {
        self.handles
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| format!("session {session_id} is not registered"))
    }
}

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "openwork-session-runtime-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).expect("workspace");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write_instructions(&self, content: impl AsRef<[u8]>) {
        fs::write(self.root.join("AGENTS.md"), content).expect("instructions");
    }

    fn write_skill(&self, root: &Path, name: &str, description: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("skill directory");
        let path = directory.join("SKILL.md");
        fs::write(
            &path,
            format!("---\nname: {name}\ndescription: {description}\n---\nBody\n"),
        )
        .expect("skill");
        fs::canonicalize(path).expect("canonical skill path")
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn runtime(
    responses: Vec<ModelResponse>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
) -> RuntimeFixture {
    runtime_with_outcomes_in_workspace(
        responses.into_iter().map(Ok).collect(),
        tool_results,
        permission_mode,
        fail_assistant,
        TestWorkspace::new(),
    )
}

fn runtime_with_capabilities(
    responses: Vec<ModelResponse>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
    model_capabilities: ModelCapabilities,
) -> RuntimeFixture {
    runtime_with_options(
        responses.into_iter().map(Ok).collect(),
        tool_results,
        permission_mode,
        fail_assistant,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            model_capabilities,
            ..RuntimeOptions::default()
        },
    )
}

fn runtime_in_workspace(
    responses: Vec<ModelResponse>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
    workspace: TestWorkspace,
) -> RuntimeFixture {
    runtime_with_outcomes_in_workspace(
        responses.into_iter().map(Ok).collect(),
        tool_results,
        permission_mode,
        fail_assistant,
        workspace,
    )
}

fn runtime_in_workspace_with_skill_roots(
    responses: Vec<ModelResponse>,
    permission_mode: PermissionMode,
    workspace: TestWorkspace,
    skill_roots: SkillRoots,
) -> RuntimeFixture {
    runtime_with_outcomes_in_workspace_and_skill_roots(
        responses.into_iter().map(Ok).collect(),
        Vec::new(),
        permission_mode,
        false,
        workspace,
        skill_roots,
    )
}

fn runtime_with_outcomes(
    outcomes: Vec<Result<ModelResponse, ModelError>>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
) -> RuntimeFixture {
    runtime_with_outcomes_in_workspace(
        outcomes,
        tool_results,
        permission_mode,
        fail_assistant,
        TestWorkspace::new(),
    )
}

fn runtime_with_outcomes_and_capabilities(
    outcomes: Vec<Result<ModelResponse, ModelError>>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
    model_capabilities: ModelCapabilities,
) -> RuntimeFixture {
    runtime_with_options(
        outcomes,
        tool_results,
        permission_mode,
        fail_assistant,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            model_capabilities,
            ..RuntimeOptions::default()
        },
    )
}

fn runtime_with_outcomes_in_workspace(
    outcomes: Vec<Result<ModelResponse, ModelError>>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
    workspace: TestWorkspace,
) -> RuntimeFixture {
    runtime_with_outcomes_in_workspace_and_skill_roots(
        outcomes,
        tool_results,
        permission_mode,
        fail_assistant,
        workspace,
        SkillRoots::default(),
    )
}

fn runtime_with_outcomes_in_workspace_and_skill_roots(
    outcomes: Vec<Result<ModelResponse, ModelError>>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
    workspace: TestWorkspace,
    skill_roots: SkillRoots,
) -> RuntimeFixture {
    runtime_with_options(
        outcomes,
        tool_results,
        permission_mode,
        fail_assistant,
        workspace,
        skill_roots,
        RuntimeOptions::default(),
    )
}

fn runtime_with_options(
    outcomes: Vec<Result<ModelResponse, ModelError>>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
    workspace: TestWorkspace,
    skill_roots: SkillRoots,
    options: RuntimeOptions,
) -> RuntimeFixture {
    let working_directory = workspace.path().to_path_buf();
    let is_sub_agent = options.parent_link.is_some();
    let agent = AgentBuilder::new(if is_sub_agent {
        explorer_definition()
    } else {
        AgentDefinition::default()
    })
    .build()
    .expect("agent");
    let chat = ChatStateHandle::spawn(Vec::new()).expect("chat");
    let model = Arc::new(ModelState {
        outcomes: Mutex::new(outcomes.into()),
        requests: Mutex::new(Vec::new()),
        model_attempt_ids: Mutex::new(Vec::new()),
    });
    let tools = Arc::new(ToolState {
        invocations: Mutex::new(Vec::new()),
        results: Mutex::new(tool_results.into()),
    });
    let storage = Arc::new(RecordingStorage {
        events: Mutex::new(Vec::new()),
        model_submissions: Mutex::new(Vec::new()),
        fail_assistant,
        fail_tool_result: AtomicBool::new(false),
        fail_compaction: AtomicBool::new(false),
        fail_plan_commit: AtomicBool::new(false),
        compactions: Mutex::new(Vec::new()),
        plans: Mutex::new(Vec::new()),
        agent_message_ids: Mutex::new(HashSet::new()),
        unfinished_plan_steps: Mutex::new(None),
    });
    let trace = Arc::new(RecordingTrace::default());
    let registry = AgentDefinition::default().tool_names.into_iter().fold(
        ToolRegistryBuilder::new(),
        |registry, name| {
            let risk = if name == "bash" {
                ToolRisk::ProcessExecution
            } else if matches!(name.as_str(), "write" | "edit") {
                ToolRisk::WorkspaceMutation
            } else {
                ToolRisk::ReadOnly
            };
            registry.register(FakeTool {
                id: ToolId::new(name),
                risk,
                state: Arc::clone(&tools),
            })
        },
    );
    let toolset = registry
        .finalize(
            agent.toolset_config(),
            ToolSessionContext::local(
                working_directory.clone(),
                PermissionProfile::from_builtin_rules(working_directory.clone()),
            ),
        )
        .expect("toolset");
    let (global_update_tx, global_updates) = broadcast::channel(512);
    let agent_control = options.agent_control.clone().or_else(|| {
        options
            .parent_link
            .as_ref()
            .map(|parent| parent.agent_control.clone())
    });
    let handle = SessionHandle::spawn_with_global_updates(
        SessionRuntimeConfig {
            session_id: options.session_id,
            working_directory,
            skill_roots,
            resolved_model: ResolvedModel::new(
                None::<String>,
                "test",
                "test-model",
                options.model_capabilities,
            ),
            agent,
            chat: chat.clone(),
            model: Arc::new(FakeModel {
                state: Arc::clone(&model),
            }),
            tools: Arc::new(
                TurnToolset::new(
                    Arc::new(toolset),
                    if is_sub_agent {
                        ControlToolSurface::SubAgent
                    } else {
                        ControlToolSurface::Root {
                            max_active_sub_agent_turns: 3,
                        }
                    },
                )
                .expect("no control tool collision"),
            ),
            storage: storage.clone(),
            compaction_state: Arc::new(CompactionStateCollector::default()),
            trace: trace.clone(),
            permission_mode,
            approval: options.approval,
            parent_link: options.parent_link,
            agent_control,
        },
        global_update_tx,
    );
    let updates = handle.subscribe_updates();
    RuntimeFixture {
        handle,
        updates,
        global_updates,
        model,
        tools,
        storage,
        trace,
        chat,
        workspace,
    }
}

#[tokio::test]
async fn session_actor_forwards_updates_to_the_core_global_bus() {
    let mut fixture = runtime(
        vec![response("done", Vec::new())],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );

    start(&fixture).await;
    let outcome = wait_for_terminal(&mut fixture.global_updates).await;

    assert!(matches!(outcome, TurnOutcome::Completed { .. }));
}

#[tokio::test]
async fn acc_13_permission_mode_is_in_memory_and_reflected_in_snapshots() {
    let fixture = runtime(
        vec![response("done", Vec::new())],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    let files_before = fs::read_dir(fixture.workspace.path())
        .expect("workspace before mode change")
        .count();

    assert_eq!(
        fixture
            .handle
            .snapshot()
            .await
            .expect("default snapshot")
            .permission_mode,
        PermissionMode::Default
    );
    assert_eq!(
        fixture
            .handle
            .set_permission_mode(PermissionMode::AcceptEdits)
            .await
            .expect("set permission mode"),
        PermissionMode::AcceptEdits
    );
    assert_eq!(
        fixture
            .handle
            .snapshot()
            .await
            .expect("updated snapshot")
            .permission_mode,
        PermissionMode::AcceptEdits
    );
    assert_eq!(
        fs::read_dir(fixture.workspace.path())
            .expect("workspace after mode change")
            .count(),
        files_before,
        "changing permission mode must not write a file"
    );
}

#[tokio::test]
async fn permission_mode_change_controls_subsequent_tool_authorization() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "write", r#"{"path":"README.md"}"#)],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    fixture
        .handle
        .set_permission_mode(PermissionMode::AcceptEdits)
        .await
        .expect("set permission mode");
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
    let signals = fixture.trace.signals.lock().unwrap();
    let tool = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("tool trace");
    assert_eq!(
        tool.attributes.permission_mode.as_deref(),
        Some("accept_edits")
    );
    assert_eq!(
        tool.attributes.permission_mode_origin.as_deref(),
        Some("user_toggle")
    );
}

fn response(text: &str, tool_calls: Vec<ToolCallBlock>) -> ModelResponse {
    ModelResponse {
        response_id: Some("response".to_string()),
        provider_request_id: Some("request".to_string()),
        model: Some("test-model".to_string()),
        text: text.to_string(),
        reasoning_text: None,
        tool_calls,
        provider_opaque_blocks: Vec::new(),
        finish_reason: if text.is_empty() {
            FinishReason::ToolUse
        } else {
            FinishReason::Stop
        },
        raw_finish_reason: None,
        usage: None,
    }
}

fn compaction_summary() -> &'static str {
    concat!(
        "<conversation_summary format_version=\"1\">\n",
        "## 1. Primary Request and Intent\nImplement conversation compaction while retaining the durable raw transcript and continuing the same task safely after the projection changes. The request requires deterministic reconstruction rather than deletion of prior messages.\n",
        "## 2. Key Technical Concepts\nConversation projection, durable checkpoint, synthetic provenance, stable system reminder, PostgreSQL boundary, and exact user-message replay are the relevant concepts.\n",
        "## 3. Files and Code Sections\nThe session actor, chat state, compaction coordinator, PostgreSQL storage implementation, context builder, desktop command bridge, and their focused tests are in scope.\n",
        "## 4. Errors and Fixes\nThe earlier turn completed without an error. Any compaction failure must preserve the old Conversation and must not install a partial checkpoint.\n",
        "## 5. Problem Solving and Decisions\nKeep System Context independent, retain raw messages, install a typed compacted projection, and validate the summary structure before persistence.\n",
        "## 6. User Messages and Constraints\nThe user requested implementation without committing changes and excluded plan, memory, todo, subagent, and MCP runtime features.\n",
        "## 7. Pending Tasks\nVerify the active projection, persistence fields, restart reconstruction, overflow resubmission, rewind behavior, and desktop contract.\n",
        "## 8. Current Work\nThe first normal model turn completed and the runtime is now generating a manual checkpoint from the current visible Conversation.\n",
        "## 9. Next Safe Action\nInstall the validated checkpoint atomically, then wait for the next real user request before starting another ordinary model call.\n",
        "</conversation_summary>"
    )
}

fn tool_call(id: &str, name: &str, input: &str) -> ToolCallBlock {
    ToolCallBlock {
        id: id.to_string(),
        name: name.to_string(),
        input: input.to_string(),
        state: ToolCallState::Submitted,
    }
}

async fn start(fixture: &RuntimeFixture) -> TurnId {
    start_with_request(fixture, "client-request").await
}

async fn start_with_request(fixture: &RuntimeFixture, client_request_id: &str) -> TurnId {
    fixture
        .handle
        .start_turn(
            ClientRequestId::new(client_request_id),
            openwork_core::session::PreparedTurnInput::text("do the task"),
            BTreeSet::new(),
        )
        .await
        .expect("turn accepted")
        .turn_id
}

/// 请求副本里全部 User 消息的文本。
fn user_message_texts(request: &ModelRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .filter(|message| message.role == Role::User)
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::Text(text)) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

/// System 前缀的字节表示，用于断言相邻 Model Call 之间逐字节一致。
fn system_prefix_bytes(request: &ModelRequest) -> Vec<u8> {
    let system: Vec<_> = request
        .messages
        .iter()
        .filter(|message| message.role == Role::System)
        .collect();
    serde_json::to_vec(&system).expect("system prefix bytes")
}

/// 一条消息是不是 world-state fragment，靠三个 section 的正文标记识别。
fn is_world_state_message(message: &Message) -> bool {
    message.role == Role::User
        && matches!(
            message.content.first(),
            Some(ContentBlock::Text(text))
                if text.text.contains("<user_project_context")
                    || text.text.contains("<project_instructions>")
                    || text.text.contains("<available_skills>")
                    || text.text.contains("不再适用。")
        )
}

/// 请求里的 role 序列，**跳过 world-state 消息**。
///
/// world-state 是独立机制，有自己的测试。让每个无关测试都去数它的条数，等于
/// 把三个 section 的变化耦合进整个测试套件——工作区多一个顶层目录就要改一堆
/// 断言，那些断言也就不再说明它们本来要说明的事。
fn roles_ignoring_world_state(request: &ModelRequest) -> Vec<Role> {
    request
        .messages
        .iter()
        .filter(|message| !is_world_state_message(message))
        .map(|message| message.role)
        .collect()
}

/// 存储事件序列，**跳过 world_state 写入**。理由同上。
///
/// world-state 的落库顺序（必须在 model_N 之前）由专门的测试断言，不摊派给
/// 每一条生命周期测试。
fn events_ignoring_world_state(fixture: &RuntimeFixture) -> Vec<String> {
    fixture
        .storage
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.as_str() != "world_state")
        .cloned()
        .collect()
}

/// 请求里 world-state 消息的条数，靠三个 section 的正文标记识别。
fn world_state_message_count(request: &ModelRequest) -> usize {
    user_message_texts(request)
        .iter()
        .filter(|text| {
            text.contains("<user_project_context")
                || text.contains("<project_instructions>")
                || text.contains("<available_skills>")
                || text.contains("不再适用。")
        })
        .count()
}

async fn wait_for_terminal(
    updates: &mut broadcast::Receiver<SessionUpdateEnvelope>,
) -> TurnOutcome {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.expect("session update");
            if let SessionUpdate::TurnFinished { outcome } = event.update {
                return outcome;
            }
        }
    })
    .await
    .expect("turn timed out")
}

async fn wait_for_permission(
    updates: &mut broadcast::Receiver<SessionUpdateEnvelope>,
) -> (TurnId, ToolCallId) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.expect("session update");
            if let SessionUpdate::PermissionRequested { request } = event.update {
                return (request.turn_id, request.tool_call_id);
            }
        }
    })
    .await
    .expect("permission timed out")
}

async fn wait_for_tool_start(updates: &mut broadcast::Receiver<SessionUpdateEnvelope>) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.expect("session update");
            if matches!(event.update, SessionUpdate::ToolCallStarted { .. }) {
                return;
            }
        }
    })
    .await
    .expect("tool start timed out");
}

#[tokio::test]
async fn no_tool_turn_completes_after_one_model_call() {
    let mut fixture = runtime(
        vec![response("done", Vec::new())],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;

    assert_eq!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed {
            final_text: "done".to_string()
        }
    );
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "test-model");
    assert_eq!(
        roles_ignoring_world_state(&requests[0]),
        [Role::System, Role::User]
    );
    assert_eq!(requests[0].temperature, None);
    assert_eq!(requests[0].max_output_tokens, Some(32_768));
    assert_eq!(requests[0].thinking, None);
    let submitted_messages = requests[0].messages.clone();
    drop(requests);
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    assert_eq!(
        events_ignoring_world_state(&fixture),
        ["begin_turn", "model_1", "assistant", "finish_turn"]
    );
    let signals = fixture.trace.signals.lock().unwrap();
    let finished = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ModelCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("model call trace");
    assert_eq!(
        finished.started.payloads.request,
        Some(serde_json::to_value(submitted_messages).unwrap())
    );
    assert_eq!(
        finished.response_message_id.as_deref(),
        Some("msg-recording")
    );
    assert!(finished.response_payload.is_none());
}

/// System 前缀只有一段，三个 section 以 User 消息进入 Conversation（§8.0、§16.2）。
///
/// 前缀里再没有会变的东西，所以它在整个 Session 内逐字节不变；三段内容改用正文
/// 标记而不是 role 表明来源。
#[tokio::test]
async fn world_state_reaches_the_model_as_user_messages_not_system_parts() {
    let workspace = TestWorkspace::new();
    workspace.write_instructions("永远先跑测试");
    let skill_root = workspace.path().join("skills");
    fs::create_dir_all(&skill_root).expect("skill root");
    workspace.write_skill(&skill_root, "commit", "Create a commit.");
    let mut fixture = runtime_in_workspace_with_skill_roots(
        vec![response("done", Vec::new())],
        PermissionMode::AcceptEdits,
        workspace,
        SkillRoots {
            agents: Some(skill_root),
        },
    );

    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let requests = fixture.model.requests.lock().unwrap();
    let system: Vec<_> = requests[0]
        .messages
        .iter()
        .filter(|message| message.role == Role::System)
        .collect();
    assert_eq!(system.len(), 1, "System 前缀只应有 core/agent-system 一段");

    let user_text = user_message_texts(&requests[0]);
    assert!(
        user_text
            .iter()
            .any(|text| text.contains("<user_project_context")),
        "项目上下文应作为 User 消息出现"
    );
    assert!(
        user_text
            .iter()
            .any(|text| text.contains("<project_instructions>") && text.contains("永远先跑测试")),
        "AGENTS.md 应带标记作为 User 消息出现"
    );
    assert!(
        user_text
            .iter()
            .any(|text| text.contains("<available_skills>")),
        "skill 清单应作为 User 消息出现"
    );
}

/// 没有变化时，第二次 Model Call 不新增任何 world-state 消息，前缀逐字节相同。
///
/// 这是整个改造的收益本身。任何一处不确定性都会让它失败。
#[tokio::test]
async fn an_unchanged_world_adds_nothing_to_the_next_model_call() {
    let workspace = TestWorkspace::new();
    workspace.write_instructions("规范");
    let mut fixture = runtime_in_workspace(
        vec![
            response(
                "",
                vec![tool_call("call-1", "read", r#"{"path":"AGENTS.md"}"#)],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        workspace,
    );

    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    // §8.3 的顺序：fragment 必须在调用 Provider 之前落库。反过来做的话，落库
    // 失败时基线已经推进，那次更新永久丢失且不报错。
    let events = fixture.storage.events.lock().unwrap().clone();
    let first_world_state = events
        .iter()
        .position(|event| event == "world_state")
        .expect("首次 Model Call 之前应有 world-state 落库");
    let first_model_call = events
        .iter()
        .position(|event| event == "model_1")
        .expect("model_1");
    assert!(
        first_world_state < first_model_call,
        "world-state 必须先落库再调用 Provider：{events:?}"
    );

    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        system_prefix_bytes(&requests[0]),
        system_prefix_bytes(&requests[1]),
        "相邻 Model Call 的 System 前缀必须逐字节一致"
    );
    // 本 fixture 没有配置任何 skill，因此 skills_catalog 是缺失的。按 §8.2，
    // "一直不存在"既不渲染也不发消息——所以正确数量是 2 而不是 3。
    //
    // 先确认第一次确实发了消息，否则下面的相等断言在"一条都没有"时也成立，
    // 会变成一条永远为真的假绿。
    assert_eq!(
        world_state_message_count(&requests[0]),
        2,
        "首次 Model Call 应发出项目上下文与 AGENTS.md 两个 section"
    );
    assert!(
        !user_message_texts(&requests[0])
            .iter()
            .any(|text| text.contains("<available_skills>")),
        "没有 skill 时不该为一个不存在的 section 发消息"
    );
    assert_eq!(
        world_state_message_count(&requests[0]),
        world_state_message_count(&requests[1]),
        "没有变化就不该再追加 world-state 消息"
    );
}

/// AGENTS.md 在 Turn 中途被改，同一个 Turn 的下一次 Model Call 就能看到（§8.3）。
///
/// 采样按 Model Call 而不是按 Turn，所以模型能看见自己动作的后果。重发带取代
/// 声明，否则历史里会同时躺着两份互相矛盾的项目规范。
#[tokio::test]
async fn an_agents_md_edit_is_seen_by_the_next_model_call_in_the_same_turn() {
    let workspace = TestWorkspace::new();
    workspace.write_instructions("instruction-v1");
    let mut fixture = runtime_in_workspace(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-update-instructions",
                    "read",
                    r#"{"replaceProjectInstruction":"instruction-v2"}"#,
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        workspace,
    );

    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let second = user_message_texts(&requests[1]);
    assert!(
        second.iter().any(|text| {
            text.contains("以下 AGENTS.md 指令取代先前提供的全部 AGENTS.md 指令。")
                && text.contains("instruction-v2")
        }),
        "同一 Turn 内的下一次 Model Call 应带取代声明重发新的 AGENTS.md"
    );
}

#[tokio::test]
async fn invalid_project_instructions_fail_before_model_and_leave_no_draft() {
    let workspace = TestWorkspace::new();
    workspace.write_instructions(vec![b'x'; 64 * 1024 + 1]);
    let mut fixture = runtime_in_workspace(
        vec![response("recovered", Vec::new())],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        workspace,
    );

    start_with_request(&fixture, "project-instructions-invalid").await;
    let failed = wait_for_terminal(&mut fixture.updates).await;
    assert!(matches!(
        failed,
        TurnOutcome::Failed { ref code, .. } if code == "project_instruction_error"
    ));
    assert!(fixture.model.requests.lock().unwrap().is_empty());
    assert!(
        fixture
            .chat
            .snapshot()
            .await
            .expect("chat snapshot")
            .draft
            .is_none()
    );

    fixture.workspace.write_instructions("valid instruction");
    start_with_request(&fixture, "project-instructions-recovered").await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    // 修好之后，规范以带标记的 User 消息重新进入 Conversation，而不再是 System 前缀。
    assert!(
        user_message_texts(&requests[0])
            .iter()
            .any(|text| text.contains("<project_instructions>")
                && text.contains("valid instruction"))
    );
}

#[tokio::test]
async fn tool_result_is_in_the_next_model_request() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "write", r#"{"path":"README.md"}"#)],
            ),
            response("final", Vec::new()),
        ],
        vec![ToolResult::succeeded("file contents")],
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    let outcome = wait_for_terminal(&mut fixture.updates).await;

    assert!(matches!(outcome, TurnOutcome::Completed { .. }));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.role == Role::Tool)
    );
    assert_eq!(
        events_ignoring_world_state(&fixture),
        [
            "begin_turn",
            "model_1",
            "assistant",
            "tool_result",
            "model_2",
            "assistant",
            "finish_turn"
        ]
    );
}

#[tokio::test]
async fn agent_message_is_persisted_after_tool_results_and_seen_by_the_same_turn() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "write", r#"{"path":"README.md"}"#)],
            ),
            response("final", Vec::new()),
        ],
        vec![ToolResult::succeeded("file contents")],
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;

    fixture
        .handle
        .deliver_agent_message(
            SessionId::new("session-find-auth-child"),
            TurnId::new("turn-find-auth-child"),
            "find_auth_flow",
            AgentMessageKind::FinalAnswer,
            "Authentication is implemented in crates/api/src/auth.rs.",
        )
        .await
        .expect("agent message delivered");
    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::AllowOnce)
        .await
        .expect("permission resolved");

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let delivered = requests[1].messages.last().expect("last request message");
    assert_eq!(delivered.role, Role::User);
    assert_eq!(
        delivered.content,
        [ContentBlock::text(
            "<agent_message>\n<task>find_auth_flow</task>\n<kind>final_answer</kind>\n<body>\nAuthentication is implemented in crates/api/src/auth.rs.\n</body>\n</agent_message>"
        )]
    );
    assert_eq!(
        events_ignoring_world_state(&fixture),
        [
            "begin_turn",
            "model_1",
            "assistant",
            "tool_result",
            "agent_message",
            "model_2",
            "assistant",
            "finish_turn"
        ]
    );
}

#[tokio::test]
async fn duplicate_agent_delivery_is_persisted_and_appended_to_chat_once() {
    let mut fixture = runtime(
        vec![response("parent saw one result", Vec::new())],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    let child_session_id = SessionId::new("session-idempotent-child");
    let child_turn_id = TurnId::new("turn-idempotent-child");

    for _ in 0..2 {
        fixture
            .handle
            .deliver_agent_message(
                child_session_id.clone(),
                child_turn_id.clone(),
                "find_auth_flow",
                AgentMessageKind::FinalAnswer,
                "Authentication is implemented in auth.rs.",
            )
            .await
            .expect("duplicate delivery enters the mailbox");
    }

    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let requests = fixture.model.requests.lock().unwrap();
    let delivered = requests[0]
        .messages
        .iter()
        .filter(|message| {
            matches!(
                message.content.first(),
                Some(ContentBlock::Text(text))
                    if text.text.contains("<task>find_auth_flow</task>")
            )
        })
        .count();
    assert_eq!(delivered, 1);
    assert_eq!(
        fixture
            .storage
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.as_str() == "agent_message")
            .count(),
        1
    );
}

#[tokio::test]
async fn an_idle_parent_does_not_start_a_turn_and_consumes_mail_on_the_next_user_turn() {
    let mut fixture = runtime(
        vec![
            response("first turn done", Vec::new()),
            response("second turn done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start_with_request(&fixture, "first-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    fixture
        .handle
        .deliver_agent_message(
            SessionId::new("session-late-result-child"),
            TurnId::new("turn-late-result-child"),
            "late_result",
            AgentMessageKind::FinalAnswer,
            "Arrived after the final answer.",
        )
        .await
        .expect("agent message delivered");
    // Snapshot 与投递走同一条命令通道；它返回就证明 actor 已处理消息，期间没有创建 Turn。
    let snapshot = fixture.handle.snapshot().await.expect("snapshot");
    assert!(matches!(
        snapshot.runtime,
        openwork_core::session::SessionRuntimeSnapshot::Terminal { .. }
    ));
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);

    start_with_request(&fixture, "second-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].messages.last().expect("mail message").role,
        Role::User
    );
    assert!(matches!(
        requests[1].messages.last().expect("mail message").content.first(),
        Some(ContentBlock::Text(text))
            if text.text.contains("<task>late_result</task>")
                && text.text.contains("Arrived after the final answer.")
    ));
}

#[tokio::test]
async fn a_child_terminal_answer_is_delivered_to_its_parent_session() {
    let parent_session_id = SessionId::new("session-parent");
    let host = Arc::new(SessionHandleHost::default());
    let agent_control = AgentControl::new(
        parent_session_id.clone(),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut parent = runtime_with_options(
        vec![Ok(response("parent consumed result", Vec::new()))],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: parent_session_id.clone(),
            ..RuntimeOptions::default()
        },
    );
    host.insert(parent.handle.clone());
    let mut child = runtime_with_options(
        vec![Ok(response("Authentication lives in auth.rs.", Vec::new()))],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-child"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::NonInteractive,
            parent_link: Some(ParentLink {
                parent_session_id,
                task_name: "find_auth_flow".to_string(),
                agent_control,
            }),
            agent_control: None,
        },
    );

    start_with_request(&child, "child-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut child.updates).await,
        TurnOutcome::Completed { .. }
    ));
    child
        .handle
        .snapshot()
        .await
        .expect("child delivery barrier");
    parent
        .handle
        .snapshot()
        .await
        .expect("parent mailbox barrier");

    start_with_request(&parent, "parent-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut parent.updates).await,
        TurnOutcome::Completed { .. }
    ));
    let requests = parent.model.requests.lock().unwrap();
    assert!(requests[0].messages.iter().any(|message| {
        matches!(
            message.content.first(),
            Some(ContentBlock::Text(text))
                if text.text.contains("<task>find_auth_flow</task>")
                    && text.text.contains("<kind>final_answer</kind>")
                    && text.text.contains("Authentication lives in auth.rs.")
        )
    }));
}

#[tokio::test]
async fn failed_and_cancelled_children_both_notify_the_parent() {
    let parent_session_id = SessionId::new("session-parent-terminal-errors");
    let host = Arc::new(SessionHandleHost::default());
    let agent_control = AgentControl::new(
        parent_session_id.clone(),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut parent = runtime_with_options(
        vec![Ok(response("parent handled failures", Vec::new()))],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: parent_session_id.clone(),
            ..RuntimeOptions::default()
        },
    );
    host.insert(parent.handle.clone());

    let mut failed_child = runtime_with_options(
        vec![Err(ModelError::protocol("child model failed"))],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-failed-child"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::NonInteractive,
            parent_link: Some(ParentLink {
                parent_session_id: parent_session_id.clone(),
                task_name: "failed_lookup".to_string(),
                agent_control: agent_control.clone(),
            }),
            agent_control: None,
        },
    );
    start_with_request(&failed_child, "failed-child-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut failed_child.updates).await,
        TurnOutcome::Failed { .. }
    ));
    failed_child
        .handle
        .snapshot()
        .await
        .expect("failed child delivery barrier");

    let mut cancelled_child = runtime_with_options(
        vec![Ok(response(
            "",
            vec![tool_call("call-wait", "read", r#"{"waitForCancel":true}"#)],
        ))],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-cancelled-child"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::NonInteractive,
            parent_link: Some(ParentLink {
                parent_session_id,
                task_name: "cancelled_lookup".to_string(),
                agent_control,
            }),
            agent_control: None,
        },
    );
    let cancelled_turn = start_with_request(&cancelled_child, "cancelled-child-turn").await;
    wait_for_tool_start(&mut cancelled_child.updates).await;
    cancelled_child
        .handle
        .cancel_turn(cancelled_turn)
        .await
        .expect("cancel child turn");
    assert_eq!(
        wait_for_terminal(&mut cancelled_child.updates).await,
        TurnOutcome::Cancelled
    );
    cancelled_child
        .handle
        .snapshot()
        .await
        .expect("cancelled child delivery barrier");
    parent
        .handle
        .snapshot()
        .await
        .expect("parent mailbox barrier");

    start_with_request(&parent, "parent-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut parent.updates).await,
        TurnOutcome::Completed { .. }
    ));
    let request = &parent.model.requests.lock().unwrap()[0];
    let delivered = request
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::Text(text)) if text.text.contains("<agent_message>") => {
                Some(text.text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 2);
    assert!(delivered.iter().any(|text| {
        text.contains("<task>failed_lookup</task>") && text.contains("<kind>failed</kind>")
    }));
    assert!(delivered.iter().any(|text| {
        text.contains("<task>cancelled_lookup</task>") && text.contains("<kind>interrupted</kind>")
    }));
}

#[tokio::test]
async fn a_child_still_completes_when_its_parent_can_no_longer_be_reached() {
    // A parent that is gone is not the child's problem: the child must finish
    // normally rather than fail or hang. The failure is reported through
    // `tracing` instead, so it is diagnosable without being fatal.
    let host = Arc::new(SessionHandleHost::default());
    let orphan_parent_id = SessionId::new("session-parent-never-registered");
    let agent_control = AgentControl::new(
        orphan_parent_id.clone(),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut child = runtime_with_options(
        vec![Ok(response("Answer nobody will read.", Vec::new()))],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-orphan-child"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::NonInteractive,
            parent_link: Some(ParentLink {
                parent_session_id: orphan_parent_id,
                task_name: "orphan_lookup".to_string(),
                agent_control,
            }),
            agent_control: None,
        },
    );

    start_with_request(&child, "orphan-child-turn").await;
    assert!(matches!(
        wait_for_terminal(&mut child.updates).await,
        TurnOutcome::Completed { .. }
    ));
    // The actor must still be responsive after the failed delivery.
    child
        .handle
        .snapshot()
        .await
        .expect("the child actor survives an undeliverable result");
}

#[tokio::test]
async fn tool_result_artifacts_are_persisted_in_messages_and_forwarded_live() {
    let artifact = ToolResultArtifact {
        kind: "file_change".to_string(),
        payload: serde_json::json!({"changeId": "change-1", "path": "README.md"}),
    };
    let mut result = ToolResult::succeeded("edited README.md");
    result.artifacts.push(artifact.clone());
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-artifact",
                    "write",
                    r#"{"path":"README.md","content":"new"}"#,
                )],
            ),
            response("final", Vec::new()),
        ],
        vec![result],
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;

    let live_artifacts = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = fixture.updates.recv().await.expect("session update");
            if let SessionUpdate::ToolCallFinished { artifacts, .. } = event.update {
                break artifacts;
            }
        }
    })
    .await
    .expect("artifact update timed out");
    assert_eq!(live_artifacts.as_slice(), std::slice::from_ref(&artifact));

    let _ = wait_for_terminal(&mut fixture.updates).await;
    let requests = fixture.model.requests.lock().unwrap();
    let persisted_artifacts = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .find_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result.artifacts.as_slice()),
            _ => None,
        })
        .expect("tool result in next request");
    assert_eq!(persisted_artifacts, [artifact]);
}

#[tokio::test]
async fn tool_progress_is_forwarded_before_the_terminal_tool_update() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-progress",
                    "read",
                    r#"{"emitProgress":"scanning"}"#,
                )],
            ),
            response("final", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;

    let (progress_sequence, finished_sequence) =
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut progress_sequence = None;
            loop {
                let event = fixture.updates.recv().await.expect("session update");
                match event.update {
                    SessionUpdate::ToolCallProgress {
                        progress: ToolProgressUpdate::Message { message },
                        ..
                    } => {
                        assert_eq!(message, "scanning");
                        progress_sequence = Some(event.sequence);
                    }
                    SessionUpdate::ToolCallFinished { .. } => {
                        break (progress_sequence.expect("progress update"), event.sequence);
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("tool updates timed out");

    assert!(progress_sequence < finished_sequence);
}

#[tokio::test]
async fn cancelling_a_turn_cancels_the_active_tool_call() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "read", r#"{"waitForCancel":true}"#)],
        )],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    let turn_id = start(&fixture).await;
    wait_for_tool_start(&mut fixture.updates).await;
    assert!(
        fixture
            .handle
            .cancel_turn(turn_id)
            .await
            .expect("cancel turn")
    );

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Cancelled
    ));
}

#[tokio::test]
async fn unknown_tool_becomes_a_result_and_the_model_continues() {
    let mut fixture = runtime(
        vec![
            response("", vec![tool_call("call-1", "missing", "{}")]),
            response("recovered", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn permission_allow_executes_the_tool_and_finishes() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "write", r#"{"path":"README.md"}"#)],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::AllowOnce)
        .await
        .expect("permission");

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn acc_54_62_69_session_exec_grant_unblocks_later_calls_only_in_that_actor() {
    let command = r#"{"program":"cargo","args":["test","-p","openwork-tools"]}"#;
    let mut fixture = runtime(
        vec![
            response("", vec![tool_call("call-1", "bash", command)]),
            response("", vec![tool_call("call-2", "bash", command)]),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    let snapshot = fixture
        .handle
        .snapshot()
        .await
        .expect("permission snapshot");
    let openwork_core::session::SessionRuntimeSnapshot::Running {
        pending_permission: Some(request),
        ..
    } = snapshot.runtime
    else {
        panic!("permission request must stay visible")
    };
    assert!(matches!(
        request.card.session_action,
        Some(ApprovalSessionAction::AllowExec { .. })
    ));

    fixture
        .handle
        .resolve_permission(
            turn_id,
            tool_call_id.clone(),
            PermissionDecision::AllowSession,
        )
        .await
        .expect("install session grant");
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 2);
    let expected_rule_id = format!("session.{}.0", tool_call_id.as_str());
    {
        let signals = fixture.trace.signals.lock().unwrap();
        let granted = signals
            .iter()
            .find_map(|signal| match signal {
                TraceSignal::ToolCallFinished(finished)
                    if finished.attributes.permission_decision_source.as_deref()
                        == Some("session_grant") =>
                {
                    Some(finished)
                }
                _ => None,
            })
            .expect("session-granted tool trace");
        assert_eq!(
            granted.attributes.permission_rule_id.as_deref(),
            Some(expected_rule_id.as_str())
        );
        assert_eq!(
            granted.attributes.permission_rule_scope.as_deref(),
            Some("session")
        );
        assert_eq!(
            granted.attributes.permission_mode.as_deref(),
            Some("default")
        );
        assert_eq!(
            granted.attributes.permission_mode_origin.as_deref(),
            Some("session_default")
        );
    }

    let mut fresh = runtime(
        vec![response("", vec![tool_call("call-1", "bash", command)])],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fresh).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fresh.updates).await;
    fresh
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Deny)
        .await
        .expect("deny fresh-session request");
    assert!(matches!(
        wait_for_terminal(&mut fresh.updates).await,
        TurnOutcome::Failed { code, .. } if code == "permission_denied"
    ));
}

#[tokio::test]
async fn approval_client_cannot_invent_a_session_action() {
    let command = r#"{"program":"cargo","args":["test"]}"#;
    let mut fixture = runtime(
        vec![
            response("", vec![tool_call("call-1", "bash", command)]),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;

    let unavailable = fixture
        .handle
        .resolve_permission(
            turn_id.clone(),
            tool_call_id.clone(),
            PermissionDecision::AcceptEdits,
        )
        .await;
    assert!(matches!(
        unavailable,
        Err(SessionError::PermissionDecisionUnavailable(id)) if id == tool_call_id
    ));
    assert!(matches!(
        fixture
            .handle
            .snapshot()
            .await
            .expect("pending snapshot")
            .runtime,
        openwork_core::session::SessionRuntimeSnapshot::Running {
            pending_permission: Some(_),
            ..
        }
    ));

    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::AllowSession)
        .await
        .expect("use the server-offered session action");
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
}

#[tokio::test]
async fn acc_59_61_62_69_card_mode_change_reuses_the_session_mode_state() {
    let mut fixture = runtime(
        vec![
            response("", vec![tool_call("call-1", "write", r#"{"path":"a.rs"}"#)]),
            response("", vec![tool_call("call-2", "write", r#"{"path":"b.rs"}"#)]),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    let snapshot = fixture
        .handle
        .snapshot()
        .await
        .expect("permission snapshot");
    let openwork_core::session::SessionRuntimeSnapshot::Running {
        pending_permission: Some(request),
        ..
    } = snapshot.runtime
    else {
        panic!("permission request must stay visible")
    };
    assert_eq!(
        request.card.session_action,
        Some(ApprovalSessionAction::EnableAcceptEdits)
    );

    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::AcceptEdits)
        .await
        .expect("switch mode from approval card");
    assert_eq!(
        fixture
            .handle
            .snapshot()
            .await
            .expect("updated snapshot")
            .permission_mode,
        PermissionMode::AcceptEdits
    );
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 2);
    {
        let signals = fixture.trace.signals.lock().unwrap();
        let automatic = signals
            .iter()
            .find_map(|signal| match signal {
                // The second write ran because the card switched the mode, so its
                // source must be `mode` — not `builtin`, which would claim it was
                // allowed all along (permissions.md §7).
                TraceSignal::ToolCallFinished(finished)
                    if finished.attributes.permission_decision_source.as_deref()
                        == Some("mode") =>
                {
                    Some(finished)
                }
                _ => None,
            })
            .expect("second write trace");
        assert_eq!(
            automatic.attributes.permission_mode.as_deref(),
            Some("accept_edits")
        );
        assert_eq!(
            automatic.attributes.permission_mode_origin.as_deref(),
            Some("approval_card")
        );
    }

    let fresh = runtime(
        vec![response("done", Vec::new())],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    assert_eq!(
        fresh
            .handle
            .snapshot()
            .await
            .expect("fresh actor snapshot")
            .permission_mode,
        PermissionMode::Default
    );
}

#[tokio::test]
async fn permission_deny_writes_a_tool_result_without_execution() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "bash", r#"{"command":"pwd"}"#)],
        )],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Deny)
        .await
        .expect("permission");

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Failed { code, .. } if code == "permission_denied"
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    let snapshot = fixture.chat.snapshot().await.expect("chat snapshot");
    assert!(
        snapshot
            .messages
            .iter()
            .any(|message| message.role == Role::Tool)
    );
}

#[tokio::test]
async fn acc_57_rule_deny_returns_a_tool_result_and_the_turn_continues() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "write", r#"{"path":".git/config"}"#)],
            ),
            response("continued", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { final_text } if final_text == "continued"
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
    let snapshot = fixture.chat.snapshot().await.expect("chat snapshot");
    assert!(snapshot.messages.iter().any(|message| {
        message.role == Role::Tool
            && message.content.iter().any(|block| {
                matches!(block, ContentBlock::ToolResult(result) if result.state == ToolResultState::Denied)
            })
    }));
}

#[tokio::test]
async fn assistant_persistence_failure_prevents_tool_execution() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "read", r#"{"path":"README.md"}"#)],
        )],
        Vec::new(),
        PermissionMode::AcceptEdits,
        true,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Failed { code, .. } if code == "persistence_error"
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn tool_failure_is_returned_to_the_model_instead_of_stopping_the_loop() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "read", r#"{"path":"missing.txt"}"#)],
            ),
            response("used another approach", Vec::new()),
        ],
        vec![ToolResult::failed(
            openwork_tools::ToolErrorCode::ExecutionFailed,
            "not found",
            false,
        )],
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn multiple_tool_results_keep_provider_order_in_the_next_request() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![
                    tool_call("call-1", "read", r#"{"path":"a"}"#),
                    tool_call("call-2", "read", r#"{"path":"b"}"#),
                ],
            ),
            response("done", Vec::new()),
        ],
        vec![ToolResult::succeeded("a"), ToolResult::succeeded("b")],
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;

    let requests = fixture.model.requests.lock().unwrap();
    let tool_ids: Vec<_> = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(tool_ids, ["call-1", "call-2"]);
}

#[tokio::test]
async fn acc_58_multiple_permission_requests_are_presented_serially() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![
                    tool_call("call-1", "bash", r#"{"command":"cargo test"}"#),
                    tool_call("call-2", "bash", r#"{"command":"cargo clippy"}"#),
                ],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;

    let (turn_id, first_tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    let snapshot = fixture.handle.snapshot().await.expect("waiting snapshot");
    assert!(matches!(
        snapshot.runtime,
        openwork_core::session::SessionRuntimeSnapshot::Running {
            pending_permission: Some(request),
            ..
        } if request.tool_call_id == first_tool_call_id
    ));
    fixture
        .handle
        .resolve_permission(
            turn_id.clone(),
            first_tool_call_id.clone(),
            PermissionDecision::AllowOnce,
        )
        .await
        .expect("first permission");

    let (second_turn_id, second_tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    assert_eq!(second_turn_id, turn_id);
    assert_ne!(second_tool_call_id, first_tool_call_id);
    fixture
        .handle
        .resolve_permission(
            second_turn_id,
            second_tool_call_id,
            PermissionDecision::AllowOnce,
        )
        .await
        .expect("second permission");

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn duplicate_client_request_is_idempotent_and_a_different_turn_is_busy() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "write", r#"{"path":"a"}"#)],
        )],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    let first_turn = start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;

    let duplicate = fixture
        .handle
        .start_turn(
            ClientRequestId::new("client-request"),
            openwork_core::session::PreparedTurnInput::text("same retry"),
            BTreeSet::new(),
        )
        .await
        .expect("idempotent retry");
    assert_eq!(duplicate.turn_id, first_turn);

    let busy = fixture
        .handle
        .start_turn(
            ClientRequestId::new("another-request"),
            openwork_core::session::PreparedTurnInput::text("overlap"),
            BTreeSet::new(),
        )
        .await;
    assert!(matches!(busy, Err(SessionError::Busy(id)) if id == first_turn));

    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Deny)
        .await
        .expect("cleanup permission");
    wait_for_terminal(&mut fixture.updates).await;
}

#[tokio::test]
async fn manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection() {
    let mut fixture = runtime(
        vec![
            response("first answer", Vec::new()),
            response(compaction_summary(), Vec::new()),
            response("continued", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;

    let compaction = fixture
        .handle
        .compact_conversation(BTreeSet::new())
        .await
        .expect("compaction");

    // 首次 Model Call 之前追加了项目上下文，因此压缩源是
    // world-state + user + assistant = 3。
    assert_eq!(compaction.source_message_count, 3);
    assert_eq!(compaction.summary, compaction_summary());
    {
        let signals = fixture.trace.signals.lock().unwrap();
        let compact_trace = signals
            .iter()
            .find_map(|signal| match signal {
                TraceSignal::CompactionFinished(finished) => Some(finished),
                _ => None,
            })
            .expect("manual compaction trace");
        assert_eq!(compact_trace.started.session_id.as_str(), "session-test");
        assert!(compact_trace.started.turn_id.is_none());
        assert_eq!(compact_trace.status, TraceStatus::Succeeded);
        assert_eq!(compact_trace.attempt_count, Some(1));
        assert_eq!(compact_trace.attributes.trigger, "manual");
        assert_eq!(compact_trace.attributes.source_message_count, Some(3));
        assert!(compact_trace.attributes.prepare_ms.is_some());
        assert_eq!(
            compact_trace.attributes.summary_max_output_tokens,
            Some(16_384)
        );
        assert_eq!(
            compact_trace
                .attributes
                .summary_estimated_tool_surface_tokens,
            Some(0)
        );
        assert!(
            compact_trace
                .attributes
                .summary_estimated_system_context_tokens
                .is_some()
        );
        assert!(
            compact_trace
                .attributes
                .summary_estimated_conversation_tokens
                .is_some()
        );
        assert_eq!(
            compact_trace.attributes.checkpoint_id.as_deref(),
            Some(compaction.id.as_str())
        );
        let summary_trace = signals
            .iter()
            .find_map(|signal| match signal {
                TraceSignal::ModelCallFinished(finished)
                    if finished.started.parent_span_id.as_deref()
                        == Some(compact_trace.started.span_id.as_str()) =>
                {
                    Some(finished)
                }
                _ => None,
            })
            .expect("summary child model trace");
        assert_eq!(
            summary_trace.started.trace_id,
            compact_trace.started.trace_id
        );
        assert!(summary_trace.started.turn_id.is_none());
        assert_eq!(summary_trace.status, TraceStatus::Succeeded);
        assert_eq!(
            summary_trace.attributes.thinking_mode.as_deref(),
            Some("disabled")
        );
        assert!(summary_trace.started.payloads.request.is_some());
        assert!(summary_trace.response_payload.is_some());
        assert!(summary_trace.response_message_id.is_none());
        assert_eq!(
            summary_trace.provider_request_id.as_deref(),
            Some("request")
        );
        assert!(
            serde_json::to_value(&summary_trace.attributes)
                .expect("summary attributes serialize")
                .get("summaryAttemptOutcome")
                .is_none()
        );
        assert!(
            serde_json::to_value(&compact_trace.attributes)
                .expect("compaction attributes serialize")
                .get("attemptRollup")
                .is_none()
        );
        // A manual compaction is not measured against a window, so no policy or
        // trigger estimate is attributed to it — but the reclaim still is.
        assert_eq!(compact_trace.attributes.context_window_tokens, None);
        assert_eq!(
            compact_trace.attributes.trigger_estimated_input_tokens,
            None
        );
        assert!(!compact_trace.started.trace_id.is_empty());
        let before = compact_trace
            .attributes
            .conversation_tokens_before
            .expect("pre-compaction conversation estimate");
        let after = compact_trace
            .attributes
            .conversation_tokens_after
            .expect("post-install conversation estimate");
        assert_eq!(
            compact_trace.attributes.reclaimed_conversation_tokens,
            Some(before.saturating_sub(after))
        );
    }
    let snapshot = fixture.chat.snapshot().await.expect("chat snapshot");
    assert_eq!(snapshot.messages.len(), 3);
    assert_eq!(snapshot.messages[0].role, Role::User);
    let ContentBlock::Text(last_user_block) = &snapshot.messages[0].content[0] else {
        panic!("expected replayed user text")
    };
    assert_eq!(last_user_block.text, "do the task");
    let ContentBlock::Text(summary_block) = &snapshot.messages[1].content[0] else {
        panic!("expected compacted summary text")
    };
    assert!(
        summary_block
            .text
            .contains("<conversation_summary format_version=\"1\">")
    );
    assert!(summary_block.text.contains(compaction_summary()));

    {
        let requests = fixture.model.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            roles_ignoring_world_state(&requests[1]),
            [Role::System, Role::User, Role::Assistant, Role::User]
        );
        assert!(requests[1].tools.is_empty());
        assert_eq!(requests[1].max_output_tokens, Some(16_384));
        assert_eq!(requests[1].thinking, Some(ThinkingConfig::disabled()));
        let ContentBlock::Text(prompt) = requests[1]
            .messages
            .last()
            .unwrap()
            .content
            .first()
            .unwrap()
        else {
            panic!("expected compaction prompt")
        };
        assert!(prompt.text.contains("durable continuation summary"));
    }

    start_with_request(&fixture, "continue-after-compaction").await;
    wait_for_terminal(&mut fixture.updates).await;
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(
        roles_ignoring_world_state(&requests[2]),
        [Role::System, Role::User, Role::User, Role::User, Role::User]
    );
    // 按标记查找而不是按下标：world-state 消息会改变位置，绑定下标的断言每次
    // 上下文结构调整都要跟着改，而且改错了也不会有人发现。
    assert!(
        user_message_texts(&requests[2])
            .iter()
            .any(|text| text.contains(compaction_summary())),
        "压缩摘要应出现在投影里"
    );
}

#[tokio::test]
async fn manual_compaction_rematerializes_the_same_explicit_user_skill_catalog() {
    let workspace = TestWorkspace::new();
    let user_root = workspace.path().join("user-skills");
    let skill_path = workspace.write_skill(&user_root, "review", "Review changes when requested.");
    let mut fixture = runtime_in_workspace_with_skill_roots(
        vec![
            response("first answer", Vec::new()),
            response(compaction_summary(), Vec::new()),
        ],
        PermissionMode::AcceptEdits,
        workspace,
        SkillRoots {
            agents: Some(user_root),
        },
    );

    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;
    fixture
        .handle
        .compact_conversation(BTreeSet::new())
        .await
        .expect("compaction");

    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let catalog_text = |request: &ModelRequest| {
        request
            .messages
            .iter()
            .filter(|message| message.role == Role::User)
            .find_map(|message| match message.content.first() {
                Some(ContentBlock::Text(text)) if text.text.contains("<available_skills>") => {
                    Some(text.text.clone())
                }
                _ => None,
            })
            .expect("skill catalog")
    };
    let turn_catalog = catalog_text(&requests[0]);
    let compaction_catalog = catalog_text(&requests[1]);
    assert_eq!(turn_catalog, compaction_catalog);
    assert!(turn_catalog.contains(skill_path.to_str().expect("UTF-8 path")));
}

#[tokio::test]
async fn session_handle_applies_disabled_skills_to_turn_and_manual_compaction() {
    let workspace = TestWorkspace::new();
    let user_root = workspace.path().join("user-skills");
    workspace.write_skill(&user_root, "review", "Review changes when requested.");
    let mut fixture = runtime_in_workspace_with_skill_roots(
        vec![
            response("first answer", Vec::new()),
            response(compaction_summary(), Vec::new()),
        ],
        PermissionMode::AcceptEdits,
        workspace,
        SkillRoots {
            agents: Some(user_root),
        },
    );
    let disabled_skill_names = BTreeSet::from(["review".to_string()]);

    fixture
        .handle
        .start_turn(
            ClientRequestId::new("disabled-skill-turn"),
            openwork_core::session::PreparedTurnInput::text("do the task"),
            disabled_skill_names.clone(),
        )
        .await
        .expect("turn accepted");
    wait_for_terminal(&mut fixture.updates).await;
    fixture
        .handle
        .compact_conversation(disabled_skill_names)
        .await
        .expect("compaction");

    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| {
        request.messages.iter().all(|message| {
            message.content.iter().all(|block| match block {
                ContentBlock::Text(text) => !text.text.contains("<available_skills>"),
                _ => true,
            })
        })
    }));
}

#[tokio::test]
async fn context_overflow_compacts_and_resubmits_once_in_the_same_turn() {
    let mut fixture = runtime_with_outcomes(
        vec![
            Err(ModelError::context_overflow(
                "input exceeds the model context window",
            )),
            Ok(response(compaction_summary(), Vec::new())),
            Ok(response("recovered after compaction", Vec::new())),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );

    let turn_id = start(&fixture).await;
    assert_eq!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed {
            final_text: "recovered after compaction".to_string()
        }
    );

    let compactions = fixture.storage.compactions.lock().unwrap();
    assert_eq!(compactions.len(), 1);
    assert_eq!(compactions[0].kind, ConversationCompactionKind::Overflow);
    assert_eq!(
        compactions[0].trigger_turn_id.as_deref(),
        Some(turn_id.as_str())
    );
    drop(compactions);

    let attempt_ids = fixture.model.model_attempt_ids.lock().unwrap();
    assert_eq!(attempt_ids.len(), 3);
    assert_ne!(attempt_ids[0], attempt_ids[2]);
    assert!(attempt_ids[0].ends_with("submission-1"));
    assert!(attempt_ids[2].ends_with("submission-2"));
    drop(attempt_ids);

    assert_eq!(
        *fixture.storage.model_submissions.lock().unwrap(),
        [(1, 1), (1, 2)]
    );

    let signals = fixture.trace.signals.lock().unwrap();
    let finished_kinds = signals
        .iter()
        .filter_map(|signal| match signal {
            TraceSignal::ModelCallFinished(finished) => {
                assert_eq!(finished.started.trace_id, turn_id.as_str());
                Some(if finished.started.parent_span_id.is_some() {
                    "summary"
                } else {
                    "model"
                })
            }
            TraceSignal::CompactionFinished(finished) => {
                assert_eq!(finished.attributes.trigger, "overflow");
                assert_eq!(finished.started.turn_id.as_ref(), Some(&turn_id));
                assert_eq!(finished.started.trace_id, turn_id.as_str());
                Some("compaction")
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished_kinds, ["model", "summary", "compaction", "model"]);
    drop(signals);

    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].tools.is_empty());
    assert!(requests[1].tools.is_empty());
    assert!(!requests[2].tools.is_empty());
    assert_eq!(
        roles_ignoring_world_state(&requests[2]),
        [Role::System, Role::User, Role::User, Role::User]
    );
}

#[tokio::test]
async fn context_budget_threshold_compacts_before_the_first_provider_submission() {
    let mut fixture = runtime_with_capabilities(
        vec![
            response(compaction_summary(), Vec::new()),
            response("continued after threshold compaction", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        test_capabilities(2, 1),
    );
    let accepted = fixture
        .handle
        .start_turn(
            ClientRequestId::new("threshold-request"),
            openwork_core::session::PreparedTurnInput::text("do the task"),
            BTreeSet::new(),
        )
        .await
        .expect("turn accepted");
    assert_eq!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed {
            final_text: "continued after threshold compaction".to_string()
        }
    );

    let compactions = fixture.storage.compactions.lock().unwrap();
    assert_eq!(compactions.len(), 1);
    assert_eq!(compactions[0].kind, ConversationCompactionKind::Threshold);
    assert_eq!(
        compactions[0].trigger_turn_id.as_deref(),
        Some(accepted.turn_id.as_str())
    );
    drop(compactions);

    assert_eq!(*fixture.storage.model_submissions.lock().unwrap(), [(1, 1)]);
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].tools.is_empty());
    assert!(!requests[1].tools.is_empty());
    let post_compaction_messages = requests[1].messages.clone();
    drop(requests);

    let signals = fixture.trace.signals.lock().unwrap();
    let finished_kinds = signals
        .iter()
        .filter_map(|signal| match signal {
            TraceSignal::CompactionFinished(finished) => {
                assert_eq!(finished.attributes.trigger, "threshold");
                assert_eq!(finished.started.turn_id.as_ref(), Some(&accepted.turn_id));
                assert_eq!(finished.started.trace_id, accepted.turn_id.as_str());
                Some("compaction")
            }
            TraceSignal::ModelCallFinished(finished) => {
                assert_eq!(finished.started.trace_id, accepted.turn_id.as_str());
                Some(if finished.started.parent_span_id.is_some() {
                    "summary"
                } else {
                    "model"
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished_kinds, ["summary", "compaction", "model"]);
    let resumed_model = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ModelCallFinished(finished)
                if finished.started.parent_span_id.is_none() =>
            {
                Some(finished)
            }
            _ => None,
        })
        .expect("post-compaction model trace");
    assert_eq!(
        resumed_model.started.payloads.request,
        Some(serde_json::to_value(post_compaction_messages).unwrap())
    );
}

#[tokio::test]
async fn compacted_tool_turn_records_the_seven_documented_spans() {
    let mut fixture = runtime_with_capabilities(
        vec![
            response(compaction_summary(), Vec::new()),
            response(
                "",
                vec![
                    tool_call("call-1", "read", r#"{"path":"README.md"}"#),
                    tool_call("call-2", "glob", r#"{"pattern":"docs/*.md"}"#),
                    tool_call("call-3", "grep", r#"{"query":"trace_id"}"#),
                ],
            ),
            response("done after tools", Vec::new()),
        ],
        vec![
            ToolResult::succeeded("readme"),
            ToolResult::succeeded("agents"),
            ToolResult::succeeded("trace docs"),
        ],
        PermissionMode::AcceptEdits,
        false,
        test_capabilities(10_000, 1),
    );
    fixture
        .chat
        .append_user(vec![ContentBlock::text("x".repeat(50_000))])
        .await
        .expect("large prior user message");
    fixture
        .chat
        .append_assistant(Message::text(Role::Assistant, "prior work completed"))
        .await
        .expect("prior assistant message");

    let accepted = fixture
        .handle
        .start_turn(
            ClientRequestId::new("seven-span-threshold-request"),
            openwork_core::session::PreparedTurnInput::text("inspect the project"),
            BTreeSet::new(),
        )
        .await
        .expect("turn accepted");
    assert_eq!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed {
            final_text: "done after tools".to_string()
        }
    );

    let signals = fixture.trace.signals.lock().unwrap();
    let finished_kinds = signals
        .iter()
        .filter_map(|signal| match signal {
            TraceSignal::CompactionFinished(finished) => {
                assert_eq!(finished.started.trace_id, accepted.turn_id.as_str());
                Some("compaction")
            }
            TraceSignal::ModelCallFinished(finished) => {
                assert_eq!(finished.started.trace_id, accepted.turn_id.as_str());
                Some(if finished.started.parent_span_id.is_some() {
                    "summary"
                } else {
                    "model"
                })
            }
            TraceSignal::ToolCallFinished(finished) => {
                assert_eq!(finished.started.trace_id, accepted.turn_id.as_str());
                Some("tool")
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished_kinds.len(), 7);
    assert_eq!(
        finished_kinds,
        [
            "summary",
            "compaction",
            "model",
            "tool",
            "tool",
            "tool",
            "model"
        ]
    );
    assert_eq!(
        *fixture.storage.model_submissions.lock().unwrap(),
        [(1, 1), (2, 1)]
    );
}

#[tokio::test]
async fn threshold_compaction_and_overflow_recovery_share_one_compaction_budget() {
    let mut fixture = runtime_with_outcomes_and_capabilities(
        vec![
            Ok(response(compaction_summary(), Vec::new())),
            Err(ModelError::context_overflow(
                "provider still rejected the compacted request",
            )),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        test_capabilities(2, 1),
    );

    fixture
        .handle
        .start_turn(
            ClientRequestId::new("threshold-overflow-request"),
            openwork_core::session::PreparedTurnInput::text("do the task"),
            BTreeSet::new(),
        )
        .await
        .expect("turn accepted");
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Failed { .. }
    ));

    assert_eq!(fixture.storage.compactions.lock().unwrap().len(), 1);
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
    assert_eq!(*fixture.storage.model_submissions.lock().unwrap(), [(1, 1)]);
}

#[tokio::test]
async fn manual_compaction_is_rejected_while_a_turn_is_active() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "write", r#"{"path":"a"}"#)],
        )],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    let active_turn = start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;

    assert!(matches!(
        fixture
            .handle
            .compact_conversation(BTreeSet::new())
            .await,
        Err(CompactionError::SessionActive(id)) if id == active_turn
    ));

    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Deny)
        .await
        .expect("cleanup permission");
    wait_for_terminal(&mut fixture.updates).await;
}

#[tokio::test]
async fn failed_compaction_persistence_keeps_the_previous_conversation() {
    let mut fixture = runtime(
        vec![
            response("first answer", Vec::new()),
            response(compaction_summary(), Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;
    let before = fixture.chat.snapshot().await.expect("before");
    fixture
        .storage
        .fail_compaction
        .store(true, Ordering::Relaxed);

    assert!(matches!(
        fixture
            .handle
            .compact_conversation(BTreeSet::new())
            .await,
        Err(CompactionError::Persistence(message)) if message.contains("write failed")
    ));
    assert_eq!(fixture.chat.snapshot().await.expect("after"), before);
    assert!(fixture.storage.compactions.lock().unwrap().is_empty());
    let signals = fixture.trace.signals.lock().unwrap();
    let compact_trace = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::CompactionFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("failed compaction trace");
    assert_eq!(compact_trace.status, TraceStatus::Failed);
    assert_eq!(
        compact_trace.error_code.as_deref(),
        Some("persistence_error")
    );
    assert_eq!(compact_trace.attempt_count, Some(1));
    assert!(compact_trace.attributes.checkpoint_id.is_none());
}

#[tokio::test]
async fn empty_conversation_is_not_sent_to_the_compaction_model() {
    let fixture = runtime(
        vec![response(compaction_summary(), Vec::new())],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );

    assert!(matches!(
        fixture.handle.compact_conversation(BTreeSet::new()).await,
        Err(CompactionError::EmptyConversation)
    ));
    assert!(fixture.model.requests.lock().unwrap().is_empty());
    let signals = fixture.trace.signals.lock().unwrap();
    let compact_trace = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::CompactionFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("empty compaction trace");
    assert_eq!(
        compact_trace.error_code.as_deref(),
        Some("empty_conversation")
    );
    assert_eq!(compact_trace.attempt_count, Some(0));
}

#[tokio::test]
async fn runtime_records_versioned_model_and_tool_trace_attributes() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "read", r#"{"path":"README.md"}"#)],
            ),
            response("done", Vec::new()),
        ],
        vec![ToolResult::succeeded("file contents")],
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let signals = fixture.trace.signals.lock().unwrap();
    let first_model = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ModelCallFinished(finished)
                if finished.attributes.model_call_index == 1 =>
            {
                Some(finished)
            }
            _ => None,
        })
        .expect("first model trace");
    assert_eq!(first_model.attempt_count, Some(1));
    assert_eq!(first_model.attributes.schema_version, 1);
    assert_eq!(first_model.attributes.model_call_index, 1);
    assert_eq!(
        first_model.attributes.finish_reason.as_deref(),
        Some("tool_use")
    );
    assert_eq!(
        first_model.attributes.response_id.as_deref(),
        Some("response")
    );
    assert_eq!(
        first_model.attributes.actual_model.as_deref(),
        Some("test-model")
    );
    assert!(
        serde_json::to_value(&first_model.attributes)
            .expect("model trace attributes")
            .get("attempts")
            .is_none()
    );
    assert!(first_model.attributes.ttft_ms.is_some());
    assert_eq!(
        first_model.attributes.delivery_state.as_deref(),
        Some("semantic_output_emitted")
    );
    assert_eq!(first_model.attributes.request_message_count, Some(3));
    assert!(
        first_model
            .attributes
            .tool_definition_count
            .is_some_and(|count| count > 0)
    );
    let model_attributes =
        serde_json::to_value(&first_model.attributes).expect("model trace attributes");
    for removed in [
        "requestSystemMessageCount",
        "requestUserMessageCount",
        "requestAssistantMessageCount",
        "requestToolMessageCount",
        "requestContentBytes",
        "toolDefinitionBytes",
        "responseTextBytes",
        "responseReasoningBytes",
        "responseToolArgumentsBytes",
    ] {
        assert!(model_attributes.get(removed).is_none());
    }
    assert!(
        model_attributes["requestEstimatedSystemContextTokens"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    assert!(
        model_attributes["requestEstimatedConversationTokens"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    assert!(
        model_attributes["requestEstimatedToolSurfaceTokens"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    assert!(
        model_attributes["requestEstimatedInputTokens"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );

    let tool = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("tool trace");
    assert_eq!(tool.attributes.schema_version, 1);
    assert_eq!(tool.attributes.permission_policy.as_deref(), Some("allow"));
    assert_eq!(
        tool.attributes.permission_decision.as_deref(),
        Some("allow")
    );
    assert_eq!(
        tool.attributes.permission_decision_source.as_deref(),
        Some("builtin")
    );
    assert_eq!(tool.attributes.result_persisted, Some(true));
    assert_eq!(tool.attributes.artifact_count, Some(0));
    let tool_attributes = serde_json::to_value(&tool.attributes).expect("tool trace attributes");
    for removed in [
        "inputBytes",
        "outputBytes",
        "outputLines",
        "inputTopLevelKeyCount",
    ] {
        assert!(tool_attributes.get(removed).is_none());
    }
}

#[tokio::test]
async fn acc_21_and_72_tool_trace_records_readonly_proof_and_rule_provenance() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-readonly",
                    "bash",
                    r#"{"readonlyProofKey":"git status"}"#,
                )],
            ),
            response("done", Vec::new()),
        ],
        vec![ToolResult::succeeded("clean")],
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let signals = fixture.trace.signals.lock().unwrap();
    let tool = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("readonly tool trace");
    assert_eq!(
        tool.attributes.permission_decision_source.as_deref(),
        Some("readonly_proof")
    );
    assert_eq!(
        tool.attributes.readonly_proof_key.as_deref(),
        Some("git status")
    );
    assert_eq!(
        tool.attributes.permission_rule_id.as_deref(),
        Some("builtin.allow.workspace_root_read")
    );
    assert_eq!(
        tool.attributes.permission_rule_scope.as_deref(),
        Some("builtin")
    );
}

#[tokio::test]
async fn p4_tool_trace_records_mode_filesystem_command_source() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-mkdir",
                    "bash",
                    r#"{"filesystemCommandProof":true,"path":"src/x"}"#,
                )],
            ),
            response("done", Vec::new()),
        ],
        vec![ToolResult::succeeded("created")],
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let signals = fixture.trace.signals.lock().unwrap();
    let tool = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("filesystem command tool trace");
    assert_eq!(
        tool.attributes.permission_decision_source.as_deref(),
        Some("mode_fs_command")
    );
}

#[tokio::test]
async fn acc_72_builtin_denial_trace_records_rule_id_and_scope() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-denied",
                    "write",
                    r#"{"path":".git/config"}"#,
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
    );
    start(&fixture).await;
    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    let signals = fixture.trace.signals.lock().unwrap();
    let tool = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("denied tool trace");
    assert_eq!(tool.status, TraceStatus::Denied);
    assert_eq!(
        tool.attributes.permission_decision_source.as_deref(),
        Some("builtin")
    );
    assert!(
        tool.attributes
            .permission_rule_id
            .as_deref()
            .is_some_and(|id| id.starts_with("builtin.deny..git."))
    );
    assert_eq!(
        tool.attributes.permission_rule_scope.as_deref(),
        Some("builtin")
    );
}

#[tokio::test]
async fn tool_trace_records_result_persistence_failure_without_changing_tool_status() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "read", r#"{"path":"README.md"}"#)],
        )],
        vec![ToolResult::succeeded("file contents")],
        PermissionMode::AcceptEdits,
        false,
    );
    fixture
        .storage
        .fail_tool_result
        .store(true, Ordering::Relaxed);
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Failed { code, .. } if code == "persistence_error"
    ));
    let signals = fixture.trace.signals.lock().unwrap();
    let tool = signals
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished) => Some(finished),
            _ => None,
        })
        .expect("terminal tool trace");
    assert_eq!(tool.status.as_str(), "succeeded");
    assert_eq!(tool.attributes.result_persisted, Some(false));
    assert!(tool.response_payload.is_some());
    assert!(
        serde_json::to_value(&tool.attributes)
            .expect("tool trace attributes")
            .get("resultPersistErrorCode")
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// 多智能体：Core 控制工具与 explorer 边界
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parent_spawns_three_explorers_and_aggregates_their_deliveries() {
    let parent_session_id = SessionId::new("session-three-explorers");
    let host = Arc::new(SpawningSessionHost::default());
    let control = AgentControl::new(
        parent_session_id.clone(),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    host.install_control(control.clone());
    let mut parent = runtime_with_options(
        vec![
            Ok(response(
                "",
                vec![
                    tool_call(
                        "spawn-api",
                        "spawn_agent",
                        r#"{"task_name":"inspect_api","message":"inspect the API"}"#,
                    ),
                    tool_call(
                        "spawn-storage",
                        "spawn_agent",
                        r#"{"task_name":"inspect_storage","message":"inspect storage"}"#,
                    ),
                    tool_call(
                        "spawn-runtime",
                        "spawn_agent",
                        r#"{"task_name":"inspect_runtime","message":"inspect runtime"}"#,
                    ),
                    tool_call("delivery-barrier", "write", r#"{"path":"barrier"}"#),
                ],
            )),
            Ok(response("aggregated three findings", Vec::new())),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: parent_session_id,
            agent_control: Some(control.clone()),
            ..RuntimeOptions::default()
        },
    );
    host.insert(parent.handle.clone());
    start(&parent).await;

    let (turn_id, tool_call_id) = wait_for_permission(&mut parent.updates).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let statuses = control.list_statuses().await.expect("list explorers");
            if statuses.len() == 3 && statuses.iter().all(|agent| agent.status == "completed") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("all three explorers complete");
    parent
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::AllowOnce)
        .await
        .expect("release parent delivery barrier");

    assert_eq!(
        wait_for_terminal(&mut parent.updates).await,
        TurnOutcome::Completed {
            final_text: "aggregated three findings".to_string()
        }
    );
    let requests = parent.model.requests.lock().unwrap();
    let delivered = requests[1]
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::Text(text)) if text.text.contains("<agent_message>") => {
                Some(text.text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 3);
    for task_name in ["inspect_api", "inspect_storage", "inspect_runtime"] {
        assert!(
            delivered
                .iter()
                .any(|message| message.contains(&format!("<task>{task_name}</task>")))
        );
    }
}

#[tokio::test]
async fn fourth_spawn_is_a_failed_tool_result_and_parent_turn_continues() {
    let host = Arc::new(SlotHoldingHost::default());
    let control = AgentControl::with_max_active_turns(
        SessionId::new("session-parent-tools"),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
        3,
    );
    let spawn_calls = (0..4)
        .map(|index| {
            tool_call(
                &format!("spawn-{index}"),
                "spawn_agent",
                &serde_json::json!({
                    "task_name": format!("lookup_{index}"),
                    "message": format!("inspect area {index}")
                })
                .to_string(),
            )
        })
        .collect();
    let mut fixture = runtime_with_options(
        vec![
            Ok(response("", spawn_calls)),
            Ok(response("parent continued", Vec::new())),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-parent-tools"),
            agent_control: Some(control.clone()),
            ..RuntimeOptions::default()
        },
    );
    start(&fixture).await;

    let updates = collect_updates_until_terminal(&mut fixture.updates).await;
    assert!(matches!(
        updates.last(),
        Some(SessionUpdate::TurnFinished {
            outcome: TurnOutcome::Completed { final_text }
        }) if final_text == "parent continued"
    ));
    let spawn_results = updates
        .iter()
        .filter_map(|update| match update {
            SessionUpdate::ToolCallFinished {
                tool_name,
                output,
                is_error,
                ..
            } if tool_name == "spawn_agent" => Some((*is_error, output.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(spawn_results.len(), 4);
    assert_eq!(spawn_results.iter().filter(|(error, _)| !error).count(), 3);
    assert!(
        spawn_results
            .iter()
            .any(|(error, output)| *error && output.contains("agent_limit_reached"))
    );
    assert_eq!(control.list().len(), 3);
    assert_eq!(control.active_turns(), 3);
    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::PermissionRequested { .. }))
    );
}

#[tokio::test]
async fn child_active_turn_owns_and_releases_its_slot_at_terminal() {
    let host = Arc::new(SessionHandleHost::default());
    let parent_session_id = SessionId::new("session-slot-parent");
    let control = AgentControl::new(
        parent_session_id.clone(),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut child = runtime_with_options(
        vec![Ok(response("done", Vec::new()))],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-slot-child"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::NonInteractive,
            parent_link: Some(ParentLink {
                parent_session_id,
                task_name: "slot_child".to_string(),
                agent_control: control.clone(),
            }),
            agent_control: Some(control.clone()),
        },
    );
    let slot = control.try_acquire_turn_slot().expect("slot");
    child
        .handle
        .start_sub_agent_turn("inspect".to_string(), BTreeSet::new(), slot)
        .await
        .expect("child turn accepted");
    assert_eq!(control.active_turns(), 1);

    assert!(matches!(
        wait_for_terminal(&mut child.updates).await,
        TurnOutcome::Completed { .. }
    ));
    child.handle.snapshot().await.expect("actor barrier");
    assert_eq!(control.active_turns(), 0);
}

#[tokio::test]
async fn explorer_denies_ask_then_runs_readonly_without_permission_card() {
    let host = Arc::new(SessionHandleHost::default());
    let parent_session_id = SessionId::new("session-readonly-parent");
    let control = AgentControl::new(
        parent_session_id.clone(),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut child = runtime_with_options(
        vec![
            Ok(response(
                "",
                vec![tool_call(
                    "cargo-check",
                    "bash",
                    r#"{"program":"cargo","args":["check"]}"#,
                )],
            )),
            Ok(response(
                "",
                vec![tool_call(
                    "git-log",
                    "bash",
                    r#"{"command":"git log","readonlyProofKey":"git log"}"#,
                )],
            )),
            Ok(response("readonly investigation complete", Vec::new())),
        ],
        vec![ToolResult::succeeded("commit history")],
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-readonly-child"),
            model_capabilities: test_capabilities(200_000, 32_768),
            approval: SessionApproval::NonInteractive,
            parent_link: Some(ParentLink {
                parent_session_id,
                task_name: "readonly_child".to_string(),
                agent_control: control.clone(),
            }),
            agent_control: Some(control),
        },
    );
    start(&child).await;

    let updates = collect_updates_until_terminal(&mut child.updates).await;
    assert!(matches!(
        updates.last(),
        Some(SessionUpdate::TurnFinished {
            outcome: TurnOutcome::Completed { .. }
        })
    ));
    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::PermissionRequested { .. }))
    );
    let bash_results = updates
        .iter()
        .filter_map(|update| match update {
            SessionUpdate::ToolCallFinished {
                tool_name,
                output,
                is_error,
                ..
            } if tool_name == "bash" => Some((*is_error, output.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bash_results.len(), 2);
    assert!(bash_results[0].0);
    assert!(bash_results[0].1.contains("cannot request approval"));
    assert!(!bash_results[1].0);
    assert_eq!(child.tools.invocations.lock().unwrap().len(), 1);

    let requests = child.model.requests.lock().unwrap();
    let advertised = requests[0]
        .tools
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(advertised, ["read", "grep", "glob", "list", "bash"]);
    for excluded in ["write", "edit", "spawn_agent", "update_plan"] {
        assert!(!advertised.contains(&excluded));
    }
    let readonly_trace = child
        .trace
        .signals
        .lock()
        .unwrap()
        .iter()
        .find_map(|signal| match signal {
            TraceSignal::ToolCallFinished(finished)
                if finished.attributes.readonly_proof_key.as_deref() == Some("git log") =>
            {
                Some(finished.clone())
            }
            _ => None,
        })
        .expect("readonly proof trace");
    assert_eq!(
        readonly_trace
            .attributes
            .permission_decision_source
            .as_deref(),
        Some("readonly_proof")
    );
}

#[tokio::test]
async fn wait_agent_can_timeout_then_wait_again_for_a_delivery() {
    let host = Arc::new(SessionHandleHost::default());
    let control = AgentControl::new(
        SessionId::new("session-wait-parent"),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut fixture = runtime_with_options(
        vec![
            Ok(response(
                "",
                vec![tool_call(
                    "wait-timeout",
                    "wait_agent",
                    r#"{"timeout_ms":10000}"#,
                )],
            )),
            Ok(response(
                "",
                vec![tool_call(
                    "wait-delivery",
                    "wait_agent",
                    r#"{"timeout_ms":60000}"#,
                )],
            )),
            Ok(response("delivery handled", Vec::new())),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-wait-parent"),
            agent_control: Some(control),
            ..RuntimeOptions::default()
        },
    );
    start(&fixture).await;

    let mut saw_timeout = false;
    let mut delivered = false;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let event = fixture.updates.recv().await.expect("session update");
            match event.update {
                SessionUpdate::ToolCallFinished {
                    tool_name, output, ..
                } if tool_name == "wait_agent" && output.contains("\"timed_out\":true") => {
                    saw_timeout = true;
                }
                SessionUpdate::ToolCallStarted { tool_call }
                    if saw_timeout && tool_call.name == "wait_agent" && !delivered =>
                {
                    fixture
                        .handle
                        .deliver_agent_message(
                            SessionId::new("session-late-explorer-child"),
                            TurnId::new("turn-late-explorer-child"),
                            "late_explorer",
                            AgentMessageKind::FinalAnswer,
                            "arrived during the second wait",
                        )
                        .await
                        .expect("deliver while waiting");
                    delivered = true;
                }
                SessionUpdate::TurnFinished { outcome } => break outcome,
                _ => {}
            }
        }
    })
    .await
    .expect("two waits should finish within the bounded timeout");

    assert!(saw_timeout);
    assert!(delivered);
    assert!(matches!(outcome, TurnOutcome::Completed { .. }));
    let requests = fixture.model.requests.lock().unwrap();
    assert!(requests[2].messages.iter().any(|message| {
        matches!(
            message.content.first(),
            Some(ContentBlock::Text(text))
                if text.text.contains("<task>late_explorer</task>")
        )
    }));
}

#[tokio::test]
async fn p2_acceptance_12_three_explorer_deliveries_do_not_trigger_wait_doom_loop() {
    let host = Arc::new(SessionHandleHost::default());
    let control = AgentControl::new(
        SessionId::new("session-three-explorer-parent"),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut fixture = runtime_with_options(
        vec![
            Ok(response(
                "",
                vec![tool_call(
                    "wait-first-explorer",
                    "wait_agent",
                    r#"{"timeout_ms":240000}"#,
                )],
            )),
            Ok(response(
                "",
                vec![tool_call(
                    "wait-second-explorer",
                    "wait_agent",
                    r#"{"timeout_ms":240000}"#,
                )],
            )),
            Ok(response(
                "",
                vec![tool_call(
                    "wait-third-explorer",
                    "wait_agent",
                    r#"{"timeout_ms":240000}"#,
                )],
            )),
            Ok(response("three explorer reports handled", Vec::new())),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-three-explorer-parent"),
            agent_control: Some(control),
            ..RuntimeOptions::default()
        },
    );
    start(&fixture).await;

    let mut wait_starts = 0;
    let mut delivered_results = 0;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = fixture.updates.recv().await.expect("session update");
            match event.update {
                SessionUpdate::ToolCallStarted { tool_call } if tool_call.name == "wait_agent" => {
                    wait_starts += 1;
                    fixture
                        .handle
                        .deliver_agent_message(
                            SessionId::new(format!("session-explorer-{wait_starts}")),
                            TurnId::new(format!("turn-explorer-{wait_starts}")),
                            format!("explorer_{wait_starts}"),
                            AgentMessageKind::FinalAnswer,
                            format!("report {wait_starts}"),
                        )
                        .await
                        .expect("deliver explorer result while waiting");
                }
                SessionUpdate::ToolCallFinished {
                    tool_name, output, ..
                } if tool_name == "wait_agent" && output.contains("\"delivered\":true") => {
                    delivered_results += 1;
                }
                SessionUpdate::TurnFinished { outcome } => break outcome,
                _ => {}
            }
        }
    })
    .await
    .expect("three delivered waits should finish within the bounded timeout");

    assert_eq!(wait_starts, 3);
    assert_eq!(delivered_results, 3);
    assert_eq!(
        outcome,
        TurnOutcome::Completed {
            final_text: "three explorer reports handled".to_string(),
        }
    );
}

#[tokio::test(start_paused = true)]
async fn three_consecutive_wait_timeouts_end_the_turn_as_doom_loop() {
    let host = Arc::new(SessionHandleHost::default());
    let control = AgentControl::new(
        SessionId::new("session-three-timeout-parent"),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut fixture = runtime_with_options(
        vec![
            Ok(response(
                "",
                vec![tool_call(
                    "wait-timeout-one",
                    "wait_agent",
                    r#"{"timeout_ms":10000}"#,
                )],
            )),
            Ok(response(
                "",
                vec![tool_call(
                    "wait-timeout-two",
                    "wait_agent",
                    r#"{"timeout_ms":10000}"#,
                )],
            )),
            Ok(response(
                "",
                vec![tool_call(
                    "wait-timeout-three",
                    "wait_agent",
                    r#"{"timeout_ms":10000}"#,
                )],
            )),
            Ok(response(
                "the third timeout should stop the turn",
                Vec::new(),
            )),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-three-timeout-parent"),
            agent_control: Some(control),
            ..RuntimeOptions::default()
        },
    );
    start(&fixture).await;

    let mut timed_out_results = 0;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            let event = fixture.updates.recv().await.expect("session update");
            match event.update {
                SessionUpdate::ToolCallFinished {
                    tool_name, output, ..
                } if tool_name == "wait_agent" && output.contains("\"timed_out\":true") => {
                    timed_out_results += 1;
                }
                SessionUpdate::TurnFinished { outcome } => break outcome,
                _ => {}
            }
        }
    })
    .await
    .expect("three paused-time waits should reach a terminal outcome");

    assert_eq!(timed_out_results, 3);
    assert!(matches!(
        outcome,
        TurnOutcome::Failed { ref code, .. } if code == "doom_loop"
    ));
}

#[tokio::test(start_paused = true)]
async fn delivered_wait_resets_timeout_streak_before_two_more_timeouts() {
    let host = Arc::new(SessionHandleHost::default());
    let control = AgentControl::new(
        SessionId::new("session-reset-wait-timeouts-parent"),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let waits = (1..=5)
        .map(|attempt| {
            Ok(response(
                "",
                vec![tool_call(
                    &format!("wait-reset-{attempt}"),
                    "wait_agent",
                    r#"{"timeout_ms":10000}"#,
                )],
            ))
        })
        .chain(std::iter::once(Ok(response(
            "wait timeout streak reset",
            Vec::new(),
        ))))
        .collect();
    let mut fixture = runtime_with_options(
        waits,
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-reset-wait-timeouts-parent"),
            agent_control: Some(control),
            ..RuntimeOptions::default()
        },
    );
    start(&fixture).await;

    let mut wait_starts = 0;
    let mut timed_out_results = 0;
    let mut delivered_results = 0;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            let event = fixture.updates.recv().await.expect("session update");
            match event.update {
                SessionUpdate::ToolCallStarted { tool_call } if tool_call.name == "wait_agent" => {
                    wait_starts += 1;
                    if wait_starts == 3 {
                        fixture
                            .handle
                            .deliver_agent_message(
                                SessionId::new("session-resetting-explorer"),
                                TurnId::new("turn-resetting-explorer"),
                                "resetting_explorer",
                                AgentMessageKind::FinalAnswer,
                                "delivery resets the timeout streak",
                            )
                            .await
                            .expect("deliver the timeout-resetting result");
                    }
                }
                SessionUpdate::ToolCallFinished {
                    tool_name, output, ..
                } if tool_name == "wait_agent" && output.contains("\"timed_out\":true") => {
                    timed_out_results += 1;
                }
                SessionUpdate::ToolCallFinished {
                    tool_name, output, ..
                } if tool_name == "wait_agent" && output.contains("\"delivered\":true") => {
                    delivered_results += 1;
                }
                SessionUpdate::TurnFinished { outcome } => break outcome,
                _ => {}
            }
        }
    })
    .await
    .expect("paused-time waits should reach a terminal outcome");

    assert_eq!(wait_starts, 5);
    assert_eq!(timed_out_results, 4);
    assert_eq!(delivered_results, 1);
    assert_eq!(
        outcome,
        TurnOutcome::Completed {
            final_text: "wait timeout streak reset".to_string(),
        }
    );
}

#[tokio::test]
async fn three_identical_read_calls_still_end_the_turn_as_doom_loop() {
    let mut fixture = runtime(
        vec![
            response("", vec![tool_call("read-one", "read", r#"{"path":"src"}"#)]),
            response("", vec![tool_call("read-two", "read", r#"{"path":"src"}"#)]),
            response(
                "",
                vec![tool_call("read-three", "read", r#"{"path":"src"}"#)],
            ),
            response("the third read should stop the turn", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;

    let outcome = wait_for_terminal(&mut fixture.updates).await;

    assert!(matches!(
        outcome,
        TurnOutcome::Failed { ref code, .. } if code == "doom_loop"
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn cancelling_parent_interrupts_wait_agent_immediately() {
    let host = Arc::new(SessionHandleHost::default());
    let control = AgentControl::new(
        SessionId::new("session-cancel-wait-parent"),
        Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
    );
    let mut fixture = runtime_with_options(
        vec![Ok(response(
            "",
            vec![tool_call(
                "wait-long",
                "wait_agent",
                r#"{"timeout_ms":600000}"#,
            )],
        ))],
        Vec::new(),
        PermissionMode::Default,
        false,
        TestWorkspace::new(),
        SkillRoots::default(),
        RuntimeOptions {
            session_id: SessionId::new("session-cancel-wait-parent"),
            agent_control: Some(control),
            ..RuntimeOptions::default()
        },
    );
    let turn_id = start_with_request(&fixture, "cancel-wait-turn").await;
    wait_for_tool_start(&mut fixture.updates).await;
    fixture
        .handle
        .cancel_turn(turn_id)
        .await
        .expect("cancel wait turn");

    assert_eq!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Cancelled
    );
}

// ---------------------------------------------------------------------------
// update_plan：Core 控制工具
// ---------------------------------------------------------------------------

fn update_plan_call(id: &str, arguments: serde_json::Value) -> ToolCallBlock {
    tool_call(id, "update_plan", &arguments.to_string())
}

async fn collect_updates_until_terminal(
    updates: &mut broadcast::Receiver<SessionUpdateEnvelope>,
) -> Vec<SessionUpdate> {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut seen = Vec::new();
        loop {
            let event = updates.recv().await.expect("session update");
            let finished = matches!(event.update, SessionUpdate::TurnFinished { .. });
            seen.push(event.update);
            if finished {
                return seen;
            }
        }
    })
    .await
    .expect("turn timed out")
}

#[tokio::test]
async fn update_plan_commits_then_broadcasts_a_complete_snapshot() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({
                        "explanation": "scoping",
                        "plan": [
                            { "step": "read schema", "status": "completed" },
                            { "step": "add migration", "status": "in_progress" },
                            { "step": "wire runner", "status": "pending" }
                        ]
                    }),
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;

    let updates = collect_updates_until_terminal(&mut fixture.updates).await;

    let plan_update = updates
        .iter()
        .find_map(|update| match update {
            SessionUpdate::PlanUpdated {
                explanation,
                plan,
                updated_at,
            } => Some((explanation.clone(), plan.clone(), updated_at.clone())),
            _ => None,
        })
        .expect("PlanUpdated");
    assert_eq!(plan_update.0.as_deref(), Some("scoping"));
    assert_eq!(plan_update.1.len(), 3);
    assert_eq!(plan_update.1[1].status, PlanStepStatus::InProgress);
    assert!(
        plan_update.2.ends_with("+08:00"),
        "a Z suffix would put the UI 16 hours off: {}",
        plan_update.2
    );

    // 事件只在持久化成功之后发出。
    let events = fixture.storage.events.lock().unwrap().clone();
    assert!(
        events.contains(&"plan_commit".to_string()),
        "got: {events:?}"
    );
    assert!(
        !events.contains(&"tool_result".to_string()),
        "the plan and its tool result go in one transaction, not two writes: {events:?}"
    );

    // Assistant Tool Call 必须先于计划副作用落库。
    let assistant = events
        .iter()
        .position(|event| event == "assistant")
        .unwrap();
    let commit = events
        .iter()
        .position(|event| event == "plan_commit")
        .unwrap();
    assert!(assistant < commit);

    // 计划提交后仍要发通用的 ToolCallFinished，UI 的工具卡不能因为它是控制工具就少一条。
    assert!(updates.iter().any(|update| matches!(
        update,
        SessionUpdate::ToolCallFinished { tool_name, is_error, .. }
            if tool_name == "update_plan" && !is_error
    )));

    let plans = fixture.storage.plans.lock().unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].steps.len(), 3);
}

#[tokio::test]
async fn update_plan_never_asks_for_permission_even_in_default_mode() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({ "plan": [{ "step": "a", "status": "pending" }] }),
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        // Default mode 下普通的写工具会走审批；控制工具不该受影响。
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;

    let updates = collect_updates_until_terminal(&mut fixture.updates).await;

    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::PermissionRequested { .. })),
        "update_plan does not touch the host, so it must not prompt"
    );
    assert!(matches!(
        updates.last(),
        Some(SessionUpdate::TurnFinished {
            outcome: TurnOutcome::Completed { .. }
        })
    ));
}

#[tokio::test]
async fn an_invalid_plan_fails_the_call_without_changing_stored_state() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    // 两个 in_progress 违反跨元素不变量。
                    serde_json::json!({
                        "plan": [
                            { "step": "a", "status": "in_progress" },
                            { "step": "b", "status": "in_progress" }
                        ]
                    }),
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;

    let updates = collect_updates_until_terminal(&mut fixture.updates).await;

    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::PlanUpdated { .. })),
        "a rejected call must not broadcast a plan"
    );
    let finished = updates
        .iter()
        .find_map(|update| match update {
            SessionUpdate::ToolCallFinished {
                tool_name,
                is_error,
                output,
                ..
            } if tool_name == "update_plan" => Some((*is_error, output.clone())),
            _ => None,
        })
        .expect("a failed tool result still goes back to the model");
    assert!(finished.0);
    assert!(
        finished.1.contains("in_progress"),
        "the model needs to know which invariant it broke: {}",
        finished.1
    );

    assert!(fixture.storage.plans.lock().unwrap().is_empty());
    let events = fixture.storage.events.lock().unwrap().clone();
    assert!(
        !events.contains(&"plan_commit".to_string()),
        "validation failure must not reach storage: {events:?}"
    );
    // 失败结果本身仍要作为普通 Tool Result 落库，否则模型历史里会缺一条。
    assert!(events.contains(&"tool_result".to_string()));
}

#[tokio::test]
async fn a_failed_plan_commit_fails_the_turn_without_broadcasting() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({ "plan": [{ "step": "a", "status": "pending" }] }),
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    fixture
        .storage
        .fail_plan_commit
        .store(true, Ordering::Relaxed);
    start(&fixture).await;

    let updates = collect_updates_until_terminal(&mut fixture.updates).await;

    assert!(
        !updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::PlanUpdated { .. })),
        "nothing was persisted, so nothing may be projected"
    );
    assert!(matches!(
        updates.last(),
        Some(SessionUpdate::TurnFinished {
            outcome: TurnOutcome::Failed { code, .. }
        }) if code == "persistence_error"
    ));
    assert!(fixture.storage.plans.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_mid_turn_compaction_reprojects_the_current_plan_into_the_reminder() {
    // 压缩把模型看到的对话整体换成"用户消息重放 + 摘要 + reminder"三条，原来的
    // update_plan Tool Call 和它的结果都不在其中。所以压缩之后 reminder 是当前计划
    // 唯一的载体——它错了模型就完全失忆，而不是少了一层冗余。
    let mut fixture = runtime_with_capabilities(
        vec![
            // 第 1 轮开头的压缩
            response(compaction_summary(), Vec::new()),
            // 第 1 轮的模型调用：建立计划
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({
                        "explanation": "scoping the work",
                        "plan": [
                            { "step": "read schema", "status": "completed" },
                            { "step": "add migration", "status": "in_progress" },
                            { "step": "wire runner", "status": "pending" }
                        ]
                    }),
                )],
            ),
            // 第 2 轮开头的压缩：此时计划已经存在
            response(compaction_summary(), Vec::new()),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        test_capabilities(2, 1),
    );
    fixture
        .handle
        .start_turn(
            ClientRequestId::new("plan-compaction-request"),
            openwork_core::session::PreparedTurnInput::text("do the multi-step task"),
            BTreeSet::new(),
        )
        .await
        .expect("turn accepted");
    wait_for_terminal(&mut fixture.updates).await;

    let compactions = fixture.storage.compactions.lock().unwrap();
    assert_eq!(compactions.len(), 2, "one compaction per model call round");

    let before = &compactions[0].runtime_reminder;
    assert!(
        !before.contains("Current plan"),
        "there was no plan yet at the first compaction: {before}"
    );

    let after = &compactions[1].runtime_reminder;
    assert!(
        after.contains("## Current plan"),
        "the plan must survive compaction: {after}"
    );
    assert!(after.contains("- [completed] read schema"), "got: {after}");
    assert!(
        after.contains("- [in_progress] add migration"),
        "got: {after}"
    );
    assert!(after.contains("- [pending] wire runner"), "got: {after}");
    assert!(after.contains("scoping the work"), "got: {after}");

    // 计划也要进 extensions，这样下一次压缩能以它为基线。
    let extensions = &compactions[1].runtime_state.extensions;
    assert!(
        extensions.contains_key("turn_plan"),
        "got keys: {:?}",
        extensions.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn clearing_the_plan_removes_it_from_the_next_reminder() {
    // collector 在 collect 返回 None 时会结转旧值，所以清空计划若实现成"没有数据"，
    // 模型会一直看到一份已经删掉的计划，而且全程不报错。
    let mut fixture = runtime_with_capabilities(
        vec![
            response(compaction_summary(), Vec::new()),
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({
                        "plan": [{ "step": "temporary step", "status": "in_progress" }]
                    }),
                )],
            ),
            response(compaction_summary(), Vec::new()),
            response(
                "",
                vec![update_plan_call(
                    "call-clear",
                    serde_json::json!({ "plan": [] }),
                )],
            ),
            response(compaction_summary(), Vec::new()),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::AcceptEdits,
        false,
        test_capabilities(2, 1),
    );
    fixture
        .handle
        .start_turn(
            ClientRequestId::new("plan-clear-request"),
            openwork_core::session::PreparedTurnInput::text("do then abandon the plan"),
            BTreeSet::new(),
        )
        .await
        .expect("turn accepted");
    wait_for_terminal(&mut fixture.updates).await;

    let compactions = fixture.storage.compactions.lock().unwrap();
    assert_eq!(compactions.len(), 3);
    assert!(
        compactions[1].runtime_reminder.contains("temporary step"),
        "got: {}",
        compactions[1].runtime_reminder
    );
    assert!(
        !compactions[2].runtime_reminder.contains("temporary step"),
        "a cleared plan must not linger in extensions: {}",
        compactions[2].runtime_reminder
    );
    assert!(
        !compactions[2].runtime_reminder.contains("Current plan"),
        "an empty plan renders no section at all: {}",
        compactions[2].runtime_reminder
    );
}

#[tokio::test]
async fn a_turn_reports_its_unfinished_plan_steps_when_it_finishes() {
    // 模型建了 3 步、只标完 1 步就作答收工 —— 提示词里"结束前把所有步骤置为 completed"
    // 这条规则没生效。Core 不拦它（Turn 本身是成功的），但必须留下可查询的事实。
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({
                        "plan": [
                            { "step": "a", "status": "completed" },
                            { "step": "b", "status": "in_progress" },
                            { "step": "c", "status": "pending" }
                        ]
                    }),
                )],
            ),
            response("here is your answer", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));

    assert_eq!(
        *fixture.storage.unfinished_plan_steps.lock().unwrap(),
        Some(Some(2)),
        "in_progress 和 pending 都算没收尾"
    );
}

#[tokio::test]
async fn a_turn_that_finished_its_plan_reports_zero() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![update_plan_call(
                    "call-plan",
                    serde_json::json!({
                        "plan": [{ "step": "a", "status": "completed" }]
                    }),
                )],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;

    assert_eq!(
        *fixture.storage.unfinished_plan_steps.lock().unwrap(),
        Some(Some(0)),
    );
}

#[tokio::test]
async fn a_turn_without_a_plan_reports_nothing_rather_than_zero() {
    let mut fixture = runtime(
        vec![response("a simple answer", Vec::new())],
        Vec::new(),
        PermissionMode::Default,
        false,
    );
    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;

    assert_eq!(
        *fixture.storage.unfinished_plan_steps.lock().unwrap(),
        Some(None),
        "简单任务本就不该建计划，把它记成 0 会污染规则生效率的分母"
    );
}
