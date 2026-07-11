use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use openwork_agent::{Agent, AgentConfig, AgentError, AgentEvent};
use openwork_capabilities::{CapabilityCatalog, CatalogError};
use openwork_execution::{BuiltinActionInvoker, ExecutionContext, ExecutionService};
use openwork_permissions::{ApprovalBridge, ApprovalPolicy, PermissionProfile};
use openwork_protocol::{
    capability::{ActionInvoker, CapabilityResolverPort, ExecutionPort},
    model::{ContentBlock, Message, Role},
    provider::{ProviderRepository, ProviderRepositoryError},
};
use openwork_providers::ProviderFactory;
use openwork_session::{NewMessage, SessionError, SessionStore};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

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

/// Frontend-consumable event payload for a chat stream.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamEventPayload {
    pub request_id: String,
    pub session_id: String,
    pub event: &'static str,
    pub delta: Option<String>,
    pub message: Option<String>,
    pub step: Option<usize>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub partial_input: Option<String>,
    pub tool_output: Option<String>,
    pub is_error: Option<bool>,
    pub approval_id: Option<String>,
    pub input: Option<serde_json::Value>,
}

impl ChatStreamEventPayload {
    pub fn simple(request_id: &str, session_id: &str, event: &'static str) -> Self {
        Self {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            event,
            delta: None,
            message: None,
            step: None,
            tool_call_id: None,
            tool_name: None,
            partial_input: None,
            tool_output: None,
            is_error: None,
            approval_id: None,
            input: None,
        }
    }
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
}

/// Composes provider configuration, session persistence, tools, permissions, and the agent loop.
#[derive(Clone)]
pub struct ChatRuntime {
    provider_repository: Arc<dyn ProviderRepository>,
    provider_factory: ProviderFactory,
    session_store: SessionStore,
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
        }
    }

    pub fn provider_repository(&self) -> &dyn ProviderRepository {
        self.provider_repository.as_ref()
    }

    pub fn session_store(&self) -> &SessionStore {
        &self.session_store
    }

    pub async fn generate_stream(
        &self,
        request: ChatGenerateRequest,
        approval_bridge: ApprovalBridge,
        cancel: CancellationToken,
        on_event: impl FnMut(ChatStreamEventPayload) + Send + 'static,
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
        let history_len = stored.len();
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
        let mut agent_config =
            AgentConfig::new(provider, request.model, capabilities, execution, cancel);
        agent_config.approval_bridge = approval_bridge;
        if let Some(approval_policy) = request.approval_policy {
            agent_config.approval_policy = approval_policy;
        }

        let agent = Agent::new(agent_config);
        let on_event = Arc::new(Mutex::new(on_event));
        let event_session_store = self.session_store.clone();
        let event_request_id = request_id.clone();
        let event_session_id = session_id.clone();
        let stream_on_event = Arc::clone(&on_event);
        let result = agent
            .run(history, move |event| {
                let payload = map_agent_event(&event_request_id, &event_session_id, &event);
                if let Ok(value) = serde_json::to_value(&payload) {
                    let store = event_session_store.clone();
                    let session_id = event_session_id.clone();
                    let request_id = event_request_id.clone();
                    let event = payload.event.to_string();
                    tokio::spawn(async move {
                        let _ = store
                            .append_llm_event(&session_id, &request_id, &event, value)
                            .await;
                    });
                }
                emit_runtime_event(&stream_on_event, payload);
            })
            .await;

        match result {
            Ok(run_result) => {
                self.persist_messages(&session_id, run_result.messages, history_len)
                    .await?;
                emit_runtime_event(
                    &on_event,
                    ChatStreamEventPayload::simple(&request_id, &session_id, "done"),
                );
                Ok(ChatGenerateResponse {
                    text: run_result.text,
                    reasoning_text: None,
                })
            }
            Err(AgentError::Cancelled(messages)) => {
                let _ = self
                    .persist_messages(&session_id, messages, history_len)
                    .await;
                emit_runtime_event(
                    &on_event,
                    ChatStreamEventPayload::simple(&request_id, &session_id, "cancelled"),
                );
                Ok(ChatGenerateResponse {
                    text: String::new(),
                    reasoning_text: None,
                })
            }
            Err(AgentError::DoomLoop(_name, messages)) => {
                let _ = self
                    .persist_messages(&session_id, messages, history_len)
                    .await;
                Ok(ChatGenerateResponse {
                    text: String::new(),
                    reasoning_text: None,
                })
            }
            Err(error) => {
                let message = error.to_string();
                emit_runtime_event(
                    &on_event,
                    ChatStreamEventPayload {
                        message: Some(message.clone()),
                        ..ChatStreamEventPayload::simple(&request_id, &session_id, "error")
                    },
                );
                Err(ChatRuntimeError::Agent(error))
            }
        }
    }

    async fn persist_messages(
        &self,
        session_id: &str,
        messages: Vec<Message>,
        history_len: usize,
    ) -> Result<(), SessionError> {
        let new_messages = messages
            .into_iter()
            .skip(history_len)
            .map(|message| NewMessage {
                role: message.role,
                parts: message.content,
            })
            .collect();
        self.session_store
            .append_messages(session_id, new_messages)
            .await
            .map(|_| ())
    }
}

fn emit_runtime_event(
    on_event: &Arc<Mutex<impl FnMut(ChatStreamEventPayload)>>,
    payload: ChatStreamEventPayload,
) {
    let mut on_event = on_event.lock().expect("chat runtime event mutex poisoned");
    on_event(payload);
}

/// Maps an agent loop event into a serializable stream payload.
pub fn map_agent_event(
    request_id: &str,
    session_id: &str,
    event: &AgentEvent,
) -> ChatStreamEventPayload {
    let mut payload = ChatStreamEventPayload::simple(request_id, session_id, "");
    match event {
        AgentEvent::Step(n) => {
            payload.event = "step";
            payload.step = Some(*n);
        }
        AgentEvent::LlmStepStart { index } => {
            payload.event = "llm_step_start";
            payload.step = Some(*index);
        }
        AgentEvent::LlmStepFinish { index, reason, .. } => {
            payload.event = "llm_step_finish";
            payload.step = Some(*index);
            payload.message = Some(reason.clone());
        }
        AgentEvent::LlmFinish { reason, .. } => {
            payload.event = "llm_finish";
            payload.message = Some(reason.clone());
        }
        AgentEvent::TextStart { id } => {
            payload.event = "text_start";
            payload.message = Some(id.clone());
        }
        AgentEvent::TextDelta(delta) => {
            payload.event = "text_delta";
            payload.delta = Some(delta.clone());
        }
        AgentEvent::TextEnd { id } => {
            payload.event = "text_end";
            payload.message = Some(id.clone());
        }
        AgentEvent::ReasoningStart { id } => {
            payload.event = "reasoning_start";
            payload.message = Some(id.clone());
        }
        AgentEvent::ReasoningDelta(delta) => {
            payload.event = "reasoning_delta";
            payload.delta = Some(delta.clone());
        }
        AgentEvent::ReasoningEnd { id } => {
            payload.event = "reasoning_end";
            payload.message = Some(id.clone());
        }
        AgentEvent::ToolCallStart { id, name } => {
            payload.event = "tool_call_start";
            payload.tool_call_id = Some(id.clone());
            payload.tool_name = Some(name.clone());
        }
        AgentEvent::ToolCallDelta { id, partial_input } => {
            payload.event = "tool_call_delta";
            payload.tool_call_id = Some(id.clone());
            payload.partial_input = Some(partial_input.clone());
        }
        AgentEvent::ToolCallEnd { id } => {
            payload.event = "tool_call_end";
            payload.tool_call_id = Some(id.clone());
        }
        AgentEvent::ToolResult {
            id,
            name,
            output,
            is_error,
        } => {
            payload.event = "tool_result";
            payload.tool_call_id = Some(id.clone());
            payload.tool_name = Some(name.clone());
            payload.tool_output = Some(extract_text(output));
            payload.is_error = Some(*is_error);
        }
        AgentEvent::ApprovalRequest { id, name, input } => {
            payload.event = "approval_request";
            payload.approval_id = Some(id.clone());
            payload.tool_name = Some(name.clone());
            payload.input = Some(input.clone());
        }
        AgentEvent::Finished(text) => {
            payload.event = "finished";
            payload.delta = Some(text.clone());
        }
        AgentEvent::DoomLoopDetected { repeated } => {
            payload.event = "doom_loop";
            payload.message = Some(repeated.clone());
        }
    }
    payload
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
