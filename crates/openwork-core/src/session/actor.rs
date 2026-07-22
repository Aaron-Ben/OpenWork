use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use openwork_agent::Agent;
use openwork_chat_state::ChatStateHandle;
use openwork_models::model::{ContentBlock, ModelPort};
use openwork_tools::FinalizedToolset;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::run_loop::{RunnerEvent, TurnRunRequest, run_turn};
use super::{
    ClientRequestId, PermissionDecision, ResolvedModel, SessionError, SessionId, SessionPhase,
    SessionRuntimeSnapshot, SessionSnapshot, SessionStorage, SessionUpdate, SessionUpdateEnvelope,
    ToolCallId, TraceRecorder, TurnAccepted, TurnId,
};

const COMMAND_BUFFER: usize = 64;
const RUNNER_EVENT_BUFFER: usize = 256;
const UPDATE_BUFFER: usize = 512;
const UPDATE_BROADCAST_CAPACITY: usize = 512;

pub struct SessionRuntimeConfig {
    pub session_id: SessionId,
    pub working_directory: PathBuf,
    pub resolved_model: ResolvedModel,
    pub agent: Agent,
    pub chat: ChatStateHandle,
    pub model: Arc<dyn ModelPort>,
    pub tools: Arc<FinalizedToolset>,
    pub storage: Arc<dyn SessionStorage>,
    pub trace: Arc<dyn TraceRecorder>,
}

#[derive(Clone)]
pub struct SessionHandle {
    session_id: SessionId,
    command_tx: mpsc::Sender<SessionCommand>,
    update_tx: broadcast::Sender<SessionUpdateEnvelope>,
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
        let actor = SessionActor::new(
            config,
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
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn subscribe_updates(&self) -> broadcast::Receiver<SessionUpdateEnvelope> {
        self.update_tx.subscribe()
    }

    pub async fn start_turn(
        &self,
        client_request_id: ClientRequestId,
        input: Vec<ContentBlock>,
    ) -> Result<TurnAccepted, SessionError> {
        let (respond_to, response) = oneshot::channel();
        self.send(SessionCommand::StartTurn {
            turn_id: TurnId::generate(),
            client_request_id,
            input,
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
        input: Vec<ContentBlock>,
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
    Snapshot {
        respond_to: oneshot::Sender<SessionSnapshot>,
    },
    ReplayUpdates {
        after_sequence: u64,
        respond_to: oneshot::Sender<Vec<SessionUpdateEnvelope>>,
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
    respond_to: oneshot::Sender<PermissionDecision>,
}

struct SessionActor {
    session_id: SessionId,
    working_directory: PathBuf,
    resolved_model: ResolvedModel,
    agent: Agent,
    chat: ChatStateHandle,
    model: Arc<dyn ModelPort>,
    tools: Arc<FinalizedToolset>,
    storage: Arc<dyn SessionStorage>,
    trace: Arc<dyn TraceRecorder>,
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
}

impl SessionActor {
    fn new(
        config: SessionRuntimeConfig,
        command_rx: mpsc::Receiver<SessionCommand>,
        runner_tx: mpsc::Sender<RunnerEvent>,
        runner_rx: mpsc::Receiver<RunnerEvent>,
        update_tx: broadcast::Sender<SessionUpdateEnvelope>,
        global_update_tx: Option<broadcast::Sender<SessionUpdateEnvelope>>,
    ) -> Self {
        Self {
            snapshot: SessionSnapshot {
                version: 2,
                session_id: config.session_id.clone(),
                last_update_sequence: 0,
                runtime: SessionRuntimeSnapshot::Idle,
            },
            session_id: config.session_id,
            working_directory: config.working_directory,
            resolved_model: config.resolved_model,
            agent: config.agent,
            chat: config.chat,
            model: config.model,
            tools: config.tools,
            storage: config.storage,
            trace: config.trace,
            command_rx,
            runner_tx,
            runner_rx,
            update_tx,
            global_update_tx,
            update_buffer: VecDeque::with_capacity(UPDATE_BUFFER),
            next_update_sequence: 1,
            active_turn: None,
            accepted_requests: HashMap::new(),
        }
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.command_rx.recv() => {
                    let Some(command) = command else { break };
                    self.handle_command(command);
                }
                event = self.runner_rx.recv() => {
                    let Some(event) = event else { break };
                    self.handle_runner_event(event);
                }
            }
        }
        if let Some(active) = self.active_turn.take() {
            active.cancel.cancel();
        }
    }

    fn handle_command(&mut self, command: SessionCommand) {
        match command {
            SessionCommand::StartTurn {
                turn_id,
                client_request_id,
                input,
                respond_to,
            } => {
                let result = self.start_turn(turn_id, client_request_id, input);
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
        }
    }

    fn start_turn(
        &mut self,
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        input: Vec<ContentBlock>,
    ) -> Result<TurnAccepted, SessionError> {
        if input.is_empty() {
            return Err(SessionError::EmptyInput);
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
            turn_id,
            client_request_id,
            input,
            resolved_model: self.resolved_model.clone(),
            agent: self.agent.clone(),
            chat: self.chat.clone(),
            model: Arc::clone(&self.model),
            tools: Arc::clone(&self.tools),
            storage: Arc::clone(&self.storage),
            trace: Arc::clone(&self.trace),
            cancel,
            events: self.runner_tx.clone(),
        };
        tokio::spawn(run_turn(request));
        Ok(accepted)
    }

    fn resolve_permission(
        &mut self,
        turn_id: TurnId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    ) -> Result<(), SessionError> {
        let Some(active) = self.active_turn.as_mut() else {
            return Err(SessionError::TurnNotActive(turn_id));
        };
        if active.turn_id != turn_id {
            return Err(SessionError::TurnNotActive(turn_id));
        }
        let Some(pending) = active.permission.take() else {
            return Err(SessionError::PermissionNotPending(tool_call_id));
        };
        if pending.tool_call_id != tool_call_id {
            active.permission = Some(pending);
            return Err(SessionError::PermissionNotPending(tool_call_id));
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
            },
        );
        Ok(())
    }

    fn handle_runner_event(&mut self, event: RunnerEvent) {
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
                self.snapshot.runtime = SessionRuntimeSnapshot::Terminal {
                    turn_id,
                    client_request_id: active.client_request_id,
                    outcome,
                };
            }
        }
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
            SessionUpdate::TurnStarted { .. } | SessionUpdate::TurnFinished { .. } => {}
        }
    }

    fn emit(&mut self, turn_id: TurnId, update: SessionUpdate) {
        let envelope = SessionUpdateEnvelope {
            // Version 3 adds structured terminal tool artifacts. Version 2
            // introduced `tool_call_progress`; snapshot version 2 carries the
            // same artifacts on running tool calls.
            version: 3,
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
