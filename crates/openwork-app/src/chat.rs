use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use openwork_capabilities::{CapabilityCatalog, CatalogError};
use openwork_core::{Agent, AgentConfig, AgentError, AgentEvent};
use openwork_execution::{
    BuiltinActionInvoker, ExecutionContext, ExecutionService, PermissionProfile,
};
use openwork_persistence::{NewMessage, SessionError, SessionStore, TurnOutcome};
use openwork_protocol::{
    approval::{ApprovalPolicy, ResolveApproval},
    capability::{ActionInvoker, CapabilityResolverPort, ExecutionPort},
    domain::TurnId,
    model::{ContentBlock, Message, Role},
    provider::{ProviderRepository, ProviderRepositoryError},
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
        tool_name: String,
        output: String,
        is_error: bool,
    },
    ApprovalRequest {
        approval_id: String,
        tool_name: String,
        input: serde_json::Value,
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
}

/// Composes provider configuration, session persistence, tools, permissions, and the agent loop.
#[derive(Clone)]
pub struct ChatRuntime {
    provider_repository: Arc<dyn ProviderRepository>,
    provider_factory: ProviderFactory,
    session_store: SessionStore,
    turn_supervisor: Arc<TurnSupervisor>,
}

impl ChatRuntime {
    pub fn new(
        provider_repository: Arc<dyn ProviderRepository>,
        session_store: SessionStore,
        provider_factory: ProviderFactory,
    ) -> Self {
        Self {
            provider_repository,
            provider_factory,
            session_store,
            turn_supervisor: Arc::new(TurnSupervisor::default()),
        }
    }

    pub async fn resolve_approval(
        &self,
        command: ResolveApproval,
    ) -> Result<(), TurnSupervisorError> {
        self.turn_supervisor.resolve(command).await
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
        let persisted_history_len = stored.len() + 1;
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
        let mut agent_config = AgentConfig::new(
            provider,
            request.model,
            capabilities,
            execution,
            turn_id.clone(),
            approval_commands,
            cancel,
        );
        if let Some(approval_policy) = request.approval_policy {
            agent_config.approval_policy = approval_policy;
        }

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
                self.finish_turn(
                    &request_id,
                    &session_id,
                    run_result.messages,
                    persisted_history_len,
                    TurnOutcome::Completed,
                )
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
                self.finish_turn(
                    &request_id,
                    &session_id,
                    messages,
                    persisted_history_len,
                    TurnOutcome::Cancelled,
                )
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
                self.finish_turn(
                    &request_id,
                    &session_id,
                    messages,
                    persisted_history_len,
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
                self.session_store
                    .finish_turn(
                        &request_id,
                        &session_id,
                        Vec::new(),
                        TurnOutcome::Failed {
                            message: message.clone(),
                        },
                    )
                    .await?;
                Err(ChatRuntimeError::Agent(error))
            }
        }
    }

    async fn finish_turn(
        &self,
        turn_id: &str,
        session_id: &str,
        messages: Vec<Message>,
        persisted_history_len: usize,
        outcome: TurnOutcome,
    ) -> Result<(), SessionError> {
        let new_messages = messages
            .into_iter()
            .skip(persisted_history_len)
            .map(|message| NewMessage {
                role: message.role,
                parts: message.content,
            })
            .collect();
        self.session_store
            .finish_turn(turn_id, session_id, new_messages, outcome)
            .await
    }
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
        AgentEvent::Step(step) => TurnLiveEventKind::Step { step: *step },
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
            name,
            output,
            is_error,
        } => TurnLiveEventKind::ToolResult {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            output: extract_text(output),
            is_error: *is_error,
        },
        AgentEvent::ApprovalRequested(request) => TurnLiveEventKind::ApprovalRequest {
            approval_id: request.approval_id.to_string(),
            tool_name: request.tool_name.clone(),
            input: request.input.clone(),
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
