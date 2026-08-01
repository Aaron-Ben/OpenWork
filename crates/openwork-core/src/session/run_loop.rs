use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use futures_util::StreamExt;
use openwork_agent::Agent;
use openwork_chat_state::{ChatStateError, ChatStateHandle};
use openwork_models::model::{
    ContentBlock, DeliveryState, Message, ModelCallOptions, ModelError, ModelErrorCode, ModelEvent,
    ModelPort, ModelResponse, Role, ToolCallBlock, ToolResultBlock, ToolResultState,
};
use openwork_tools::{
    Authorization, DecisionSource, FinalizedToolset, ToolCallContext as RuntimeToolCallContext,
    ToolCallId as RuntimeToolCallId, ToolErrorCode, ToolInvocation,
    ToolProgress as RuntimeToolProgress, ToolResult, ToolResultContent, ToolResultStatus,
    ToolValidationError,
};
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::context::{ResolvedSystemContext, SystemContextBuildError, SystemContextBuilder};
use crate::model_call::{ModelRequestBuilder, ModelRequestInput};

use super::compaction::{
    AutomaticCompactionPolicy, CompactionTrigger, ConversationCompactionRequest, run_compaction,
};
use super::permission_state::SessionPermissionState;
use super::{
    ClientRequestId, CompactionStateCollector, LiveToolCall, ModelCallStarted, ModelCallTraceGuard,
    ModelTraceAttributesV1, PermissionDecision, PermissionRequest, ResolvedModel, SessionId,
    SessionPhase, SessionStorage, SessionUpdate, ToolCallId, ToolCallStarted, ToolCallTraceGuard,
    ToolProgressUpdate, ToolTraceAttributesV1, TracePayloads, TraceRecorder, TraceStatus, TurnId,
    TurnOutcome,
};

pub(super) struct TurnRunRequest {
    pub session_id: SessionId,
    pub working_directory: PathBuf,
    pub turn_id: TurnId,
    pub client_request_id: ClientRequestId,
    pub input: Vec<ContentBlock>,
    pub resolved_model: ResolvedModel,
    pub agent: Agent,
    pub chat: ChatStateHandle,
    pub model: Arc<dyn ModelPort>,
    pub tools: Arc<FinalizedToolset>,
    pub storage: Arc<dyn SessionStorage>,
    pub compaction_state: Arc<CompactionStateCollector>,
    pub compaction_policy: AutomaticCompactionPolicy,
    pub reload_required: Arc<AtomicBool>,
    pub trace: Arc<dyn TraceRecorder>,
    pub cancel: CancellationToken,
    pub events: mpsc::Sender<RunnerEvent>,
    pub permission_state: watch::Receiver<SessionPermissionState>,
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
        last_model_call: None,
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
    /// The most recent Model Call submission. An overflow compaction is caused
    /// by a Model Call that already failed, so its Span and input estimate are
    /// no longer reachable through the call's return value.
    last_model_call: Option<SubmittedModelCall>,
}

#[derive(Debug, Clone)]
struct SubmittedModelCall {
    trace_span_id: String,
    estimated_input_tokens: u64,
}

struct CompletedModelCall {
    response: ModelResponse,
    trace_span_id: String,
    trace: ModelCallTraceGuard,
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
        self.ensure_not_cancelled()?;
        let system_context = SystemContextBuilder::new(&self.request.working_directory)
            .build(self.request.agent.system_prompt())
            .await?;
        for model_call_index in 1..=self.request.agent.policy().max_model_calls {
            self.ensure_not_cancelled()?;
            self.update(SessionUpdate::PhaseChanged {
                phase: SessionPhase::RunningModel,
            })
            .await?;

            let threshold_estimate = self
                .threshold_estimate_before_sampling(&system_context)
                .await?;
            let compacted_before_sampling = match threshold_estimate {
                Some(estimated_input_tokens) => {
                    self.compact(
                        &system_context,
                        CompactionTrigger::Threshold {
                            turn_id: self.request.turn_id.clone(),
                            policy: self.request.compaction_policy,
                            estimated_input_tokens,
                        },
                    )
                    .await?;
                    true
                }
                None => false,
            };
            let completed_model = match self.call_model(model_call_index, 1, &system_context).await
            {
                Ok(completed) => completed,
                Err(error) if !compacted_before_sampling && is_safe_context_overflow(&error) => {
                    self.update(SessionUpdate::DraftCleared).await?;
                    let trigger = self.overflow_trigger(&error);
                    self.compact(&system_context, trigger).await?;
                    self.call_model(model_call_index, 2, &system_context)
                        .await?
                }
                Err(error) => return Err(error),
            };
            let CompletedModelCall {
                response,
                trace_span_id,
                trace: model_trace,
            } = completed_model;
            let assistant_message = assistant_message(&response);
            let response_message_id = match self
                .request
                .storage
                .append_assistant_message(&self.request.turn_id, &assistant_message, response.usage)
                .await
            {
                Ok(message_id) => message_id,
                Err(message) => {
                    model_trace.finish_success(&response);
                    return Err(TurnRunError::Persistence(message));
                }
            };
            model_trace.finish_success_with_message(&response, response_message_id);
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
                if let Err(error) = self.run_tool_call(tool_call, &trace_span_id).await {
                    for pending in response.tool_calls.iter().skip(index + 1) {
                        self.append_cancelled_tool_result(
                            pending,
                            &trace_span_id,
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

    async fn call_model(
        &mut self,
        model_call_index: u32,
        submission_attempt: u8,
        system_context: &ResolvedSystemContext,
    ) -> Result<CompletedModelCall, TurnRunError> {
        let request_build_started = Instant::now();
        let conversation = self.request.chat.conversation_view().await?;
        let prepared = ModelRequestBuilder::build(ModelRequestInput::new(
            &self.request.resolved_model.model_name,
            system_context,
            conversation,
            self.request.tools.definitions(),
        ))
        .map_err(|error| TurnRunError::Protocol(error.to_string()))?;
        let request = prepared.request;
        let request_build_ms = elapsed_millis_u64(request_build_started);
        self.request
            .storage
            .begin_model_call(&self.request.turn_id, model_call_index, submission_attempt)
            .await
            .map_err(TurnRunError::Persistence)?;
        self.request.chat.begin_draft().await?;
        let mut attributes =
            ModelTraceAttributesV1::from_request(model_call_index, request_build_ms, &request);
        attributes.record_context_budget(prepared.context_budget);
        let options = ModelCallOptions::new(format!(
            "{}-model-{model_call_index}-submission-{submission_attempt}",
            self.request.turn_id
        ));
        let max_transport_attempts = options.max_transport_attempts;
        let mut model_trace = ModelCallTraceGuard::start(
            Arc::clone(&self.request.trace),
            ModelCallStarted {
                span_id: span_id("model"),
                trace_id: self.trace_id().to_string(),
                session_id: self.request.session_id.clone(),
                turn_id: Some(self.request.turn_id.clone()),
                parent_span_id: None,
                model_id: self.request.resolved_model.model_id.clone(),
                resolved_model_name: self.request.resolved_model.model_name.clone(),
                started_at: OffsetDateTime::now_utc(),
                attributes,
                payloads: TracePayloads::for_model_call(&request, system_context),
            },
            self.request.cancel.clone(),
            max_transport_attempts,
        );
        let trace_span_id = model_trace.span_id().to_string();
        self.last_model_call = Some(SubmittedModelCall {
            trace_span_id: trace_span_id.clone(),
            estimated_input_tokens: prepared.context_budget.estimated_input_tokens,
        });
        let options = options.with_transport_observer(model_trace.transport_observer());
        let result = self
            .invoke_and_consume_model(request, options, &mut model_trace)
            .await;
        match result {
            Ok(response) => Ok(CompletedModelCall {
                response,
                trace_span_id,
                trace: model_trace,
            }),
            Err(error) => {
                let trace_model_error = match &error {
                    TurnRunError::Model(error) => Some(error.clone()),
                    TurnRunError::Protocol(message) => Some(ModelError::protocol(message.clone())),
                    _ => None,
                };
                let provider_request_id = trace_model_error
                    .as_ref()
                    .and_then(|error| error.provider_request_id.clone());
                model_trace.finish_failure(
                    trace_status_for_error(&error),
                    provider_request_id,
                    error.code(),
                    error.to_string(),
                    trace_model_error.as_ref(),
                );
                Err(error)
            }
        }
    }

    /// The pre-sampling input estimate when it has reached the compaction
    /// threshold, `None` otherwise. The estimate is returned rather than a bare
    /// bool so the compaction Span can record what actually tripped it.
    async fn threshold_estimate_before_sampling(
        &self,
        system_context: &ResolvedSystemContext,
    ) -> Result<Option<u64>, TurnRunError> {
        let conversation = self.request.chat.conversation_view().await?;
        let prepared = ModelRequestBuilder::build(ModelRequestInput::new(
            &self.request.resolved_model.model_name,
            system_context,
            conversation,
            self.request.tools.definitions(),
        ))
        .map_err(|error| TurnRunError::Protocol(error.to_string()))?;
        let estimated_input_tokens = prepared.context_budget.estimated_input_tokens;
        Ok(self
            .request
            .compaction_policy
            .should_compact(estimated_input_tokens)
            .then_some(estimated_input_tokens))
    }

    /// Build the trigger for a compaction forced by a context overflow, keeping
    /// the failed Model Call Span and its input estimate as the recorded cause.
    fn overflow_trigger(&self, error: &TurnRunError) -> CompactionTrigger {
        let submitted = self.last_model_call.clone();
        CompactionTrigger::Overflow {
            turn_id: self.request.turn_id.clone(),
            policy: self.request.compaction_policy,
            estimated_input_tokens: submitted.as_ref().map(|call| call.estimated_input_tokens),
            model_span_id: submitted.map(|call| call.trace_span_id),
            error_code: error.code().to_string(),
        }
    }

    async fn compact(
        &mut self,
        system_context: &ResolvedSystemContext,
        trigger: CompactionTrigger,
    ) -> Result<(), TurnRunError> {
        self.update(SessionUpdate::PhaseChanged {
            phase: SessionPhase::Compacting,
        })
        .await?;
        run_compaction(ConversationCompactionRequest {
            session_id: self.request.session_id.clone(),
            model_id: self.request.resolved_model.model_id.clone(),
            resolved_model_name: self.request.resolved_model.model_name.clone(),
            working_directory: self.request.working_directory.clone(),
            agent: self.request.agent.clone(),
            chat: self.request.chat.clone(),
            model: Arc::clone(&self.request.model),
            storage: Arc::clone(&self.request.storage),
            state_collector: Arc::clone(&self.request.compaction_state),
            reload_required: Arc::clone(&self.request.reload_required),
            trigger,
            system_context: Some(system_context.clone()),
            trace: Arc::clone(&self.request.trace),
            trace_id: self.trace_id().to_string(),
            cancellation: self.request.cancel.clone(),
        })
        .await
        .map(|_| ())
        .map_err(|error| TurnRunError::Compaction(error.to_string()))?;
        self.update(SessionUpdate::PhaseChanged {
            phase: SessionPhase::RunningModel,
        })
        .await?;
        Ok(())
    }

    async fn invoke_and_consume_model(
        &self,
        request: openwork_models::model::ModelRequest,
        options: ModelCallOptions,
        model_trace: &mut ModelCallTraceGuard,
    ) -> Result<ModelResponse, TurnRunError> {
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
        let mut semantic_output_emitted = false;

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
                    return Err(TurnRunError::Model(if semantic_output_emitted {
                        error.with_delivery(DeliveryState::SemanticOutputEmitted)
                    } else {
                        error
                    }));
                }
            };
            if is_first_semantic_event(&event) {
                semantic_output_emitted = true;
                model_trace.record_first_semantic_event();
            }
            model_trace.record_response_event(&event);
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
        model_trace.record_stream_finished();
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
        let mut tool_trace = self.start_tool_trace(call, parent_span_id);
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
                return self
                    .append_tool_result(call, tool_call_id, result, tool_trace)
                    .await;
            }
        };
        self.start_live_tool(call, &tool_call_id, input.clone())
            .await?;

        let invocation = ToolInvocation::new(&call.name, input.clone());
        let resolved_tool_name = match self.request.tools.validate(&invocation) {
            Ok(definition) => {
                let name = definition.id.to_string();
                tool_trace.set_resolved_tool_name(&name);
                name
            }
            Err(error) => {
                let code = match error {
                    ToolValidationError::UnknownTool(_) => ToolErrorCode::ToolNotFound,
                    ToolValidationError::InvalidInput(_) => ToolErrorCode::InvalidArguments,
                };
                let result = ToolResult::failed(code, error.to_string(), false);
                return self
                    .append_tool_result(call, tool_call_id, result, tool_trace)
                    .await;
            }
        };

        let canonical_input = serde_json::to_string(&input).unwrap_or_else(|_| call.input.clone());
        if self.is_doom_loop(&call.name, &canonical_input) {
            let result = ToolResult::failed(
                ToolErrorCode::ExecutionFailed,
                format!("doom loop detected for tool '{}'", call.name),
                false,
            );
            self.append_tool_result(call, tool_call_id, result, tool_trace)
                .await?;
            return Err(TurnRunError::DoomLoop(call.name.clone()));
        }

        let permission_state = self.request.permission_state.borrow().clone();
        tool_trace.record_permission_mode(
            match permission_state.mode() {
                openwork_tools::PermissionMode::Default => "default",
                openwork_tools::PermissionMode::AcceptEdits => "accept_edits",
            },
            permission_state.mode_origin().as_str(),
        );
        let permit = match self.request.tools.authorize(
            &invocation,
            permission_state.mode(),
            permission_state.session_rules(),
        ) {
            Authorization::Allow { permit, evidence } => {
                tool_trace.record_permission_policy("allow");
                let source = match evidence.source {
                    DecisionSource::ReadonlyProof => "readonly_proof",
                    DecisionSource::ModeFsCommand => "mode_fs_command",
                    DecisionSource::SessionGrant => "session_grant",
                    DecisionSource::Rule => "rule",
                    DecisionSource::Builtin | DecisionSource::Mode => "builtin",
                };
                tool_trace.record_permission_decision("allow", source);
                if let (Some(rule_id), Some(rule_scope)) =
                    (evidence.rule_id.as_ref(), evidence.rule_scope)
                {
                    tool_trace.record_permission_rule(rule_id.as_str(), rule_scope.as_str());
                }
                if let Some(key) = evidence.readonly_proof_key.as_deref() {
                    tool_trace.record_readonly_proof(key);
                }
                permit
            }
            Authorization::Deny {
                reason,
                rule_id,
                rule_scope,
                silent,
            } => {
                tool_trace.record_permission_policy("deny");
                // permissions.md §7: built-in denials are sourced to `builtin`,
                // everything else to the rule that produced them.
                tool_trace
                    .record_permission_decision("deny", if silent { "builtin" } else { "rule" });
                tool_trace.record_permission_rule(rule_id.as_str(), rule_scope.as_str());
                let result = ToolResult::denied(reason.clone());
                self.append_tool_result(call, tool_call_id, result, tool_trace)
                    .await?;
                return Ok(());
            }
            Authorization::Unavailable { code, message } => {
                // Not a permission verdict — the call could not be judged at
                // all, so it must not read as "a rule closed this path".
                let result = ToolResult::failed(code, message, false);
                return self
                    .append_tool_result(call, tool_call_id, result, tool_trace)
                    .await;
            }
            Authorization::Ask { card, permit } => {
                tool_trace.record_permission_policy("ask");
                let request = PermissionRequest {
                    session_id: self.request.session_id.clone(),
                    turn_id: self.request.turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    provider_call_id: call.id.clone(),
                    tool_name: resolved_tool_name.clone(),
                    card,
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
                    tool_trace.record_permission_decision("cancelled", "system");
                    let result = ToolResult::outcome_unknown("session actor stopped");
                    self.append_tool_result(call, tool_call_id, result, tool_trace)
                        .await?;
                    return Err(TurnRunError::ActorStopped);
                }
                let wait_started = Instant::now();
                let decision = tokio::select! {
                    _ = self.request.cancel.cancelled() => {
                        None
                    }
                    result = decision => result.ok(),
                };
                tool_trace.record_permission_wait_ms(elapsed_millis(wait_started));
                let Some(decision) = decision else {
                    let cancelled = self.request.cancel.is_cancelled();
                    tool_trace.record_permission_decision("cancelled", "system");
                    let result = if cancelled {
                        ToolResult::cancelled("turn cancelled while waiting for permission")
                    } else {
                        ToolResult::outcome_unknown("permission responder stopped")
                    };
                    self.append_tool_result(call, tool_call_id, result, tool_trace)
                        .await?;
                    return Err(if cancelled {
                        TurnRunError::Cancelled
                    } else {
                        TurnRunError::ActorStopped
                    });
                };
                if decision == PermissionDecision::Deny {
                    tool_trace.record_permission_decision("deny", "user");
                    let result = ToolResult::denied("user denied tool permission");
                    self.append_tool_result(call, tool_call_id, result, tool_trace)
                        .await?;
                    return Err(TurnRunError::PermissionDenied(
                        "user denied tool permission".to_string(),
                    ));
                }
                let permission_state = self.request.permission_state.borrow().clone();
                tool_trace.record_permission_mode(
                    match permission_state.mode() {
                        openwork_tools::PermissionMode::Default => "default",
                        openwork_tools::PermissionMode::AcceptEdits => "accept_edits",
                    },
                    permission_state.mode_origin().as_str(),
                );
                tool_trace.record_permission_decision("allow", "user");
                permit
            }
        };

        let (progress_tx, mut progress_rx) = mpsc::channel(64);
        let call_context = RuntimeToolCallContext::new(
            RuntimeToolCallId::new(tool_call_id.to_string()),
            self.request.cancel.child_token(),
        )
        .with_progress_sender(progress_tx);
        let tools = Arc::clone(&self.request.tools);
        let execution_started = Instant::now();
        let mut execution = Box::pin(tools.call(call_context, invocation, permit));
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
        tool_trace.record_execution_ms(elapsed_millis_u64(execution_started));
        while let Ok(progress) = progress_rx.try_recv() {
            self.update(SessionUpdate::ToolCallProgress {
                tool_call_id: tool_call_id.clone(),
                progress: tool_progress_update(progress),
            })
            .await?;
        }
        let cancelled = result.status == ToolResultStatus::Cancelled;
        self.append_tool_result(call, tool_call_id, result, tool_trace)
            .await?;
        if cancelled {
            return Err(TurnRunError::Cancelled);
        }
        Ok(())
    }

    fn start_tool_trace(
        &mut self,
        call: &ToolCallBlock,
        parent_span_id: &str,
    ) -> ToolCallTraceGuard {
        ToolCallTraceGuard::start(
            Arc::clone(&self.request.trace),
            ToolCallStarted {
                span_id: span_id("tool"),
                trace_id: self.trace_id().to_string(),
                turn_id: self.request.turn_id.clone(),
                parent_span_id: parent_span_id.to_string(),
                provider_call_id: call.id.clone(),
                requested_tool_name: call.name.clone(),
                started_at: OffsetDateTime::now_utc(),
                attributes: ToolTraceAttributesV1::new(),
            },
            self.request.cancel.clone(),
        )
    }

    /// 一个 Turn 的全部 Span 共享一个 Trace 根，直接复用 Turn 的标识。
    fn trace_id(&self) -> &str {
        self.request.turn_id.as_str()
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
                artifacts: Vec::new(),
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
        let mut tool_trace = self.start_tool_trace(call, parent_span_id);
        let input = serde_json::from_str(&call.input).unwrap_or(serde_json::Value::Null);
        tool_trace.record_permission_decision("cancelled", "system");
        self.start_live_tool(call, &tool_call_id, input).await?;
        let result = ToolResult::cancelled(reason);
        self.append_tool_result(call, tool_call_id, result, tool_trace)
            .await
    }

    async fn append_tool_result(
        &self,
        call: &ToolCallBlock,
        tool_call_id: ToolCallId,
        result: ToolResult,
        tool_trace: ToolCallTraceGuard,
    ) -> Result<(), TurnRunError> {
        let message = tool_result_message(call, &result);
        let persisted = self
            .request
            .storage
            .append_tool_result(&self.request.turn_id, &message)
            .await;
        match persisted {
            Ok(()) => tool_trace.finish_result(&result, true),
            Err(message) => {
                tool_trace.finish_result(&result, false);
                return Err(TurnRunError::Persistence(message));
            }
        }
        self.request.chat.append_tool_result(message).await?;
        self.update(SessionUpdate::ToolCallFinished {
            tool_call_id,
            provider_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            status: tool_status_name(result.status).to_string(),
            output: result.text_content(),
            is_error: result.is_error(),
            artifacts: result.artifacts.clone(),
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
            artifacts: result.artifacts.clone(),
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

fn is_first_semantic_event(event: &ModelEvent) -> bool {
    match event {
        ModelEvent::TextStart { .. }
        | ModelEvent::TextDelta { .. }
        | ModelEvent::ReasoningStart { .. }
        | ModelEvent::ReasoningDelta { .. }
        | ModelEvent::ToolCallStart { .. }
        | ModelEvent::ToolCallDelta { .. } => true,
        ModelEvent::ResponseCompleted { response } => {
            !response.text.is_empty()
                || response
                    .reasoning_text
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                || !response.tool_calls.is_empty()
        }
        ModelEvent::TextEnd { .. }
        | ModelEvent::ReasoningEnd { .. }
        | ModelEvent::ToolCallEnd { .. } => false,
    }
}

fn trace_status_for_error(error: &TurnRunError) -> TraceStatus {
    match error {
        TurnRunError::Cancelled => TraceStatus::Cancelled,
        TurnRunError::ActorStopped => TraceStatus::OutcomeUnknown,
        _ => TraceStatus::Failed,
    }
}

fn is_safe_context_overflow(error: &TurnRunError) -> bool {
    matches!(
        error,
        TurnRunError::Model(ModelError {
            kind: ModelErrorCode::ContextOverflow,
            delivery: DeliveryState::NotSent
                | DeliveryState::PossiblySent
                | DeliveryState::AcceptedNoSemanticOutput,
            ..
        })
    )
}

fn span_id(kind: &str) -> String {
    format!("span-{kind}-{}", Uuid::new_v4().simple())
}

fn elapsed_millis(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

fn elapsed_millis_u64(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[derive(Debug, thiserror::Error)]
enum TurnRunError {
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("chat state error: {0}")]
    Chat(#[from] ChatStateError),
    #[error("context build error: {0}")]
    Context(#[from] SystemContextBuildError),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("model protocol error: {0}")]
    Protocol(String),
    #[error("conversation compaction failed: {0}")]
    Compaction(String),
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
            Self::Context(error) => error.code(),
            Self::Persistence(_) => "persistence_error",
            Self::Protocol(_) => "model_protocol_error",
            Self::Compaction(_) => "compaction_error",
            Self::PermissionDenied(_) => "permission_denied",
            Self::DoomLoop(_) => "doom_loop",
            Self::MaxModelCalls(_) => "max_model_calls",
            Self::Cancelled => "cancelled",
            Self::ActorStopped => "actor_stopped",
        }
    }
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{
        DeliveryState, ModelError, ModelErrorCode, ModelFailurePhase, RetryHint,
    };

    use super::{TurnRunError, is_safe_context_overflow};

    #[test]
    fn only_context_overflow_without_semantic_output_can_compact_and_resubmit() {
        let safe = TurnRunError::Model(ModelError::context_overflow("too large"));
        assert!(is_safe_context_overflow(&safe));

        let emitted = TurnRunError::Model(ModelError::new(
            ModelErrorCode::ContextOverflow,
            ModelFailurePhase::StreamDecode,
            DeliveryState::SemanticOutputEmitted,
            RetryHint::CallerDecision,
            "too large after output",
        ));
        assert!(!is_safe_context_overflow(&emitted));

        let invalid = TurnRunError::Model(ModelError::invalid_request("bad request"));
        assert!(!is_safe_context_overflow(&invalid));
    }
}
