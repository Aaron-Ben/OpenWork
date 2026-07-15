use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use openwork_capabilities::{CapabilityCatalog, CatalogError};
use openwork_core::{
    Agent, AgentConfig, AgentError, AgentEvent, AgentPorts, AgentTraceContext, ApprovalRecovery,
};
use openwork_execution::{
    BuiltinActionInvoker, ExecutionContext, ExecutionService, PermissionProfile,
};
use openwork_observability::{TraceContext, TraceRuntime, TracingTurnRecorder};
use openwork_persistence::{NewMessage, SessionError, SessionStore, TurnOutcome};
use openwork_protocol::{
    approval::{ApprovalPolicy, ApprovalRequested, ResolveApproval},
    capability::{ActionInvoker, CapabilityResolverPort, ExecutionPort},
    domain::{ApprovalId, StepId, ToolRunId, TurnId},
    model::{ContentBlock, Message, Role},
    provider::{ProviderRepository, ProviderRepositoryError},
    trace::{
        TraceRecorderPort, TraceSignal, TraceSpanKind, TraceSpanStart, TraceSpanStatus,
        TraceSpanUpdate,
    },
    turn::TurnRecorderPort,
};
use openwork_providers::ProviderFactory;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{TurnSupervisor, TurnSupervisorError};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGenerateRequest {
    pub request_id: String,
    pub session_id: String,
    pub provider_id: String,
    pub model: String,
    pub user_text: String,
    pub approval_policy: Option<ApprovalPolicy>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGenerateResponse {
    pub text: String,
    pub reasoning_text: Option<String>,
}

/// Application live event envelope consumed by hosts such as Tauri.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnLiveEvent {
    pub request_id: String,
    pub session_id: String,
    #[serde(flatten)]
    pub kind: TurnLiveEventKind,
}

impl TurnLiveEvent {
    pub fn new(request_id: &str, session_id: &str, kind: TurnLiveEventKind) -> Self {
        Self {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TurnLiveEventKind {
    Step {
        step: usize,
        step_id: String,
    },
    LlmStepStart {
        step: usize,
    },
    LlmStepFinish {
        step: usize,
        reason: String,
    },
    LlmFinish {
        reason: String,
    },
    TextStart {
        block_id: String,
    },
    TextDelta {
        delta: String,
    },
    TextEnd {
        block_id: String,
    },
    ReasoningStart {
        block_id: String,
    },
    ReasoningDelta {
        delta: String,
    },
    ReasoningEnd {
        block_id: String,
    },
    ToolCallStart {
        tool_call_id: String,
        tool_name: String,
    },
    ToolCallDelta {
        tool_call_id: String,
        partial_input: String,
    },
    ToolCallEnd {
        tool_call_id: String,
    },
    ToolResult {
        tool_call_id: String,
        tool_run_id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
    ApprovalRequest {
        approval_id: String,
        tool_run_id: String,
        tool_name: String,
        input: serde_json::Value,
        reason: String,
    },
    ApprovalResolved {
        approval_id: String,
    },
    Finished {
        text: String,
    },
    Done,
    Cancelled,
    DoomLoop {
        tool_name: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Error)]
pub enum ChatRuntimeError {
    #[error("capability catalog error: {0}")]
    CapabilityCatalog(#[from] CatalogError),
    #[error("provider not found: {0}")]
    ProviderNotFound(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("provider repository error: {0}")]
    ProviderRepository(#[from] ProviderRepositoryError),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
    #[error("agent error: {0}")]
    Agent(#[from] AgentError),
    #[error("turn supervisor error: {0}")]
    TurnSupervisor(#[from] TurnSupervisorError),
    #[error("pending approval not found: {0}")]
    PendingApprovalNotFound(String),
}

/// Composes provider configuration, session persistence, tools, permissions, and the agent loop.
#[derive(Clone)]
pub struct ChatRuntime {
    provider_repository: Arc<dyn ProviderRepository>,
    provider_factory: ProviderFactory,
    session_store: SessionStore,
    turn_supervisor: Arc<TurnSupervisor>,
    trace: TraceRuntime,
}

impl ChatRuntime {
    pub fn new(
        provider_repository: Arc<dyn ProviderRepository>,
        session_store: SessionStore,
        provider_factory: ProviderFactory,
        trace: TraceRuntime,
    ) -> Self {
        Self {
            provider_repository,
            provider_factory,
            session_store,
            turn_supervisor: Arc::new(TurnSupervisor::default()),
            trace,
        }
    }

    pub fn is_turn_active(&self, turn_id: &TurnId) -> Result<bool, TurnSupervisorError> {
        self.turn_supervisor.contains(turn_id)
    }

    pub async fn resolve_active_approval(
        &self,
        command: ResolveApproval,
    ) -> Result<(), TurnSupervisorError> {
        self.turn_supervisor.resolve(command).await
    }

    pub async fn resume_approval(
        &self,
        command: ResolveApproval,
        cancel: CancellationToken,
        on_event: impl FnMut(TurnLiveEvent) + Send + 'static,
    ) -> Result<(), ChatRuntimeError> {
        let turn_id = command.turn_id.clone();
        let snapshot = self
            .session_store
            .load_turn_lifecycle(turn_id.as_str())
            .await?
            .ok_or_else(|| {
                ChatRuntimeError::PendingApprovalNotFound(command.approval_id.to_string())
            })?;
        let pending = snapshot.pending_approval.ok_or_else(|| {
            ChatRuntimeError::PendingApprovalNotFound(command.approval_id.to_string())
        })?;
        if pending.approval_id != command.approval_id.as_str() {
            return Err(ChatRuntimeError::PendingApprovalNotFound(
                command.approval_id.to_string(),
            ));
        }
        let pending_trace = pending.clone();
        let trace_context = TraceContext {
            trace_id: turn_id.to_string(),
            session_id: snapshot.session_id.clone(),
            turn_id: turn_id.to_string(),
            provider_id: snapshot.provider_id.clone(),
            model: snapshot.model.clone(),
        };
        self.start_turn_trace(
            &trace_context,
            snapshot.started_at.saturating_mul(1_000),
            serde_json::json!({
                "providerId": snapshot.provider_id,
                "model": snapshot.model,
                "recovered": true
            }),
        );

        let provider_config = self
            .provider_repository
            .load_runtime(&snapshot.provider_id)
            .await?
            .ok_or_else(|| ChatRuntimeError::ProviderNotFound(snapshot.provider_id.clone()))?;
        let provider = self.provider_factory.build(&provider_config);
        let session = self
            .session_store
            .load_session(&snapshot.session_id)
            .await?
            .ok_or_else(|| ChatRuntimeError::SessionNotFound(snapshot.session_id.clone()))?;
        let history = self
            .session_store
            .load_messages(&snapshot.session_id)
            .await?
            .into_iter()
            .map(|message| Message {
                role: message.role,
                content: message.parts,
            })
            .collect::<Vec<_>>();
        let working_dir = session
            .working_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let capabilities: Arc<dyn CapabilityResolverPort> = Arc::new(CapabilityCatalog::builtin()?);
        let invoker: Arc<dyn ActionInvoker> =
            Arc::new(BuiltinActionInvoker::new(ExecutionContext::new(
                working_dir.clone(),
                PermissionProfile::workspace_write(working_dir),
                cancel.clone(),
            )));
        let execution: Arc<dyn ExecutionPort> =
            Arc::new(ExecutionService::new(Arc::clone(&capabilities), invoker));
        let approval_commands = self.turn_supervisor.register(turn_id.clone())?;
        let trace_port: Arc<dyn TraceRecorderPort> = Arc::new(self.trace.clone());
        let durable_recorder: Arc<dyn TurnRecorderPort> = Arc::new(
            self.session_store
                .turn_recorder(turn_id.as_str(), &snapshot.session_id),
        );
        let recorder: Arc<dyn TurnRecorderPort> = Arc::new(TracingTurnRecorder::new(
            durable_recorder,
            Arc::clone(&trace_port),
            trace_context.clone(),
        ));
        let agent_config = AgentConfig::new(
            provider,
            snapshot.model.clone(),
            AgentPorts::new(capabilities, execution, recorder).with_trace(trace_port),
            turn_id.clone(),
            approval_commands,
            cancel,
        )
        .with_trace_context(AgentTraceContext {
            trace_id: turn_id.to_string(),
            session_id: snapshot.session_id.clone(),
            provider_id: snapshot.provider_id.clone(),
        });
        let mut agent = Agent::new(agent_config);
        let recovery = ApprovalRecovery {
            request: ApprovalRequested {
                approval_id: ApprovalId::new(pending.approval_id),
                turn_id: turn_id.clone(),
                step_id: StepId::new(pending.step_id),
                tool_run_id: ToolRunId::new(pending.tool_run_id),
                tool_name: pending.tool_name,
                input: pending.input,
                reason: pending.reason,
            },
            provider_tool_call_id: pending.provider_tool_call_id,
            step_index: pending.step_index,
        };
        let session_id = snapshot.session_id;
        let recovery_span_id = format!("{}:recovery:{}", turn_id, pending_trace.approval_id);
        let recovery_started_at = now_unix_ms();
        self.trace.record(TraceSignal::Start(TraceSpanStart {
            trace_id: turn_id.to_string(),
            span_id: recovery_span_id.clone(),
            parent_span_id: Some(turn_id.to_string()),
            span_kind: TraceSpanKind::Recovery,
            span_name: "turn.recovery".to_string(),
            status: TraceSpanStatus::Running,
            session_id: session_id.clone(),
            turn_id: turn_id.to_string(),
            step_id: Some(pending_trace.step_id.clone()),
            tool_run_id: Some(pending_trace.tool_run_id.clone()),
            started_at_unix_ms: recovery_started_at,
            attributes: serde_json::json!({
                "reason": "pending_approval",
                "approvalId": pending_trace.approval_id,
                "stepIndex": pending_trace.step_index
            }),
        }));
        self.trace.record(TraceSignal::Update(TraceSpanUpdate {
            span_id: recovery_span_id,
            status: TraceSpanStatus::Succeeded,
            occurred_at_unix_ms: now_unix_ms(),
            ended: true,
            attributes: serde_json::json!({}),
            error_type: None,
            error_code: None,
            error_message: None,
        }));
        let event_turn_id = turn_id.to_string();
        let on_event = Arc::new(Mutex::new(on_event));
        let stream_on_event = Arc::clone(&on_event);
        let event_session_id = session_id.clone();
        let result = agent
            .resume_after_approval(history, recovery, command.resolution, move |event| {
                let payload = map_agent_event(&event_turn_id, &event_session_id, &event);
                emit_runtime_event(&stream_on_event, payload);
            })
            .await;
        self.turn_supervisor.remove(&turn_id)?;

        match result {
            Ok(_) => {
                self.finish_turn(turn_id.as_str(), &session_id, TurnOutcome::Completed)
                    .await?;
                emit_runtime_event(
                    &on_event,
                    TurnLiveEvent::new(turn_id.as_str(), &session_id, TurnLiveEventKind::Done),
                );
                Ok(())
            }
            Err(AgentError::Cancelled(_)) => {
                self.finish_turn(turn_id.as_str(), &session_id, TurnOutcome::Cancelled)
                    .await?;
                emit_runtime_event(
                    &on_event,
                    TurnLiveEvent::new(turn_id.as_str(), &session_id, TurnLiveEventKind::Cancelled),
                );
                Ok(())
            }
            Err(AgentError::DoomLoop(name, _)) => {
                self.finish_turn(
                    turn_id.as_str(),
                    &session_id,
                    TurnOutcome::DoomLoop { repeated: name },
                )
                .await?;
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                self.finish_turn(
                    turn_id.as_str(),
                    &session_id,
                    TurnOutcome::Failed {
                        message: message.clone(),
                    },
                )
                .await?;
                Err(ChatRuntimeError::Agent(error))
            }
        }
    }

    pub async fn generate_stream(
        &self,
        request: ChatGenerateRequest,
        cancel: CancellationToken,
        on_event: impl FnMut(TurnLiveEvent) + Send + 'static,
    ) -> Result<ChatGenerateResponse, ChatRuntimeError> {
        let request_id = request.request_id.clone();
        let session_id = request.session_id.clone();

        let config = self
            .provider_repository
            .load_runtime(&request.provider_id)
            .await?
            .ok_or_else(|| ChatRuntimeError::ProviderNotFound(request.provider_id.clone()))?;
        let provider = self.provider_factory.build(&config);

        let session = self
            .session_store
            .load_session(&session_id)
            .await?
            .ok_or_else(|| ChatRuntimeError::SessionNotFound(session_id.clone()))?;

        let stored = self.session_store.load_messages(&session_id).await?;
        let mut history: Vec<Message> = stored
            .into_iter()
            .map(|message| Message {
                role: message.role,
                content: message.parts,
            })
            .collect();
        history.push(Message::text(Role::User, &request.user_text));

        let working_dir = session
            .working_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let capabilities: Arc<dyn CapabilityResolverPort> = Arc::new(CapabilityCatalog::builtin()?);
        let invoker: Arc<dyn ActionInvoker> =
            Arc::new(BuiltinActionInvoker::new(ExecutionContext::new(
                working_dir.clone(),
                PermissionProfile::workspace_write(working_dir),
                cancel.clone(),
            )));
        let execution: Arc<dyn ExecutionPort> =
            Arc::new(ExecutionService::new(Arc::clone(&capabilities), invoker));
        let turn_id = TurnId::new(request_id.clone());
        let approval_commands = self.turn_supervisor.register(turn_id.clone())?;
        let trace_context = TraceContext {
            trace_id: request_id.clone(),
            session_id: session_id.clone(),
            turn_id: request_id.clone(),
            provider_id: request.provider_id.clone(),
            model: request.model.clone(),
        };
        let trace_port: Arc<dyn TraceRecorderPort> = Arc::new(self.trace.clone());
        let durable_recorder: Arc<dyn TurnRecorderPort> =
            Arc::new(self.session_store.turn_recorder(&request_id, &session_id));
        let tracing_recorder: Arc<dyn TurnRecorderPort> = Arc::new(TracingTurnRecorder::new(
            durable_recorder,
            Arc::clone(&trace_port),
            trace_context.clone(),
        ));
        let approval_policy = request.approval_policy.unwrap_or_default();
        let mut agent_config = AgentConfig::new(
            provider,
            request.model.clone(),
            AgentPorts::new(capabilities, execution, tracing_recorder).with_trace(trace_port),
            turn_id.clone(),
            approval_commands,
            cancel,
        )
        .with_trace_context(AgentTraceContext {
            trace_id: request_id.clone(),
            session_id: session_id.clone(),
            provider_id: request.provider_id.clone(),
        });
        agent_config.approval_policy = approval_policy;

        let mut agent = Agent::new(agent_config);
        if let Err(error) = self
            .session_store
            .start_turn(
                &request_id,
                &session_id,
                NewMessage {
                    role: Role::User,
                    parts: vec![ContentBlock::text(request.user_text)],
                },
            )
            .await
        {
            let _ = self.turn_supervisor.remove(&turn_id);
            return Err(error.into());
        }
        self.start_turn_trace(
            &trace_context,
            now_unix_ms(),
            serde_json::json!({
                "providerId": request.provider_id,
                "model": request.model,
                "approvalPolicy": approval_policy
            }),
        );
        let on_event = Arc::new(Mutex::new(on_event));
        let event_request_id = request_id.clone();
        let event_session_id = session_id.clone();
        let stream_on_event = Arc::clone(&on_event);
        let result = agent
            .run(history, move |event| {
                let payload = map_agent_event(&event_request_id, &event_session_id, &event);
                emit_runtime_event(&stream_on_event, payload);
            })
            .await;
        self.turn_supervisor.remove(&turn_id)?;

        match result {
            Ok(run_result) => {
                self.finish_turn(&request_id, &session_id, TurnOutcome::Completed)
                    .await?;
                emit_runtime_event(
                    &on_event,
                    TurnLiveEvent::new(&request_id, &session_id, TurnLiveEventKind::Done),
                );
                Ok(ChatGenerateResponse {
                    text: run_result.text,
                    reasoning_text: None,
                })
            }
            Err(AgentError::Cancelled(messages)) => {
                let _ = messages;
                self.finish_turn(&request_id, &session_id, TurnOutcome::Cancelled)
                    .await?;
                emit_runtime_event(
                    &on_event,
                    TurnLiveEvent::new(&request_id, &session_id, TurnLiveEventKind::Cancelled),
                );
                Ok(ChatGenerateResponse {
                    text: String::new(),
                    reasoning_text: None,
                })
            }
            Err(AgentError::DoomLoop(name, messages)) => {
                let _ = messages;
                self.finish_turn(
                    &request_id,
                    &session_id,
                    TurnOutcome::DoomLoop { repeated: name },
                )
                .await?;
                Ok(ChatGenerateResponse {
                    text: String::new(),
                    reasoning_text: None,
                })
            }
            Err(error) => {
                let message = error.to_string();
                self.finish_turn(
                    &request_id,
                    &session_id,
                    TurnOutcome::Failed {
                        message: message.clone(),
                    },
                )
                .await?;
                Err(ChatRuntimeError::Agent(error))
            }
        }
    }

    fn start_turn_trace(
        &self,
        context: &TraceContext,
        started_at_unix_ms: i64,
        attributes: serde_json::Value,
    ) {
        self.trace.record(TraceSignal::Start(TraceSpanStart {
            trace_id: context.trace_id.clone(),
            span_id: context.turn_id.clone(),
            parent_span_id: None,
            span_kind: TraceSpanKind::Turn,
            span_name: "turn.run".to_string(),
            status: TraceSpanStatus::Running,
            session_id: context.session_id.clone(),
            turn_id: context.turn_id.clone(),
            step_id: None,
            tool_run_id: None,
            started_at_unix_ms,
            attributes,
        }));
    }

    async fn finish_turn(
        &self,
        turn_id: &str,
        session_id: &str,
        outcome: TurnOutcome,
    ) -> Result<(), SessionError> {
        let (status, attributes, error_type, error_message) = match &outcome {
            TurnOutcome::Completed => (
                TraceSpanStatus::Succeeded,
                serde_json::json!({"outcome": "completed"}),
                None,
                None,
            ),
            TurnOutcome::Cancelled => (
                TraceSpanStatus::Cancelled,
                serde_json::json!({"outcome": "cancelled"}),
                None,
                None,
            ),
            TurnOutcome::DoomLoop { repeated } => (
                TraceSpanStatus::Failed,
                serde_json::json!({"outcome": "doom_loop", "repeated": repeated}),
                Some("doom_loop".to_string()),
                Some(format!("repeated tool call: {repeated}")),
            ),
            TurnOutcome::Failed { message } => (
                TraceSpanStatus::Failed,
                serde_json::json!({"outcome": "failed"}),
                Some("turn_failed".to_string()),
                Some(message.clone()),
            ),
        };
        self.session_store
            .finish_turn(turn_id, session_id, Vec::new(), outcome)
            .await?;
        self.trace.record(TraceSignal::Update(TraceSpanUpdate {
            span_id: turn_id.to_string(),
            status,
            occurred_at_unix_ms: now_unix_ms(),
            ended: true,
            attributes,
            error_type,
            error_code: None,
            error_message,
        }));
        self.trace.flush().await;
        Ok(())
    }
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn emit_runtime_event(on_event: &Arc<Mutex<impl FnMut(TurnLiveEvent)>>, payload: TurnLiveEvent) {
    let mut on_event = on_event.lock().expect("chat runtime event mutex poisoned");
    on_event(payload);
}

/// Maps an agent loop event into a serializable stream payload.
pub(crate) fn map_agent_event(
    request_id: &str,
    session_id: &str,
    event: &AgentEvent,
) -> TurnLiveEvent {
    let kind = match event {
        AgentEvent::Step { id, index } => TurnLiveEventKind::Step {
            step: *index,
            step_id: id.to_string(),
        },
        AgentEvent::LlmStepStart { index } => TurnLiveEventKind::LlmStepStart { step: *index },
        AgentEvent::LlmStepFinish { index, reason, .. } => TurnLiveEventKind::LlmStepFinish {
            step: *index,
            reason: reason.clone(),
        },
        AgentEvent::LlmFinish { reason, .. } => TurnLiveEventKind::LlmFinish {
            reason: reason.clone(),
        },
        AgentEvent::TextStart { id } => TurnLiveEventKind::TextStart {
            block_id: id.clone(),
        },
        AgentEvent::TextDelta(delta) => TurnLiveEventKind::TextDelta {
            delta: delta.clone(),
        },
        AgentEvent::TextEnd { id } => TurnLiveEventKind::TextEnd {
            block_id: id.clone(),
        },
        AgentEvent::ReasoningStart { id } => TurnLiveEventKind::ReasoningStart {
            block_id: id.clone(),
        },
        AgentEvent::ReasoningDelta(delta) => TurnLiveEventKind::ReasoningDelta {
            delta: delta.clone(),
        },
        AgentEvent::ReasoningEnd { id } => TurnLiveEventKind::ReasoningEnd {
            block_id: id.clone(),
        },
        AgentEvent::ToolCallStart { id, name } => TurnLiveEventKind::ToolCallStart {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
        },
        AgentEvent::ToolCallDelta { id, partial_input } => TurnLiveEventKind::ToolCallDelta {
            tool_call_id: id.clone(),
            partial_input: partial_input.clone(),
        },
        AgentEvent::ToolCallEnd { id } => TurnLiveEventKind::ToolCallEnd {
            tool_call_id: id.clone(),
        },
        AgentEvent::ToolResult {
            id,
            tool_run_id,
            name,
            output,
            is_error,
        } => TurnLiveEventKind::ToolResult {
            tool_call_id: id.clone(),
            tool_run_id: tool_run_id.to_string(),
            tool_name: name.clone(),
            output: extract_text(output),
            is_error: *is_error,
        },
        AgentEvent::ApprovalRequested(request) => TurnLiveEventKind::ApprovalRequest {
            approval_id: request.approval_id.to_string(),
            tool_run_id: request.tool_run_id.to_string(),
            tool_name: request.tool_name.clone(),
            input: request.input.clone(),
            reason: request.reason.clone(),
        },
        AgentEvent::ApprovalResolved(resolved) => TurnLiveEventKind::ApprovalResolved {
            approval_id: resolved.approval_id.to_string(),
        },
        AgentEvent::Finished(text) => TurnLiveEventKind::Finished { text: text.clone() },
        AgentEvent::DoomLoopDetected { repeated } => TurnLiveEventKind::DoomLoop {
            tool_name: repeated.clone(),
        },
    };
    TurnLiveEvent::new(request_id, session_id, kind)
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
