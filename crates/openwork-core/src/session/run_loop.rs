use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use futures_util::StreamExt;
use openwork_agent::Agent;
use openwork_chat_state::{ChatStateError, ChatStateHandle, MessageKind};
use openwork_models::model::{
    ContentBlock, DeliveryState, Message, ModelCallOptions, ModelError, ModelErrorCode, ModelEvent,
    ModelPort, ModelResponse, Role, ToolCallBlock, ToolResultBlock, ToolResultState,
};
use openwork_tools::{
    Authorization, DecisionSource, ToolCallContext as RuntimeToolCallContext,
    ToolCallId as RuntimeToolCallId, ToolErrorCode, ToolInvocation,
    ToolProgress as RuntimeToolProgress, ToolResult, ToolResultContent, ToolResultStatus,
    ToolValidationError,
};
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::AgentControl;
use crate::agent::{
    AgentTool, FollowupTaskArgs, InterruptAgentArgs, NoArgs, SpawnAgentArgs, WaitAgentArgs,
    parse_args as parse_agent_args, validate_wait_timeout,
};
use crate::context::{ResolvedSystemContext, SystemContextBuildError, SystemContextBuilder};
use crate::model_call::{ModelRequestBuilder, ModelRequestInput};
use crate::plan::{
    TurnPlan, UPDATE_PLAN_TOOL_NAME, parse_update_plan_arguments, update_plan_success_output,
    validate_args,
};
use crate::skills::SkillRoots;
use crate::storage::time::china_now;

use super::toolset::{ResolvedTurnTool, TurnToolset};

use super::agent_message::AgentMailbox;
use super::compaction::{
    AutomaticCompactionPolicy, CompactionTrigger, ConversationCompactionRequest, run_compaction,
};
use super::permission_state::{NON_INTERACTIVE_DENIAL, SessionApproval, SessionPermissionState};
use super::{
    ClientRequestId, CompactionStateCollector, LiveToolCall, ModelCallStarted, ModelCallTraceGuard,
    ModelTraceAttributesV1, PermissionDecision, PermissionRequest, PreparedTurnInput,
    ResolvedModel, SessionId, SessionPhase, SessionStorage, SessionUpdate, ToolCallId,
    ToolCallStarted, ToolCallTraceGuard, ToolProgressUpdate, ToolTraceAttributesV1, TracePayloads,
    TraceRecorder, TraceStatus, TurnId, TurnOutcome,
};

pub(super) struct TurnRunRequest {
    pub session_id: SessionId,
    pub working_directory: PathBuf,
    pub skill_roots: SkillRoots,
    pub disabled_skill_names: BTreeSet<String>,
    pub turn_id: TurnId,
    pub client_request_id: ClientRequestId,
    pub input: PreparedTurnInput,
    pub resolved_model: ResolvedModel,
    pub agent: Agent,
    pub chat: ChatStateHandle,
    pub model: Arc<dyn ModelPort>,
    pub tools: Arc<TurnToolset>,
    pub storage: Arc<dyn SessionStorage>,
    pub compaction_state: Arc<CompactionStateCollector>,
    pub compaction_policy: AutomaticCompactionPolicy,
    pub reload_required: Arc<AtomicBool>,
    pub trace: Arc<dyn TraceRecorder>,
    pub cancel: CancellationToken,
    pub events: mpsc::Sender<RunnerEvent>,
    pub permission_state: watch::Receiver<SessionPermissionState>,
    /// `NonInteractive` turns every `Ask` into an immediate denial.
    pub approval: SessionApproval,
    pub mailbox: AgentMailbox,
    pub agent_control: Option<AgentControl>,
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
        consecutive_wait_timeouts: 0,
        last_model_call: None,
        // 新 Turn 从无计划开始，不继承上一个 Turn 的计划。
        current_plan: None,
    };

    let outcome = match runner.begin().await {
        Ok(()) => {
            let outcome = match runner.run_loop().await {
                Ok(final_text) => TurnOutcome::Completed { final_text },
                Err(error) => error.into_outcome(),
            };
            // §15.1 的观测信号。这里读的是 Turn 真正结束那一刻的计划，不管 Turn 是
            // 完成、失败还是被取消 —— 后两种留下未完成步骤是正确的，靠 turns.status
            // 在查询时区分，而不是在这里提前过滤掉。
            let unfinished_plan_steps = runner.unfinished_plan_steps();
            match storage
                .finish_turn(&turn_id, &outcome, unfinished_plan_steps)
                .await
            {
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
    /// wait_agent 的同参重复本身合法，只有连续超时才说明父 Turn 没有取得进展。
    consecutive_wait_timeouts: usize,
    /// The most recent Model Call submission. An overflow compaction is caused
    /// by a Model Call that already failed, so its Span and input estimate are
    /// no longer reachable through the call's return value.
    last_model_call: Option<SubmittedModelCall>,
    /// 这个 Turn 的当前计划，由成功的 `update_plan` 调用维护。
    ///
    /// 压缩要在 Turn 中途把它重新投影给模型，直接从这里取，不回查数据库：提交路径已经
    /// 保证了内存与 `turn_plans` 一致，而压缩正好可能把原来的 Tool Call 移出投影。
    current_plan: Option<TurnPlan>,
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
        self.request
            .storage
            .begin_turn(
                &self.request.session_id,
                &self.request.turn_id,
                &self.request.client_request_id,
                &self.request.resolved_model,
                self.request.input.contextual_messages(),
                self.request.input.user_message(),
            )
            .await
            .map_err(TurnRunError::Persistence)?;
        for (kind, message) in self.request.input.clone().into_messages_with_kind() {
            self.request
                .chat
                .append_user_with_kind(message.content, kind)
                .await?;
        }
        Ok(())
    }

    /// Turn 收尾时还没标成 `completed` 的计划步骤数。
    ///
    /// `None` 表示这个 Turn 从头到尾没建过计划 —— 简单任务本来就不该建，那是正常的。
    /// 它与 `Some(0)`（建了并且全部收尾）必须分开，否则统计"规则生效率"时分母就错了。
    fn unfinished_plan_steps(&self) -> Option<usize> {
        self.current_plan
            .as_ref()
            .map(TurnPlan::unfinished_step_count)
    }

    /// Agent 的系统提示，外加当前工具面所需的行为规则。
    ///
    /// 计划规则跟着工具走：`update_plan` 没被广告时不能出现，否则提示里会讲一个模型
    /// 调用不到的工具。
    fn system_prompt(&self) -> String {
        self.request
            .tools
            .system_prompt(self.request.agent.system_prompt())
    }

    async fn run_loop(&mut self) -> Result<String, TurnRunError> {
        self.ensure_not_cancelled()?;
        let system_context = SystemContextBuilder::new(
            &self.request.working_directory,
            self.request.skill_roots.clone(),
        )
        .with_disabled_skills(self.request.disabled_skill_names.clone())
        .build(&self.system_prompt())
        .await?;
        for model_call_index in 1..=self.request.agent.policy().max_model_calls {
            self.ensure_not_cancelled()?;
            self.drain_agent_messages().await?;
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

    async fn drain_agent_messages(&self) -> Result<(), TurnRunError> {
        while let Some(delivered) = self.request.mailbox.front().await {
            let message_id = delivered.id.clone();
            let message = delivered.into_model_message();
            let inserted = self
                .request
                .storage
                .append_agent_message(&self.request.turn_id, &message_id, &message)
                .await
                .map_err(TurnRunError::Persistence)?;
            if inserted {
                self.request
                    .chat
                    .append_user_with_kind(message.content, MessageKind::AgentMessage)
                    .await?;
            }
            self.request.mailbox.pop_front().await;
        }
        Ok(())
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
            skill_roots: self.request.skill_roots.clone(),
            disabled_skill_names: self.request.disabled_skill_names.clone(),
            agent: self.request.agent.clone(),
            chat: self.request.chat.clone(),
            model: Arc::clone(&self.request.model),
            storage: Arc::clone(&self.request.storage),
            state_collector: Arc::clone(&self.request.compaction_state),
            plan: self.current_plan.clone(),
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

        // 一次解析，之后按类型分派。控制工具与普通工具从这里开始走两条路：前者提交 Turn
        // 状态，后者经 PermissionEngine 访问主机能力。后续步骤不再比较工具名字符串。
        let resolved = self.request.tools.resolve(&call.name);
        let is_control_tool = matches!(
            &resolved,
            Some(ResolvedTurnTool::UpdatePlan | ResolvedTurnTool::Agent(_))
        );
        let resolved_tool_name = match &resolved {
            Some(ResolvedTurnTool::UpdatePlan) => UPDATE_PLAN_TOOL_NAME.to_string(),
            Some(ResolvedTurnTool::Agent(tool)) => tool.name().to_string(),
            Some(ResolvedTurnTool::Registered(id)) => id.to_string(),
            None => {
                let error = ToolValidationError::UnknownTool(call.name.clone());
                let result =
                    ToolResult::failed(ToolErrorCode::ToolNotFound, error.to_string(), false);
                return self
                    .append_tool_result(call, tool_call_id, result, tool_trace)
                    .await;
            }
        };
        tool_trace.set_resolved_tool_name(&resolved_tool_name);

        // 入参校验只对普通工具走注册表的 schema；控制工具的参数由 plan 模块自己解析，
        // 因为它要区分"JSON 不合法"和"违反领域不变量"两种失败。
        if !is_control_tool
            && let Err(error) = self.request.tools.registered().validate(&invocation)
        {
            let code = match error {
                ToolValidationError::UnknownTool(_) => ToolErrorCode::ToolNotFound,
                ToolValidationError::InvalidInput(_) => ToolErrorCode::InvalidArguments,
            };
            let result = ToolResult::failed(code, error.to_string(), false);
            return self
                .append_tool_result(call, tool_call_id, result, tool_trace)
                .await;
        }

        let canonical_input = serde_json::to_string(&input).unwrap_or_else(|_| call.input.clone());
        let is_wait_agent = matches!(&resolved, Some(ResolvedTurnTool::Agent(AgentTool::Wait)));
        // wait_agent 的同参重复由外部子 Agent 的完成节奏驱动，入参相同不代表没有进展。
        // 它仍要打断其他工具的连续计数，避免 read → wait → read 被误算成连续 read。
        if is_wait_agent {
            self.repeated_tool = None;
        } else {
            self.consecutive_wait_timeouts = 0;
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
        }

        let permission_state = self.request.permission_state.borrow().clone();
        tool_trace.record_permission_mode(
            match permission_state.mode() {
                openwork_tools::PermissionMode::Default => "default",
                openwork_tools::PermissionMode::AcceptEdits => "accept_edits",
            },
            permission_state.mode_origin().as_str(),
        );

        // 控制工具在这里分流：它不访问主机能力，也不改工作区，所以不进 PermissionEngine。
        // 免审批由这条类型化分支表达，而不是给它伪造一个 ToolRisk::ReadOnly——后者会让
        // 权限日志把"Core 控制工具"和"被内置规则放行的只读工具"混为一谈。
        match resolved.expect("unknown tools returned above") {
            ResolvedTurnTool::UpdatePlan => {
                return self
                    .run_update_plan(call, tool_call_id, &input, tool_trace)
                    .await;
            }
            ResolvedTurnTool::Agent(tool) => {
                return self
                    .run_agent_tool(call, tool_call_id, tool, &input, tool_trace)
                    .await;
            }
            ResolvedTurnTool::Registered(_) => {}
        }
        let permit = match self.request.tools.registered().authorize(
            &invocation,
            permission_state.mode(),
            permission_state.session_rules(),
        ) {
            Authorization::Allow { permit, evidence } => {
                tool_trace.record_permission_policy("allow");
                // `Mode` must not collapse into `Builtin`: permissions.md §7 exists
                // to answer "为什么这条没问我就跑了", and the two answers differ —
                // `builtin` means it was always allowed, `mode` means it ran because
                // acceptEdits was switched on. Merging them hides the one an incident
                // review would ask about first.
                let source = match evidence.source {
                    DecisionSource::ReadonlyProof => "readonly_proof",
                    DecisionSource::ModeFsCommand => "mode_fs_command",
                    DecisionSource::SessionGrant => "session_grant",
                    DecisionSource::Mode => "mode",
                    DecisionSource::Builtin => "builtin",
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
                // An unattended Session has nobody to ask. Suspending here would
                // hang the sub-agent until the parent Turn is cancelled, so the
                // call is denied straight away — but the Turn continues, exactly
                // like a rule-based `Deny`, so the model can switch to a
                // provably read-only command. See permissions.md §6.6.
                if !self.request.approval.is_interactive() {
                    // §7 still applies: the denial has to be reconstructable, or
                    // "why did the explorer find nothing" is unanswerable.
                    tool_trace.record_permission_decision("deny", "non_interactive");
                    let result = ToolResult::denied(NON_INTERACTIVE_DENIAL);
                    self.append_tool_result(call, tool_call_id, result, tool_trace)
                        .await?;
                    return Ok(());
                }
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
        let tools = Arc::clone(self.request.tools.registered());
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

    /// 执行一次 `update_plan`。
    ///
    /// 顺序（见 `docs/update-plan.md` §7）：解析校验 → 单事务提交计划与成功 Tool Result
    /// → Chat State → 内存中的 current_plan → PlanUpdated → ToolCallFinished。
    /// 任何一步失败都不能留下"计划变了但历史里没有对应结果"的状态。
    async fn run_update_plan(
        &mut self,
        call: &ToolCallBlock,
        tool_call_id: ToolCallId,
        input: &serde_json::Value,
        mut tool_trace: ToolCallTraceGuard,
    ) -> Result<(), TurnRunError> {
        tool_trace.record_permission_policy("allow");
        // `control_tool` 是一个独立的来源，不复用 `builtin`：事故复盘要能区分
        // "它是 Core 控制工具，本来就不过权限"和"它被一条内置规则放行了"。
        tool_trace.record_permission_decision("allow", "control_tool");

        let plan = match parse_update_plan_arguments(input).and_then(|args| {
            validate_args(&self.request.turn_id, args, china_now())
                .map_err(|error| error.to_string())
        }) {
            Ok(plan) => plan,
            Err(message) => {
                // 校验失败必须整体失败：不写 turn_plans，已有计划保持不变，模型拿到一条
                // 说明得够清楚、能据此改正重试的失败结果。
                let result = ToolResult::failed(ToolErrorCode::InvalidArguments, message, false);
                return self
                    .append_tool_result(call, tool_call_id, result, tool_trace)
                    .await;
            }
        };

        let result = ToolResult::succeeded(update_plan_success_output());
        let message = tool_result_message(call, &result);
        if let Err(error) = self
            .request
            .storage
            .commit_plan_update(&self.request.turn_id, &plan, &message)
            .await
        {
            tool_trace.finish_result(&result, false);
            return Err(TurnRunError::Persistence(error));
        }
        tool_trace.finish_result(&result, true);

        self.request.chat.append_tool_result(message).await?;
        let snapshot = plan.to_snapshot();
        self.current_plan = Some(plan);

        self.update(SessionUpdate::PlanUpdated {
            explanation: snapshot.explanation,
            plan: snapshot.steps,
            updated_at: snapshot.updated_at,
        })
        .await?;
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

    /// 执行一次多智能体控制调用。所有领域错误都写成失败 Tool Result，父 Turn 继续运行。
    async fn run_agent_tool(
        &mut self,
        call: &ToolCallBlock,
        tool_call_id: ToolCallId,
        tool: AgentTool,
        input: &serde_json::Value,
        mut tool_trace: ToolCallTraceGuard,
    ) -> Result<(), TurnRunError> {
        tool_trace.record_permission_policy("allow");
        tool_trace.record_permission_decision("allow", "control_tool");

        let Some(control) = self.request.agent_control.clone() else {
            let result = ToolResult::failed(
                ToolErrorCode::ExecutionFailed,
                "agent_control_unavailable: this session has no collaboration control plane",
                false,
            );
            return self
                .append_tool_result(call, tool_call_id, result, tool_trace)
                .await;
        };

        let result = match tool {
            AgentTool::Spawn => match parse_agent_args::<SpawnAgentArgs>(tool, input) {
                Ok(args) if !args.message.trim().is_empty() => {
                    let spawn_span_id = Some(tool_trace.span_id().to_string());
                    match control
                        .spawn(&args.task_name, args.message, spawn_span_id)
                        .await
                    {
                        Ok(agent) => ToolResult::succeeded(
                            serde_json::json!({ "task_name": agent.task_name }).to_string(),
                        ),
                        Err(error) => agent_error_result(error),
                    }
                }
                Ok(_) => invalid_agent_args("message must not be blank"),
                Err(message) => invalid_agent_args(message),
            },
            AgentTool::Wait => match parse_agent_args::<WaitAgentArgs>(tool, input)
                .and_then(|args| validate_wait_timeout(args.timeout_ms))
            {
                Ok(timeout_ms) => {
                    let delivered = tokio::select! {
                        _ = self.request.cancel.cancelled() => {
                            let result = ToolResult::cancelled(
                                "turn cancelled while waiting for a sub-agent"
                            );
                            self.append_tool_result(call, tool_call_id, result, tool_trace)
                                .await?;
                            return Err(TurnRunError::Cancelled);
                        }
                        delivered = self.request.mailbox.wait_for_delivery(
                            std::time::Duration::from_millis(timeout_ms)
                        ) => delivered,
                    };
                    let result = ToolResult::succeeded(
                        serde_json::json!({
                            "delivered": delivered,
                            "timed_out": !delivered,
                        })
                        .to_string(),
                    );
                    self.append_tool_result(call, tool_call_id, result, tool_trace)
                        .await?;
                    if delivered {
                        self.consecutive_wait_timeouts = 0;
                    } else {
                        self.consecutive_wait_timeouts += 1;
                        if self.consecutive_wait_timeouts
                            >= self.request.agent.policy().doom_loop_threshold
                        {
                            return Err(TurnRunError::DoomLoop(call.name.clone()));
                        }
                    }
                    return Ok(());
                }
                Err(message) => {
                    self.consecutive_wait_timeouts = 0;
                    invalid_agent_args(message)
                }
            },
            AgentTool::List => match parse_agent_args::<NoArgs>(tool, input) {
                Ok(_) => match control.list_statuses().await {
                    Ok(statuses) => match serde_json::to_string(&statuses) {
                        Ok(output) => ToolResult::succeeded(output),
                        Err(error) => ToolResult::failed(
                            ToolErrorCode::ExecutionFailed,
                            format!("failed to serialize agent statuses: {error}"),
                            false,
                        ),
                    },
                    Err(error) => agent_error_result(error),
                },
                Err(message) => invalid_agent_args(message),
            },
            AgentTool::Followup => match parse_agent_args::<FollowupTaskArgs>(tool, input) {
                Ok(args) if !args.message.trim().is_empty() => {
                    match control.followup(&args.task_name, args.message).await {
                        Ok(()) => ToolResult::succeeded("Follow-up task started"),
                        Err(error) => agent_error_result(error),
                    }
                }
                Ok(_) => invalid_agent_args("message must not be blank"),
                Err(message) => invalid_agent_args(message),
            },
            AgentTool::Interrupt => match parse_agent_args::<InterruptAgentArgs>(tool, input) {
                Ok(args) => match control.interrupt(&args.task_name).await {
                    Ok(()) => ToolResult::succeeded("Interrupt requested"),
                    Err(error) => agent_error_result(error),
                },
                Err(message) => invalid_agent_args(message),
            },
        };

        self.append_tool_result(call, tool_call_id, result, tool_trace)
            .await
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

fn invalid_agent_args(message: impl Into<String>) -> ToolResult {
    ToolResult::failed(ToolErrorCode::InvalidArguments, message, false)
}

fn agent_error_result(error: crate::AgentControlError) -> ToolResult {
    ToolResult::failed(
        ToolErrorCode::ExecutionFailed,
        format!("{}: {error}", error.code()),
        false,
    )
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
