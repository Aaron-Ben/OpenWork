use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use openwork_agent::Agent;
use openwork_chat_state::{ChatStateError, ChatStateHandle};
use openwork_models::model::{
    ContentBlock, Message, ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelResponse,
    Role, ToolCallBlock, ToolResultBlock, ToolResultState,
};
use openwork_tools::{
    FinalizedToolset, PolicyDecision, ToolCallContext as RuntimeToolCallContext,
    ToolCallId as RuntimeToolCallId, ToolErrorCode, ToolInvocation,
    ToolProgress as RuntimeToolProgress, ToolResult, ToolResultContent, ToolResultStatus,
    ToolValidationError,
};
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    ClientRequestId, LiveToolCall, ModelCallFinished, ModelCallStarted, PermissionDecision,
    PermissionRequest, ResolvedModel, SessionId, SessionPhase, SessionStorage, SessionUpdate,
    ToolCallFinished, ToolCallId, ToolCallStarted, ToolProgressUpdate, TraceRecorder, TraceSignal,
    TraceStatus, TurnId, TurnOutcome,
};

pub(super) struct TurnRunRequest {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub client_request_id: ClientRequestId,
    pub input: Vec<ContentBlock>,
    pub resolved_model: ResolvedModel,
    pub agent: Agent,
    pub chat: ChatStateHandle,
    pub model: Arc<dyn ModelPort>,
    pub tools: Arc<FinalizedToolset>,
    pub storage: Arc<dyn SessionStorage>,
    pub trace: Arc<dyn TraceRecorder>,
    pub cancel: CancellationToken,
    pub events: mpsc::Sender<RunnerEvent>,
}

pub(super) enum RunnerEvent {
    Update {
        turn_id: TurnId,
        update: SessionUpdate,
    },
    PermissionRequested {
        request: PermissionRequest,
        respond_to: oneshot::Sender<PermissionDecision>,
    },
    Finished {
        turn_id: TurnId,
        outcome: TurnOutcome,
    },
}

pub(super) async fn run_turn(request: TurnRunRequest) {
    let turn_id = request.turn_id.clone();
    let storage = Arc::clone(&request.storage);
    let events = request.events.clone();
    let mut runner = TurnRunner {
        request,
        repeated_tool: None,
        next_trace_sequence: 1,
    };

    let outcome = match runner.begin().await {
        Ok(()) => {
            let outcome = match runner.run_loop().await {
                Ok(final_text) => TurnOutcome::Completed { final_text },
                Err(error) => error.into_outcome(),
            };
            match storage.finish_turn(&turn_id, &outcome).await {
                Ok(()) => outcome,
                Err(message) => TurnOutcome::Failed {
                    code: "persistence_error".to_string(),
                    message,
                },
            }
        }
        Err(error) => error.into_outcome(),
    };

    let _ = runner.request.trace.flush_turn(&turn_id).await;

    let _ = events
        .send(RunnerEvent::Finished { turn_id, outcome })
        .await;
}

struct TurnRunner {
    request: TurnRunRequest,
    repeated_tool: Option<(String, String, usize)>,
    next_trace_sequence: i64,
}

impl TurnRunner {
    async fn begin(&self) -> Result<(), TurnRunError> {
        self.ensure_not_cancelled()?;
        let user_message = Message {
            role: Role::User,
            content: self.request.input.clone(),
        };
        self.request
            .storage
            .begin_turn(
                &self.request.session_id,
                &self.request.turn_id,
                &self.request.client_request_id,
                &self.request.resolved_model,
                &user_message,
            )
            .await
            .map_err(TurnRunError::Persistence)?;
        self.request
            .chat
            .append_user(self.request.input.clone())
            .await?;
        Ok(())
    }

    async fn run_loop(&mut self) -> Result<String, TurnRunError> {
        for model_call_index in 1..=self.request.agent.policy().max_model_calls {
            self.ensure_not_cancelled()?;
            self.update(SessionUpdate::PhaseChanged {
                phase: SessionPhase::RunningModel,
            })
            .await?;
            self.request
                .storage
                .begin_model_call(&self.request.turn_id, model_call_index)
                .await
                .map_err(TurnRunError::Persistence)?;

            let model_trace = self.start_model_trace();
            let response = match self.call_model(model_call_index).await {
                Ok(response) => {
                    self.finish_model_trace(&model_trace, Ok(&response));
                    response
                }
                Err(error) => {
                    self.finish_model_trace(&model_trace, Err(&error));
                    return Err(error);
                }
            };
            let assistant_message = assistant_message(&response);
            self.request
                .storage
                .append_assistant_message(&self.request.turn_id, &assistant_message, response.usage)
                .await
                .map_err(TurnRunError::Persistence)?;
            self.request
                .chat
                .append_assistant(assistant_message)
                .await?;
            self.update(SessionUpdate::DraftCleared).await?;

            if response.tool_calls.is_empty() {
                return Ok(response.text);
            }

            self.update(SessionUpdate::PhaseChanged {
                phase: SessionPhase::RunningTools,
            })
            .await?;
            for (index, tool_call) in response.tool_calls.iter().enumerate() {
                if let Err(error) = self.run_tool_call(tool_call, &model_trace.span_id).await {
                    for pending in response.tool_calls.iter().skip(index + 1) {
                        self.append_cancelled_tool_result(
                            pending,
                            &model_trace.span_id,
                            "tool was not executed because the turn already terminated",
                        )
                        .await?;
                    }
                    return Err(error);
                }
            }
        }

        Err(TurnRunError::MaxModelCalls(
            self.request.agent.policy().max_model_calls,
        ))
    }

    fn start_model_trace(&mut self) -> ModelCallStarted {
        let started = ModelCallStarted {
            span_id: trace_id("model"),
            turn_id: self.request.turn_id.clone(),
            sequence: self.allocate_trace_sequence(),
            model_id: self.request.resolved_model.model_id.clone(),
            resolved_model_name: self.request.resolved_model.model_name.clone(),
            started_at: OffsetDateTime::now_utc(),
        };
        self.request
            .trace
            .record(TraceSignal::ModelCallStarted(started.clone()));
        started
    }

    fn finish_model_trace(
        &self,
        started: &ModelCallStarted,
        result: Result<&ModelResponse, &TurnRunError>,
    ) {
        let (status, provider_request_id, usage, error_code, error_message) = match result {
            Ok(response) => (
                TraceStatus::Succeeded,
                response.provider_request_id.clone(),
                response.usage,
                None,
                None,
            ),
            Err(error) => (
                trace_status_for_error(error),
                None,
                None,
                Some(error.code().to_string()),
                Some(error.to_string()),
            ),
        };
        self.request
            .trace
            .record(TraceSignal::ModelCallFinished(ModelCallFinished {
                started: started.clone(),
                status,
                provider_request_id,
                attempt_count: 1,
                usage,
                ended_at: OffsetDateTime::now_utc(),
                error_code,
                error_message,
            }));
    }

    async fn call_model(&self, model_call_index: u32) -> Result<ModelResponse, TurnRunError> {
        self.request.chat.begin_draft().await?;
        let request = self
            .request
            .chat
            .build_request(
                &self.request.resolved_model.model_name,
                self.request.agent.system_prompt(),
                self.request.tools.definitions().to_vec(),
            )
            .await?;
        let options =
            ModelCallOptions::new(format!("{}-model-{model_call_index}", self.request.turn_id));
        let stream = tokio::select! {
            _ = self.request.cancel.cancelled() => {
                self.request.chat.discard_draft().await?;
                return Err(TurnRunError::Cancelled);
            }
            result = self.request.model.invoke(request, options) => result,
        };
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                self.request.chat.discard_draft().await?;
                return Err(TurnRunError::Model(error));
            }
        };
        let mut completed = None;

        loop {
            let next = tokio::select! {
                _ = self.request.cancel.cancelled() => {
                    self.request.chat.discard_draft().await?;
                    return Err(TurnRunError::Cancelled);
                }
                item = stream.next() => item,
            };
            let Some(event) = next else { break };
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    self.request.chat.discard_draft().await?;
                    return Err(TurnRunError::Model(error));
                }
            };
            match event {
                ModelEvent::TextDelta { delta, .. } => {
                    self.request.chat.apply_text_delta(&delta).await?;
                    self.update(SessionUpdate::TextDelta { delta }).await?;
                }
                ModelEvent::ReasoningDelta { delta, .. } => {
                    self.request.chat.apply_reasoning_delta(&delta).await?;
                    self.update(SessionUpdate::ReasoningDelta { delta }).await?;
                }
                ModelEvent::ResponseCompleted { response } => {
                    if completed.replace(*response).is_some() {
                        self.request.chat.discard_draft().await?;
                        return Err(TurnRunError::Protocol(
                            "model stream completed more than once".to_string(),
                        ));
                    }
                }
                ModelEvent::TextStart { .. }
                | ModelEvent::TextEnd { .. }
                | ModelEvent::ReasoningStart { .. }
                | ModelEvent::ReasoningEnd { .. }
                | ModelEvent::ToolCallStart { .. }
                | ModelEvent::ToolCallDelta { .. }
                | ModelEvent::ToolCallEnd { .. } => {}
            }
        }

        let Some(response) = completed else {
            self.request.chat.discard_draft().await?;
            return Err(TurnRunError::Protocol(
                "model stream ended without a completed response".to_string(),
            ));
        };
        let _ = self.request.chat.finish_draft().await?;
        Ok(response)
    }

    async fn run_tool_call(
        &mut self,
        call: &ToolCallBlock,
        parent_span_id: &str,
    ) -> Result<(), TurnRunError> {
        self.ensure_not_cancelled()?;
        let tool_call_id = ToolCallId::generate();
        let tool_trace = self.start_tool_trace(call, parent_span_id);
        let mut permission_wait_ms = None;
        let input: serde_json::Value = match serde_json::from_str(&call.input) {
            Ok(input) => input,
            Err(error) => {
                let input = serde_json::Value::Null;
                self.start_live_tool(call, &tool_call_id, input.clone())
                    .await?;
                let result = ToolResult::failed(
                    ToolErrorCode::InvalidArguments,
                    format!("invalid tool input: {error}"),
                    false,
                );
                self.finish_tool_trace(&tool_trace, &result, None, permission_wait_ms);
                return self.append_tool_result(call, tool_call_id, result).await;
            }
        };
        self.start_live_tool(call, &tool_call_id, input.clone())
            .await?;

        let invocation = ToolInvocation::new(&call.name, input.clone());
        let resolved_tool_name = match self.request.tools.validate(&invocation) {
            Ok(definition) => definition.id.to_string(),
            Err(error) => {
                let code = match error {
                    ToolValidationError::UnknownTool(_) => ToolErrorCode::ToolNotFound,
                    ToolValidationError::InvalidInput(_) => ToolErrorCode::InvalidArguments,
                };
                let result = ToolResult::failed(code, error.to_string(), false);
                self.finish_tool_trace(&tool_trace, &result, None, permission_wait_ms);
                return self.append_tool_result(call, tool_call_id, result).await;
            }
        };

        let canonical_input = serde_json::to_string(&input).unwrap_or_else(|_| call.input.clone());
        if self.is_doom_loop(&call.name, &canonical_input) {
            let result = ToolResult::failed(
                ToolErrorCode::ExecutionFailed,
                format!("doom loop detected for tool '{}'", call.name),
                false,
            );
            self.finish_tool_trace(
                &tool_trace,
                &result,
                Some(&resolved_tool_name),
                permission_wait_ms,
            );
            self.append_tool_result(call, tool_call_id, result).await?;
            return Err(TurnRunError::DoomLoop(call.name.clone()));
        }

        match self
            .request
            .tools
            .authorize(&invocation, self.request.agent.policy().permission_mode)
        {
            PolicyDecision::Allow => {}
            PolicyDecision::Deny { reason } => {
                let result = ToolResult::denied(reason.clone());
                self.finish_tool_trace(
                    &tool_trace,
                    &result,
                    Some(&resolved_tool_name),
                    permission_wait_ms,
                );
                self.append_tool_result(call, tool_call_id, result).await?;
                return Err(TurnRunError::PermissionDenied(reason));
            }
            PolicyDecision::Ask { reason } => {
                let request = PermissionRequest {
                    session_id: self.request.session_id.clone(),
                    turn_id: self.request.turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    provider_call_id: call.id.clone(),
                    tool_name: resolved_tool_name.clone(),
                    input,
                    reason,
                };
                let (respond_to, decision) = oneshot::channel();
                if self
                    .request
                    .events
                    .send(RunnerEvent::PermissionRequested {
                        request,
                        respond_to,
                    })
                    .await
                    .is_err()
                {
                    let result = ToolResult::outcome_unknown("session actor stopped");
                    self.finish_tool_trace(
                        &tool_trace,
                        &result,
                        Some(&resolved_tool_name),
                        permission_wait_ms,
                    );
                    self.append_tool_result(call, tool_call_id, result).await?;
                    return Err(TurnRunError::ActorStopped);
                }
                let wait_started = Instant::now();
                let decision = tokio::select! {
                    _ = self.request.cancel.cancelled() => {
                        None
                    }
                    result = decision => result.ok(),
                };
                permission_wait_ms = Some(elapsed_millis(wait_started));
                let Some(decision) = decision else {
                    let cancelled = self.request.cancel.is_cancelled();
                    let result = if cancelled {
                        ToolResult::cancelled("turn cancelled while waiting for permission")
                    } else {
                        ToolResult::outcome_unknown("permission responder stopped")
                    };
                    self.finish_tool_trace(
                        &tool_trace,
                        &result,
                        Some(&resolved_tool_name),
                        permission_wait_ms,
                    );
                    self.append_tool_result(call, tool_call_id, result).await?;
                    return Err(if cancelled {
                        TurnRunError::Cancelled
                    } else {
                        TurnRunError::ActorStopped
                    });
                };
                if decision == PermissionDecision::Deny {
                    let result = ToolResult::denied("user denied tool permission");
                    self.finish_tool_trace(
                        &tool_trace,
                        &result,
                        Some(&resolved_tool_name),
                        permission_wait_ms,
                    );
                    self.append_tool_result(call, tool_call_id, result).await?;
                    return Err(TurnRunError::PermissionDenied(
                        "user denied tool permission".to_string(),
                    ));
                }
            }
        }

        let (progress_tx, mut progress_rx) = mpsc::channel(64);
        let call_context = RuntimeToolCallContext::new(
            RuntimeToolCallId::new(tool_call_id.to_string()),
            self.request.cancel.child_token(),
        )
        .with_progress_sender(progress_tx);
        let tools = Arc::clone(&self.request.tools);
        let mut execution = Box::pin(tools.call(call_context, invocation));
        let mut progress_open = true;
        let result = loop {
            tokio::select! {
                biased;
                progress = progress_rx.recv(), if progress_open => {
                    let Some(progress) = progress else {
                        progress_open = false;
                        continue;
                    };
                    self.update(SessionUpdate::ToolCallProgress {
                        tool_call_id: tool_call_id.clone(),
                        progress: tool_progress_update(progress),
                    }).await?;
                }
                result = &mut execution => break result,
            }
        };
        while let Ok(progress) = progress_rx.try_recv() {
            self.update(SessionUpdate::ToolCallProgress {
                tool_call_id: tool_call_id.clone(),
                progress: tool_progress_update(progress),
            })
            .await?;
        }
        let cancelled = result.status == ToolResultStatus::Cancelled;
        self.finish_tool_trace(
            &tool_trace,
            &result,
            Some(&resolved_tool_name),
            permission_wait_ms,
        );
        self.append_tool_result(call, tool_call_id, result).await?;
        if cancelled {
            return Err(TurnRunError::Cancelled);
        }
        Ok(())
    }

    fn start_tool_trace(&mut self, call: &ToolCallBlock, parent_span_id: &str) -> ToolCallStarted {
        let started = ToolCallStarted {
            span_id: trace_id("tool"),
            turn_id: self.request.turn_id.clone(),
            parent_span_id: parent_span_id.to_string(),
            sequence: self.allocate_trace_sequence(),
            provider_call_id: call.id.clone(),
            requested_tool_name: call.name.clone(),
            started_at: OffsetDateTime::now_utc(),
        };
        self.request
            .trace
            .record(TraceSignal::ToolCallStarted(started.clone()));
        started
    }

    fn finish_tool_trace(
        &self,
        started: &ToolCallStarted,
        result: &ToolResult,
        resolved_tool_name: Option<&str>,
        permission_wait_ms: Option<i64>,
    ) {
        self.request
            .trace
            .record(TraceSignal::ToolCallFinished(ToolCallFinished {
                started: started.clone(),
                status: trace_status_for_tool(result.status),
                resolved_tool_name: resolved_tool_name.map(str::to_string),
                permission_wait_ms,
                ended_at: OffsetDateTime::now_utc(),
                error_code: result
                    .error
                    .as_ref()
                    .map(|error| tool_error_code(error.code).to_string()),
                error_message: result.error.as_ref().map(|error| error.message.clone()),
            }));
    }

    fn allocate_trace_sequence(&mut self) -> i64 {
        let sequence = self.next_trace_sequence;
        self.next_trace_sequence = self.next_trace_sequence.saturating_add(1);
        sequence
    }

    async fn start_live_tool(
        &self,
        call: &ToolCallBlock,
        tool_call_id: &ToolCallId,
        input: serde_json::Value,
    ) -> Result<(), TurnRunError> {
        self.update(SessionUpdate::ToolCallStarted {
            tool_call: LiveToolCall {
                tool_call_id: tool_call_id.clone(),
                provider_call_id: call.id.clone(),
                name: call.name.clone(),
                input,
                status: "validating".to_string(),
                output: None,
                is_error: None,
            },
        })
        .await
    }

    async fn append_cancelled_tool_result(
        &mut self,
        call: &ToolCallBlock,
        parent_span_id: &str,
        reason: &str,
    ) -> Result<(), TurnRunError> {
        let tool_call_id = ToolCallId::generate();
        let tool_trace = self.start_tool_trace(call, parent_span_id);
        let input = serde_json::from_str(&call.input).unwrap_or(serde_json::Value::Null);
        self.start_live_tool(call, &tool_call_id, input).await?;
        let result = ToolResult::cancelled(reason);
        self.finish_tool_trace(&tool_trace, &result, None, None);
        self.append_tool_result(call, tool_call_id, result).await
    }

    async fn append_tool_result(
        &self,
        call: &ToolCallBlock,
        tool_call_id: ToolCallId,
        result: ToolResult,
    ) -> Result<(), TurnRunError> {
        let message = tool_result_message(call, &result);
        self.request
            .storage
            .append_tool_result(&self.request.turn_id, &message)
            .await
            .map_err(TurnRunError::Persistence)?;
        self.request.chat.append_tool_result(message).await?;
        self.update(SessionUpdate::ToolCallFinished {
            tool_call_id,
            provider_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            status: tool_status_name(result.status).to_string(),
            output: result.text_content(),
            is_error: result.is_error(),
        })
        .await
    }

    fn is_doom_loop(&mut self, name: &str, canonical_input: &str) -> bool {
        let next_count = match &self.repeated_tool {
            Some((previous_name, previous_input, count))
                if previous_name == name && previous_input == canonical_input =>
            {
                count + 1
            }
            _ => 1,
        };
        self.repeated_tool = Some((name.to_string(), canonical_input.to_string(), next_count));
        next_count >= self.request.agent.policy().doom_loop_threshold
    }

    fn ensure_not_cancelled(&self) -> Result<(), TurnRunError> {
        if self.request.cancel.is_cancelled() {
            Err(TurnRunError::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn update(&self, update: SessionUpdate) -> Result<(), TurnRunError> {
        self.request
            .events
            .send(RunnerEvent::Update {
                turn_id: self.request.turn_id.clone(),
                update,
            })
            .await
            .map_err(|_| TurnRunError::ActorStopped)
    }
}

fn tool_progress_update(progress: RuntimeToolProgress) -> ToolProgressUpdate {
    match progress {
        RuntimeToolProgress::Stdout { chunk } => ToolProgressUpdate::Stdout { chunk },
        RuntimeToolProgress::Stderr { chunk } => ToolProgressUpdate::Stderr { chunk },
        RuntimeToolProgress::Message { message } => ToolProgressUpdate::Message { message },
    }
}

fn assistant_message(response: &ModelResponse) -> Message {
    let mut content = Vec::new();
    if let Some(reasoning) = response
        .reasoning_text
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        content.push(ContentBlock::thinking(reasoning));
    }
    if !response.text.is_empty() {
        content.push(ContentBlock::text(&response.text));
    }
    content.extend(
        response
            .provider_opaque_blocks
            .iter()
            .cloned()
            .map(ContentBlock::ProviderOpaque),
    );
    content.extend(
        response
            .tool_calls
            .iter()
            .cloned()
            .map(ContentBlock::ToolCall),
    );
    Message {
        role: Role::Assistant,
        content,
    }
}

fn tool_result_message(call: &ToolCallBlock, result: &ToolResult) -> Message {
    let output = result
        .content
        .iter()
        .map(|content| match content {
            ToolResultContent::Text { text } => ContentBlock::text(text),
        })
        .collect();
    let state = match result.status {
        ToolResultStatus::Succeeded => ToolResultState::Success,
        ToolResultStatus::Failed | ToolResultStatus::OutcomeUnknown => ToolResultState::Error,
        ToolResultStatus::Denied => ToolResultState::Denied,
        ToolResultStatus::Cancelled => ToolResultState::Interrupted,
    };
    Message {
        role: Role::Tool,
        content: vec![ContentBlock::ToolResult(ToolResultBlock {
            id: call.id.clone(),
            name: call.name.clone(),
            output,
            state,
        })],
    }
}

fn tool_status_name(status: ToolResultStatus) -> &'static str {
    match status {
        ToolResultStatus::Succeeded => "succeeded",
        ToolResultStatus::Failed => "failed",
        ToolResultStatus::Denied => "denied",
        ToolResultStatus::Cancelled => "cancelled",
        ToolResultStatus::OutcomeUnknown => "outcome_unknown",
    }
}

fn trace_status_for_tool(status: ToolResultStatus) -> TraceStatus {
    match status {
        ToolResultStatus::Succeeded => TraceStatus::Succeeded,
        ToolResultStatus::Failed => TraceStatus::Failed,
        ToolResultStatus::Denied => TraceStatus::Denied,
        ToolResultStatus::Cancelled => TraceStatus::Cancelled,
        ToolResultStatus::OutcomeUnknown => TraceStatus::OutcomeUnknown,
    }
}

fn trace_status_for_error(error: &TurnRunError) -> TraceStatus {
    match error {
        TurnRunError::Cancelled => TraceStatus::Cancelled,
        TurnRunError::ActorStopped => TraceStatus::OutcomeUnknown,
        _ => TraceStatus::Failed,
    }
}

fn tool_error_code(code: ToolErrorCode) -> &'static str {
    match code {
        ToolErrorCode::ToolNotFound => "tool_not_found",
        ToolErrorCode::InvalidArguments => "invalid_arguments",
        ToolErrorCode::PermissionDenied => "permission_denied",
        ToolErrorCode::Cancelled => "cancelled",
        ToolErrorCode::Timeout => "timeout",
        ToolErrorCode::ExecutionFailed => "execution_failed",
        ToolErrorCode::OutcomeUnknown => "outcome_unknown",
    }
}

fn trace_id(kind: &str) -> String {
    format!("span-{kind}-{}", Uuid::new_v4().simple())
}

fn elapsed_millis(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

#[derive(Debug, thiserror::Error)]
enum TurnRunError {
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("chat state error: {0}")]
    Chat(#[from] ChatStateError),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("model protocol error: {0}")]
    Protocol(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("doom loop detected: {0}")]
    DoomLoop(String),
    #[error("maximum model calls exceeded: {0}")]
    MaxModelCalls(u32),
    #[error("turn cancelled")]
    Cancelled,
    #[error("session actor stopped")]
    ActorStopped,
}

impl TurnRunError {
    fn into_outcome(self) -> TurnOutcome {
        match self {
            Self::Cancelled => TurnOutcome::Cancelled,
            error => TurnOutcome::Failed {
                code: error.code().to_string(),
                message: error.to_string(),
            },
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Model(_) => "model_error",
            Self::Chat(_) => "chat_state_error",
            Self::Persistence(_) => "persistence_error",
            Self::Protocol(_) => "model_protocol_error",
            Self::PermissionDenied(_) => "permission_denied",
            Self::DoomLoop(_) => "doom_loop",
            Self::MaxModelCalls(_) => "max_model_calls",
            Self::Cancelled => "cancelled",
            Self::ActorStopped => "actor_stopped",
        }
    }
}
