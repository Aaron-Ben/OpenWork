use std::collections::BTreeSet;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use openwork_agent::Agent;
use openwork_chat_state::ChatStateHandle;
use openwork_models::model::ModelPort;
use openwork_tools::{ApprovalSessionAction, PermissionMode};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::skills::SkillRoots;

use super::agent_message::{AgentMailbox, AgentMessage};
use super::compaction::{
    AutomaticCompactionPolicy, CompactionTrigger, ConversationCompactionRequest,
    ConversationRewindRequest, new_trace_id, rewind_conversation, run_compaction,
};
use super::permission_state::{PermissionModeOrigin, SessionPermissionState};
use super::run_loop::{RunnerEvent, TurnRunRequest, run_turn};
use super::toolset::TurnToolset;
use super::{
    ClientRequestId, CompactionError, CompactionStateCollector, ConversationCompaction,
    PermissionDecision, PreparedTurnInput, ResolvedModel, SessionApproval, SessionError, SessionId,
    SessionPhase, SessionRuntimeSnapshot, SessionSnapshot, SessionStorage, SessionUpdate,
    SessionUpdateEnvelope, ToolCallId, TraceRecorder, TurnAccepted, TurnId,
};
use crate::plan::TurnPlanSnapshot;

const COMMAND_BUFFER: usize = 64;
const RUNNER_EVENT_BUFFER: usize = 256;
const UPDATE_BUFFER: usize = 512;
const UPDATE_BROADCAST_CAPACITY: usize = 512;

pub struct SessionRuntimeConfig {
    pub session_id: SessionId,
    pub working_directory: PathBuf,
    pub skill_roots: SkillRoots,
    pub resolved_model: ResolvedModel,
    pub agent: Agent,
    pub chat: ChatStateHandle,
    pub model: Arc<dyn ModelPort>,
    pub tools: Arc<TurnToolset>,
    pub storage: Arc<dyn SessionStorage>,
    pub compaction_state: Arc<CompactionStateCollector>,
    pub trace: Arc<dyn TraceRecorder>,
    pub permission_mode: PermissionMode,
    /// Whether anyone can answer an approval prompt. Sub-agent Sessions are
    /// `NonInteractive`, which turns every `Ask` into an immediate denial.
    pub approval: SessionApproval,
    /// Present only for a sub-agent Session. It identifies the parent and
    /// carries the shared control plane used for terminal delivery.
    pub parent_link: Option<super::ParentLink>,
}

#[derive(Clone)]
pub struct SessionHandle {
    session_id: SessionId,
    command_tx: mpsc::Sender<SessionCommand>,
    update_tx: broadcast::Sender<SessionUpdateEnvelope>,
    reload_required: Arc<AtomicBool>,
}

impl SessionHandle {
    pub fn spawn(config: SessionRuntimeConfig) -> Self {
        Self::spawn_inner(config, None)
    }

    pub fn spawn_with_global_updates(
        config: SessionRuntimeConfig,
        global_update_tx: broadcast::Sender<SessionUpdateEnvelope>,
    ) -> Self {
        Self::spawn_inner(config, Some(global_update_tx))
    }

    fn spawn_inner(
        config: SessionRuntimeConfig,
        global_update_tx: Option<broadcast::Sender<SessionUpdateEnvelope>>,
    ) -> Self {
        let session_id = config.session_id.clone();
        let (command_tx, command_rx) = mpsc::channel(COMMAND_BUFFER);
        let (runner_tx, runner_rx) = mpsc::channel(RUNNER_EVENT_BUFFER);
        let (update_tx, _) = broadcast::channel(UPDATE_BROADCAST_CAPACITY);
        let reload_required = Arc::new(AtomicBool::new(false));
        let actor = SessionActor::new(
            config,
            Arc::clone(&reload_required),
            command_rx,
            runner_tx,
            runner_rx,
            update_tx.clone(),
            global_update_tx,
        );
        tokio::spawn(actor.run());
        Self {
            session_id,
            command_tx,
            update_tx,
            reload_required,
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn requires_reload(&self) -> bool {
        self.reload_required.load(Ordering::Acquire)
    }

    pub fn subscribe_updates(&self) -> broadcast::Receiver<SessionUpdateEnvelope> {
        self.update_tx.subscribe()
    }

    pub async fn start_turn(
        &self,
        client_request_id: ClientRequestId,
        input: PreparedTurnInput,
        disabled_skill_names: BTreeSet<String>,
    ) -> Result<TurnAccepted, SessionError> {
        self.start_turn_with_policy(
            client_request_id,
            input,
            AutomaticCompactionPolicy::default(),
            disabled_skill_names,
        )
        .await
    }

    pub async fn start_turn_with_context_window(
        &self,
        client_request_id: ClientRequestId,
        input: PreparedTurnInput,
        context_window_tokens: u64,
        disabled_skill_names: BTreeSet<String>,
    ) -> Result<TurnAccepted, SessionError> {
        let compaction_policy =
            AutomaticCompactionPolicy::for_context_window(context_window_tokens)
                .ok_or(SessionError::InvalidContextWindowTokens)?;
        self.start_turn_with_policy(
            client_request_id,
            input,
            compaction_policy,
            disabled_skill_names,
        )
        .await
    }

    async fn start_turn_with_policy(
        &self,
        client_request_id: ClientRequestId,
        input: PreparedTurnInput,
        compaction_policy: AutomaticCompactionPolicy,
        disabled_skill_names: BTreeSet<String>,
    ) -> Result<TurnAccepted, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::StartTurn {
            turn_id: TurnId::generate(),
            client_request_id,
            input,
            compaction_policy,
            disabled_skill_names,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| SessionError::ActorStopped)?
    }

    pub async fn cancel_turn(&self, turn_id: TurnId) -> Result<bool, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::CancelTurn {
            turn_id,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| SessionError::ActorStopped)?
    }

    /// Queues a sub-agent result for the next Model Call without starting a
    /// Turn. An idle Session keeps the message until the next user Turn.
    pub async fn deliver_agent_message(
        &self,
        task_name: impl Into<String>,
        kind: super::AgentMessageKind,
        body: impl Into<String>,
    ) -> Result<(), SessionError> {
        self.send(SessionCommand::DeliverAgentMessage {
            task_name: task_name.into(),
            kind,
            body: body.into(),
        })
        .await
    }

    pub async fn resolve_permission(
        &self,
        turn_id: TurnId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    ) -> Result<(), SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::ResolvePermission {
            turn_id,
            tool_call_id,
            decision,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| SessionError::ActorStopped)?
    }

    pub async fn set_permission_mode(
        &self,
        mode: PermissionMode,
    ) -> Result<PermissionMode, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::SetPermissionMode { mode, respond_to })
            .await?;
        response.await.map_err(|_| SessionError::ActorStopped)
    }

    pub async fn compact_conversation(
        &self,
        disabled_skill_names: BTreeSet<String>,
    ) -> Result<ConversationCompaction, CompactionError> {
        let (respond_to, response) = oneshot::channel();
        self.command_tx
            .send(SessionCommand::CompactConversation {
                disabled_skill_names,
                respond_to,
            })
            .await
            .map_err(|_| CompactionError::ActorStopped)?;
        response.await.map_err(|_| CompactionError::ActorStopped)?
    }

    pub async fn rewind_conversation(
        &self,
        compaction_id: String,
    ) -> Result<ConversationCompaction, CompactionError> {
        let (respond_to, response) = oneshot::channel();
        self.command_tx
            .send(SessionCommand::RewindConversation {
                compaction_id,
                respond_to,
            })
            .await
            .map_err(|_| CompactionError::ActorStopped)?;
        response.await.map_err(|_| CompactionError::ActorStopped)?
    }

    pub async fn snapshot(&self) -> Result<SessionSnapshot, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::Snapshot { respond_to }).await?;
        response.await.map_err(|_| SessionError::ActorStopped)
    }

    pub async fn replay_updates(
        &self,
        after_sequence: u64,
    ) -> Result<Vec<SessionUpdateEnvelope>, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::ReplayUpdates {
            after_sequence,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| SessionError::ActorStopped)
    }

    pub(crate) async fn accepted_turn(
        &self,
        client_request_id: ClientRequestId,
    ) -> Result<Option<TurnAccepted>, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::AcceptedTurn {
            client_request_id,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| SessionError::ActorStopped)
    }

    async fn send(&self, command: SessionCommand) -> Result<(), SessionError> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| SessionError::ActorStopped)
    }
}

enum SessionCommand {
    StartTurn {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        input: PreparedTurnInput,
        compaction_policy: AutomaticCompactionPolicy,
        disabled_skill_names: BTreeSet<String>,
        respond_to: oneshot::Sender<Result<TurnAccepted, SessionError>>,
    },
    CancelTurn {
        turn_id: TurnId,
        respond_to: oneshot::Sender<Result<bool, SessionError>>,
    },
    ResolvePermission {
        turn_id: TurnId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
        respond_to: oneshot::Sender<Result<(), SessionError>>,
    },
    DeliverAgentMessage {
        task_name: String,
        kind: super::AgentMessageKind,
        body: String,
    },
    SetPermissionMode {
        mode: PermissionMode,
        respond_to: oneshot::Sender<PermissionMode>,
    },
    CompactConversation {
        disabled_skill_names: BTreeSet<String>,
        respond_to: oneshot::Sender<Result<ConversationCompaction, CompactionError>>,
    },
    RewindConversation {
        compaction_id: String,
        respond_to: oneshot::Sender<Result<ConversationCompaction, CompactionError>>,
    },
    Snapshot {
        respond_to: oneshot::Sender<SessionSnapshot>,
    },
    ReplayUpdates {
        after_sequence: u64,
        respond_to: oneshot::Sender<Vec<SessionUpdateEnvelope>>,
    },
    AcceptedTurn {
        client_request_id: ClientRequestId,
        respond_to: oneshot::Sender<Option<TurnAccepted>>,
    },
}

struct ActiveTurn {
    turn_id: TurnId,
    client_request_id: ClientRequestId,
    cancel: CancellationToken,
    permission: Option<PendingPermission>,
}

struct PendingPermission {
    tool_call_id: ToolCallId,
    session_action: Option<ApprovalSessionAction>,
    respond_to: oneshot::Sender<PermissionDecision>,
}

struct SessionActor {
    session_id: SessionId,
    working_directory: PathBuf,
    skill_roots: SkillRoots,
    resolved_model: ResolvedModel,
    agent: Agent,
    chat: ChatStateHandle,
    model: Arc<dyn ModelPort>,
    tools: Arc<TurnToolset>,
    storage: Arc<dyn SessionStorage>,
    compaction_state: Arc<CompactionStateCollector>,
    reload_required: Arc<AtomicBool>,
    trace: Arc<dyn TraceRecorder>,
    approval: SessionApproval,
    parent_link: Option<super::ParentLink>,
    mailbox: AgentMailbox,
    command_rx: mpsc::Receiver<SessionCommand>,
    runner_tx: mpsc::Sender<RunnerEvent>,
    runner_rx: mpsc::Receiver<RunnerEvent>,
    update_tx: broadcast::Sender<SessionUpdateEnvelope>,
    global_update_tx: Option<broadcast::Sender<SessionUpdateEnvelope>>,
    update_buffer: VecDeque<SessionUpdateEnvelope>,
    next_update_sequence: u64,
    snapshot: SessionSnapshot,
    active_turn: Option<ActiveTurn>,
    accepted_requests: HashMap<ClientRequestId, TurnAccepted>,
    permission_state_tx: watch::Sender<SessionPermissionState>,
}

impl SessionActor {
    fn new(
        config: SessionRuntimeConfig,
        reload_required: Arc<AtomicBool>,
        command_rx: mpsc::Receiver<SessionCommand>,
        runner_tx: mpsc::Sender<RunnerEvent>,
        runner_rx: mpsc::Receiver<RunnerEvent>,
        update_tx: broadcast::Sender<SessionUpdateEnvelope>,
        global_update_tx: Option<broadcast::Sender<SessionUpdateEnvelope>>,
    ) -> Self {
        let (permission_state_tx, _) =
            watch::channel(SessionPermissionState::new(config.permission_mode));
        Self {
            snapshot: SessionSnapshot {
                // Snapshot V5 adds session-scoped approval actions.
                version: 5,
                session_id: config.session_id.clone(),
                last_update_sequence: 0,
                permission_mode: config.permission_mode,
                runtime: SessionRuntimeSnapshot::Idle,
            },
            session_id: config.session_id,
            working_directory: config.working_directory,
            skill_roots: config.skill_roots,
            resolved_model: config.resolved_model,
            agent: config.agent,
            chat: config.chat,
            model: config.model,
            tools: config.tools,
            storage: config.storage,
            compaction_state: config.compaction_state,
            reload_required,
            trace: config.trace,
            approval: config.approval,
            parent_link: config.parent_link,
            mailbox: AgentMailbox::default(),
            command_rx,
            runner_tx,
            runner_rx,
            update_tx,
            global_update_tx,
            update_buffer: VecDeque::with_capacity(UPDATE_BUFFER),
            next_update_sequence: 1,
            active_turn: None,
            accepted_requests: HashMap::new(),
            permission_state_tx,
        }
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.command_rx.recv() => {
                    let Some(command) = command else { break };
                    self.handle_command(command).await;
                }
                event = self.runner_rx.recv() => {
                    let Some(event) = event else { break };
                    self.handle_runner_event(event).await;
                }
            }
        }
        if let Some(active) = self.active_turn.take() {
            active.cancel.cancel();
        }
    }

    async fn handle_command(&mut self, command: SessionCommand) {
        match command {
            SessionCommand::StartTurn {
                turn_id,
                client_request_id,
                input,
                compaction_policy,
                disabled_skill_names,
                respond_to,
            } => {
                let result = self.start_turn(
                    turn_id,
                    client_request_id,
                    input,
                    compaction_policy,
                    disabled_skill_names,
                );
                let _ = respond_to.send(result);
            }
            SessionCommand::CancelTurn {
                turn_id,
                respond_to,
            } => {
                let result = match self.active_turn.as_ref() {
                    Some(active) if active.turn_id == turn_id => {
                        active.cancel.cancel();
                        Ok(true)
                    }
                    _ => Err(SessionError::TurnNotActive(turn_id)),
                };
                let _ = respond_to.send(result);
            }
            SessionCommand::ResolvePermission {
                turn_id,
                tool_call_id,
                decision,
                respond_to,
            } => {
                let result = self.resolve_permission(turn_id, tool_call_id, decision);
                let _ = respond_to.send(result);
            }
            SessionCommand::DeliverAgentMessage {
                task_name,
                kind,
                body,
            } => {
                self.mailbox
                    .push(AgentMessage {
                        task_name,
                        kind,
                        body,
                    })
                    .await;
            }
            SessionCommand::SetPermissionMode { mode, respond_to } => {
                self.set_permission_mode(mode, PermissionModeOrigin::UserToggle);
                let _ = respond_to.send(mode);
            }
            SessionCommand::CompactConversation {
                disabled_skill_names,
                respond_to,
            } => {
                let result = self.compact_conversation(disabled_skill_names).await;
                let _ = respond_to.send(result);
            }
            SessionCommand::RewindConversation {
                compaction_id,
                respond_to,
            } => {
                let result = self.rewind_conversation(&compaction_id).await;
                let _ = respond_to.send(result);
            }
            SessionCommand::Snapshot { respond_to } => {
                let _ = respond_to.send(self.snapshot.clone());
            }
            SessionCommand::ReplayUpdates {
                after_sequence,
                respond_to,
            } => {
                let updates = self
                    .update_buffer
                    .iter()
                    .filter(|event| event.sequence > after_sequence)
                    .cloned()
                    .collect();
                let _ = respond_to.send(updates);
            }
            SessionCommand::AcceptedTurn {
                client_request_id,
                respond_to,
            } => {
                let _ = respond_to.send(self.accepted_requests.get(&client_request_id).cloned());
            }
        }
    }

    fn start_turn(
        &mut self,
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        input: PreparedTurnInput,
        compaction_policy: AutomaticCompactionPolicy,
        disabled_skill_names: BTreeSet<String>,
    ) -> Result<TurnAccepted, SessionError> {
        if input.is_empty() {
            return Err(SessionError::EmptyInput);
        }
        if self.reload_required.load(Ordering::Acquire) {
            return Err(SessionError::ReloadRequired);
        }
        if let Some(accepted) = self.accepted_requests.get(&client_request_id) {
            return Ok(accepted.clone());
        }
        if let Some(active) = &self.active_turn {
            return Err(SessionError::Busy(active.turn_id.clone()));
        }

        let accepted = TurnAccepted {
            turn_id: turn_id.clone(),
            client_request_id: client_request_id.clone(),
        };
        let cancel = CancellationToken::new();
        self.active_turn = Some(ActiveTurn {
            turn_id: turn_id.clone(),
            client_request_id: client_request_id.clone(),
            cancel: cancel.clone(),
            permission: None,
        });
        self.snapshot.runtime = SessionRuntimeSnapshot::Running {
            turn_id: turn_id.clone(),
            client_request_id: client_request_id.clone(),
            phase: SessionPhase::Starting,
            draft_text: String::new(),
            draft_reasoning: String::new(),
            tool_calls: Vec::new(),
            pending_permission: None,
            // 新 Turn 从无计划开始，不继承上一个 Turn 的计划。
            plan: None,
        };
        self.accepted_requests
            .insert(client_request_id.clone(), accepted.clone());
        self.emit(
            turn_id.clone(),
            SessionUpdate::TurnStarted {
                client_request_id: client_request_id.clone(),
            },
        );

        let request = TurnRunRequest {
            session_id: self.session_id.clone(),
            working_directory: self.working_directory.clone(),
            skill_roots: self.skill_roots.clone(),
            disabled_skill_names,
            turn_id,
            client_request_id,
            input,
            resolved_model: self.resolved_model.clone(),
            agent: self.agent.clone(),
            chat: self.chat.clone(),
            model: Arc::clone(&self.model),
            tools: Arc::clone(&self.tools),
            storage: Arc::clone(&self.storage),
            compaction_state: Arc::clone(&self.compaction_state),
            compaction_policy,
            reload_required: Arc::clone(&self.reload_required),
            trace: Arc::clone(&self.trace),
            cancel,
            events: self.runner_tx.clone(),
            permission_state: self.permission_state_tx.subscribe(),
            approval: self.approval,
            mailbox: self.mailbox.clone(),
        };
        tokio::spawn(run_turn(request));
        Ok(accepted)
    }

    async fn compact_conversation(
        &self,
        disabled_skill_names: BTreeSet<String>,
    ) -> Result<ConversationCompaction, CompactionError> {
        if let Some(active) = &self.active_turn {
            return Err(CompactionError::SessionActive(active.turn_id.clone()));
        }
        let result = run_compaction(ConversationCompactionRequest {
            session_id: self.session_id.clone(),
            model_id: self.resolved_model.model_id.clone(),
            resolved_model_name: self.resolved_model.model_name.clone(),
            working_directory: self.working_directory.clone(),
            skill_roots: self.skill_roots.clone(),
            disabled_skill_names,
            agent: self.agent.clone(),
            chat: self.chat.clone(),
            model: Arc::clone(&self.model),
            storage: Arc::clone(&self.storage),
            state_collector: Arc::clone(&self.compaction_state),
            // 手动压缩在 Turn 活动时会被上面的 SessionActive 挡下，所以这里必然没有
            // 当前计划。传 None 让 collector 结转上次的值，而不是当作一次清空。
            plan: None,
            reload_required: Arc::clone(&self.reload_required),
            trigger: CompactionTrigger::Manual,
            system_context: None,
            trace: Arc::clone(&self.trace),
            trace_id: new_trace_id(),
            cancellation: CancellationToken::new(),
        })
        .await;
        let _ = self.trace.flush_session(&self.session_id).await;
        result
    }

    async fn rewind_conversation(
        &self,
        compaction_id: &str,
    ) -> Result<ConversationCompaction, CompactionError> {
        if let Some(active) = &self.active_turn {
            return Err(CompactionError::SessionActive(active.turn_id.clone()));
        }
        let result = rewind_conversation(ConversationRewindRequest {
            session_id: &self.session_id,
            compaction_id,
            resolved_model_name: &self.resolved_model.model_name,
            chat: &self.chat,
            storage: self.storage.as_ref(),
            state_collector: self.compaction_state.as_ref(),
            reload_required: self.reload_required.as_ref(),
            trace: Arc::clone(&self.trace),
        })
        .await;
        let _ = self.trace.flush_session(&self.session_id).await;
        result
    }

    fn resolve_permission(
        &mut self,
        turn_id: TurnId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    ) -> Result<(), SessionError> {
        let pending = {
            let Some(active) = self.active_turn.as_mut() else {
                return Err(SessionError::TurnNotActive(turn_id));
            };
            if active.turn_id != turn_id {
                return Err(SessionError::TurnNotActive(turn_id));
            }
            let Some(pending) = active.permission.take() else {
                return Err(SessionError::PermissionNotPending(tool_call_id));
            };
            pending
        };
        if pending.tool_call_id != tool_call_id {
            if let Some(active) = self.active_turn.as_mut() {
                active.permission = Some(pending);
            }
            return Err(SessionError::PermissionNotPending(tool_call_id));
        }

        let selected_action = match decision {
            PermissionDecision::AllowSession => match pending.session_action.as_ref() {
                Some(action @ ApprovalSessionAction::AllowExec { .. }) => Some(action),
                _ => {
                    if let Some(active) = self.active_turn.as_mut() {
                        active.permission = Some(pending);
                    }
                    return Err(SessionError::PermissionDecisionUnavailable(tool_call_id));
                }
            },
            PermissionDecision::AcceptEdits => match pending.session_action.as_ref() {
                Some(action @ ApprovalSessionAction::EnableAcceptEdits) => Some(action),
                _ => {
                    if let Some(active) = self.active_turn.as_mut() {
                        active.permission = Some(pending);
                    }
                    return Err(SessionError::PermissionDecisionUnavailable(tool_call_id));
                }
            },
            PermissionDecision::AllowOnce | PermissionDecision::Deny => None,
        };
        match selected_action {
            Some(ApprovalSessionAction::AllowExec { grants }) => {
                self.permission_state_tx.send_modify(|state| {
                    state.apply_exec_grants(&tool_call_id, grants);
                });
            }
            Some(ApprovalSessionAction::EnableAcceptEdits) => {
                self.set_permission_mode(
                    PermissionMode::AcceptEdits,
                    PermissionModeOrigin::ApprovalCard,
                );
            }
            None => {}
        }

        if let SessionRuntimeSnapshot::Running {
            phase,
            pending_permission,
            ..
        } = &mut self.snapshot.runtime
        {
            *phase = SessionPhase::RunningTools;
            *pending_permission = None;
        }
        let _ = pending.respond_to.send(decision.clone());
        self.emit(
            turn_id,
            SessionUpdate::PermissionResolved {
                tool_call_id,
                decision,
                permission_mode: self.snapshot.permission_mode,
            },
        );
        Ok(())
    }

    async fn handle_runner_event(&mut self, event: RunnerEvent) {
        match event {
            RunnerEvent::Update { turn_id, update } => {
                if self.is_active(&turn_id) {
                    self.apply_update(&update);
                    self.emit(turn_id, update);
                }
            }
            RunnerEvent::PermissionRequested {
                request,
                respond_to,
            } => {
                if !self.is_active(&request.turn_id) {
                    let _ = respond_to.send(PermissionDecision::Deny);
                    return;
                }
                if let Some(active) = self.active_turn.as_mut() {
                    active.permission = Some(PendingPermission {
                        tool_call_id: request.tool_call_id.clone(),
                        session_action: request.card.session_action.clone(),
                        respond_to,
                    });
                }
                if let SessionRuntimeSnapshot::Running {
                    phase,
                    pending_permission,
                    ..
                } = &mut self.snapshot.runtime
                {
                    *phase = SessionPhase::WaitingPermission;
                    *pending_permission = Some(Box::new(request.clone()));
                }
                self.emit(
                    request.turn_id.clone(),
                    SessionUpdate::PermissionRequested { request },
                );
            }
            RunnerEvent::Finished { turn_id, outcome } => {
                let Some(active) = self.active_turn.take() else {
                    return;
                };
                if active.turn_id != turn_id {
                    self.active_turn = Some(active);
                    return;
                }
                self.emit(
                    turn_id.clone(),
                    SessionUpdate::TurnFinished {
                        outcome: outcome.clone(),
                    },
                );
                // Turn 刚结束时同进程重连不该丢掉计划卡：把 Running 里的最后一份快照
                // 顺延到 Terminal，而不是让 UI 去重新查询。
                let plan = match &self.snapshot.runtime {
                    SessionRuntimeSnapshot::Running { plan, .. } => plan.clone(),
                    _ => None,
                };
                self.snapshot.runtime = SessionRuntimeSnapshot::Terminal {
                    turn_id,
                    client_request_id: active.client_request_id,
                    outcome: outcome.clone(),
                    plan,
                };
                self.deliver_terminal_outcome(&outcome).await;
            }
        }
    }

    async fn deliver_terminal_outcome(&self, outcome: &super::TurnOutcome) {
        let Some(parent) = &self.parent_link else {
            return;
        };
        let (kind, body) = match outcome {
            super::TurnOutcome::Completed { final_text } => {
                (super::AgentMessageKind::FinalAnswer, final_text.clone())
            }
            super::TurnOutcome::Failed { code, message } => (
                super::AgentMessageKind::Failed,
                format!("{code}: {message}"),
            ),
            super::TurnOutcome::Cancelled => (
                super::AgentMessageKind::Interrupted,
                "Sub-agent turn was interrupted.".to_string(),
            ),
        };
        let _ = parent
            .agent_control
            .deliver_to_parent(&parent.parent_session_id, &parent.task_name, kind, &body)
            .await;
    }

    fn set_permission_mode(&mut self, mode: PermissionMode, origin: PermissionModeOrigin) {
        self.snapshot.permission_mode = mode;
        self.permission_state_tx
            .send_modify(|state| state.set_mode(mode, origin));
    }

    fn is_active(&self, turn_id: &TurnId) -> bool {
        self.active_turn
            .as_ref()
            .is_some_and(|active| &active.turn_id == turn_id)
    }

    fn apply_update(&mut self, update: &SessionUpdate) {
        let SessionRuntimeSnapshot::Running {
            phase,
            draft_text,
            draft_reasoning,
            tool_calls,
            pending_permission,
            plan,
            ..
        } = &mut self.snapshot.runtime
        else {
            return;
        };

        match update {
            SessionUpdate::PhaseChanged { phase: next } => *phase = *next,
            SessionUpdate::TextDelta { delta } => draft_text.push_str(delta),
            SessionUpdate::ReasoningDelta { delta } => draft_reasoning.push_str(delta),
            SessionUpdate::DraftCleared => {
                draft_text.clear();
                draft_reasoning.clear();
            }
            SessionUpdate::ToolCallStarted { tool_call } => tool_calls.push(tool_call.clone()),
            // Progress is intentionally live-only observation data. It remains in
            // the bounded update buffer but is not folded into runtime snapshots.
            SessionUpdate::ToolCallProgress { .. } => {}
            SessionUpdate::ToolCallFinished {
                tool_call_id,
                status,
                output,
                is_error,
                artifacts,
                ..
            } => {
                if let Some(tool_call) = tool_calls
                    .iter_mut()
                    .find(|tool_call| &tool_call.tool_call_id == tool_call_id)
                {
                    tool_call.status = status.clone();
                    tool_call.output = Some(output.clone());
                    tool_call.is_error = Some(*is_error);
                    tool_call.artifacts = artifacts.clone();
                }
            }
            SessionUpdate::PermissionRequested { request } => {
                *phase = SessionPhase::WaitingPermission;
                *pending_permission = Some(Box::new(request.clone()));
            }
            SessionUpdate::PermissionResolved { .. } => {
                *phase = SessionPhase::RunningTools;
                *pending_permission = None;
            }
            // 完整替换，绝不与旧步骤按文本合并：事件本身就是一份完整快照。
            SessionUpdate::PlanUpdated {
                explanation,
                plan: steps,
                updated_at,
            } => {
                *plan = Some(TurnPlanSnapshot {
                    explanation: explanation.clone(),
                    steps: steps.clone(),
                    updated_at: updated_at.clone(),
                });
            }
            SessionUpdate::TurnStarted { .. } | SessionUpdate::TurnFinished { .. } => {}
        }
    }

    fn emit(&mut self, turn_id: TurnId, update: SessionUpdate) {
        let envelope = SessionUpdateEnvelope {
            // Version 6 adds session-scoped approval actions. Version 5 adds
            // the structured permission card. Version 4 adds
            // the `compacting` phase. Version 3 adds structured
            // terminal tool artifacts. Version 2 introduced
            // `tool_call_progress`; snapshot version 2 carries the same
            // artifacts on running tool calls.
            version: 6,
            session_id: self.session_id.clone(),
            turn_id,
            sequence: self.next_update_sequence,
            occurred_at_ms: now_ms(),
            update,
        };
        self.next_update_sequence += 1;
        self.snapshot.last_update_sequence = envelope.sequence;
        if self.update_buffer.len() == UPDATE_BUFFER {
            self.update_buffer.pop_front();
        }
        self.update_buffer.push_back(envelope.clone());
        let _ = self.update_tx.send(envelope.clone());
        if let Some(global_update_tx) = &self.global_update_tx {
            let _ = global_update_tx.send(envelope);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
