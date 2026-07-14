//! Turn 内的 Agent 主循环:取消息 → 喂模型(流式)→ 解析工具调用 → 执行 → 回填 → 重复,
//! 直到模型返回不带工具调用的纯文本。

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use futures_util::StreamExt;
use openwork_protocol::approval::{
    ApprovalPolicy, ApprovalRequested, ApprovalResolution, ApprovalResolved,
    ExecutionPolicyDecision,
};
use openwork_protocol::capability::{
    ActionRequest, CapabilityResolveError, CapabilityResolverPort, ExecutionPort, Observation,
    ObservationContent, ObservationStatus,
};
use openwork_protocol::domain::{ApprovalId, StepId, ToolRunId, TurnId};
use openwork_protocol::model::{
    ContentBlock, Message, ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest,
    ModelResponse, Role, ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
};
use openwork_protocol::turn::{
    AssistantMessageRecorded, StepCompleted, StepFailed, StepStarted, ToolMessageRecorded,
    ToolRunFinished, ToolRunRequested, ToolRunStarted, TurnRecordError, TurnRecordedEvent,
    TurnRecorderPort,
};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{ApprovalWaitOutcome, TurnCommandInbox};

const DEFAULT_MAX_STEPS: usize = 20;
/// 连续同名 + 规范化同参的工具调用达到此次数,判定为 doom loop。
const DOOM_LOOP_THRESHOLD: usize = 3;

const AGENT_SYSTEM_PROMPT: &str = "\
You are OpenWork, an autonomous coding agent operating in a working directory on the user's machine.\n\
You have tools to read and write files, list directories, and run shell commands. \
Call a tool whenever it helps you make progress toward the task. \
When the task is done or you have a final answer, respond with plain text and no tool calls.";

pub struct AgentPorts {
    pub capabilities: Arc<dyn CapabilityResolverPort>,
    pub execution: Arc<dyn ExecutionPort>,
    /// Awaited durable fact sink. A failed append stops the control loop before
    /// any following side effect is invoked.
    pub recorder: Arc<dyn TurnRecorderPort>,
}

impl AgentPorts {
    pub fn new(
        capabilities: Arc<dyn CapabilityResolverPort>,
        execution: Arc<dyn ExecutionPort>,
        recorder: Arc<dyn TurnRecorderPort>,
    ) -> Self {
        Self {
            capabilities,
            execution,
            recorder,
        }
    }
}

pub struct AgentConfig {
    pub provider: Box<dyn ModelPort>,
    pub model: String,
    pub capabilities: Arc<dyn CapabilityResolverPort>,
    pub execution: Arc<dyn ExecutionPort>,
    pub recorder: Arc<dyn TurnRecorderPort>,
    pub turn_id: TurnId,
    /// 何时需要对工具调用发起审批。
    pub approval_policy: ApprovalPolicy,
    /// Core-owned command inbox. User decisions return here as typed Turn commands.
    pub approval_commands: TurnCommandInbox,
    /// 取消令牌:外部触发 `cancel()` 后,agent loop 在下一个 await 点终止并返回
    /// [`AgentError::Cancelled`],携带截止当前的对话轨迹。
    pub cancel: CancellationToken,
    pub max_steps: usize,
    /// First model step to run. Recovery continues after the checkpointed step.
    pub first_step: usize,
}

impl AgentConfig {
    /// 注入能力发现和执行 Port，并使用默认审批配置构造。
    pub fn new(
        provider: Box<dyn ModelPort>,
        model: impl Into<String>,
        ports: AgentPorts,
        turn_id: TurnId,
        approval_commands: TurnCommandInbox,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            capabilities: ports.capabilities,
            execution: ports.execution,
            recorder: ports.recorder,
            turn_id,
            approval_policy: ApprovalPolicy::Untrusted,
            approval_commands,
            cancel,
            max_steps: DEFAULT_MAX_STEPS,
            first_step: 1,
        }
    }

    pub fn with_first_step(mut self, first_step: usize) -> Self {
        self.first_step = first_step.max(1);
        self
    }
}

/// agent loop 向外发出的事件,供调用方(UI/测试)消费。
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Step {
        id: StepId,
        index: usize,
    },
    LlmStepStart {
        index: usize,
    },
    LlmStepFinish {
        index: usize,
        reason: String,
        usage: Option<openwork_protocol::model::TokenUsage>,
    },
    LlmFinish {
        reason: String,
        usage: Option<openwork_protocol::model::TokenUsage>,
    },
    TextStart {
        id: String,
    },
    TextDelta(String),
    TextEnd {
        id: String,
    },
    ReasoningStart {
        id: String,
    },
    ReasoningDelta(String),
    ReasoningEnd {
        id: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        partial_input: String,
    },
    ToolCallEnd {
        id: String,
    },
    ToolResult {
        id: String,
        tool_run_id: ToolRunId,
        name: String,
        output: Vec<ContentBlock>,
        is_error: bool,
    },
    /// Core has moved this Turn to the explicit waiting-approval state.
    ApprovalRequested(ApprovalRequested),
    /// Core applied a typed ResolveApproval command to the waiting Turn.
    ApprovalResolved(ApprovalResolved),
    Finished(String),
    /// 检测到 doom loop(连续重复同名同参工具调用),agent 即将停止。
    DoomLoopDetected {
        repeated: String,
    },
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("provider error: {0}")]
    Provider(#[from] ModelError),
    #[error("capability resolver error: {0}")]
    Capability(#[from] CapabilityResolveError),
    #[error("turn lifecycle record error: {0}")]
    Record(#[from] TurnRecordError),
    #[error("turn recovery state is invalid: {0}")]
    InvalidRecovery(String),
    #[error("exceeded max steps ({0})")]
    MaxStepsExceeded(usize),
    /// 被外部取消;携带截止取消时的对话轨迹(不含 system prompt),供持久化部分结果。
    #[error("cancelled")]
    Cancelled(Vec<Message>),
    /// 检测到 doom loop;携带截止时的轨迹 + 重复的工具名。
    #[error("doom loop: tool '{0}' repeated")]
    DoomLoop(String, Vec<Message>),
}

/// `Agent::run` 的返回:最终文本回答 + 完整对话轨迹(不含 system prompt)。
/// 轨迹包含传入的 history 与本轮新增的 assistant/tool 消息,调用方可据此持久化。
#[derive(Debug, Clone)]
pub struct RunResult {
    pub text: String,
    pub messages: Vec<Message>,
}

/// Durable state needed to continue one Step that was parked for approval.
#[derive(Debug, Clone)]
pub struct ApprovalRecovery {
    pub request: ApprovalRequested,
    pub provider_tool_call_id: String,
    pub step_index: usize,
}

pub struct Agent {
    config: AgentConfig,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    /// 跑 turn loop,流式向外 emit 事件,返回最终文本回答。
    /// `history` 为对话历史(user/assistant 消息);Agent 会在前面补上 system prompt。
    pub async fn run(
        &mut self,
        history: Vec<Message>,
        mut on_event: impl FnMut(AgentEvent),
    ) -> Result<RunResult, AgentError> {
        let mut messages = vec![Message::text(Role::System, AGENT_SYSTEM_PROMPT)];
        messages.extend(history);
        let tool_defs = self.tool_definitions().await?;
        self.run_steps(
            messages,
            tool_defs,
            self.config.first_step,
            VecDeque::new(),
            &mut on_event,
        )
        .await
    }

    /// Continues a Turn that was durably parked before invoking one tool.
    /// Only a still-pending approval is resumable. A recorded `tool_run_started`
    /// without a terminal fact is deliberately handled as outcome-unknown by
    /// the recovery projection and never enters this method.
    pub async fn resume_after_approval(
        &mut self,
        history: Vec<Message>,
        recovery: ApprovalRecovery,
        resolution: ApprovalResolution,
        mut on_event: impl FnMut(AgentEvent),
    ) -> Result<RunResult, AgentError> {
        let mut messages = vec![Message::text(Role::System, AGENT_SYSTEM_PROMPT)];
        messages.extend(history);
        let tool_calls = messages
            .iter()
            .rev()
            .find_map(|message| {
                if message.role != Role::Assistant {
                    return None;
                }
                let calls = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolCall(call) => Some(call.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                calls
                    .iter()
                    .any(|call| call.id == recovery.provider_tool_call_id)
                    .then_some(calls)
            })
            .ok_or_else(|| {
                AgentError::InvalidRecovery(format!(
                    "assistant tool call {} is missing",
                    recovery.provider_tool_call_id
                ))
            })?;
        let pending_call = tool_calls
            .iter()
            .find(|call| call.id == recovery.provider_tool_call_id)
            .cloned()
            .ok_or_else(|| {
                AgentError::InvalidRecovery(format!(
                    "tool call {} is missing",
                    recovery.provider_tool_call_id
                ))
            })?;
        if pending_call.name != recovery.request.tool_name {
            return Err(AgentError::InvalidRecovery(format!(
                "tool call name {} does not match pending approval {}",
                pending_call.name, recovery.request.tool_name
            )));
        }

        let mut completed_call_ids = tool_result_ids(&messages);
        if completed_call_ids.contains(&pending_call.id) {
            return Err(AgentError::InvalidRecovery(format!(
                "tool call {} already has a result",
                pending_call.id
            )));
        }

        let observation = self
            .execute_recovered_tool_call(
                &pending_call,
                &recovery.request,
                resolution,
                &mut on_event,
            )
            .await?;
        self.finish_tool_run(
            &mut messages,
            &recovery.request.step_id,
            &recovery.request.tool_run_id,
            &pending_call,
            observation,
            &mut on_event,
        )
        .await?;
        completed_call_ids.insert(pending_call.id.clone());

        // A provider may return multiple tool calls in one Step. Calls that
        // follow the recovered approval must finish before the next model call.
        for tool_call in tool_calls {
            if completed_call_ids.contains(&tool_call.id) {
                continue;
            }
            let input = parse_tool_input(&tool_call);
            let (tool_run_id, observation) = self
                .execute_new_tool_call(&recovery.request.step_id, &tool_call, input, &mut on_event)
                .await?;
            self.finish_tool_run(
                &mut messages,
                &recovery.request.step_id,
                &tool_run_id,
                &tool_call,
                observation,
                &mut on_event,
            )
            .await?;
        }
        self.record(vec![TurnRecordedEvent::StepCompleted(StepCompleted {
            step_id: recovery.request.step_id.clone(),
            step_index: recovery.step_index,
        })])
        .await?;

        let tool_defs = self.tool_definitions().await?;
        self.run_steps(
            messages,
            tool_defs,
            recovery.step_index.saturating_add(1),
            VecDeque::new(),
            &mut on_event,
        )
        .await
    }

    async fn run_steps(
        &mut self,
        mut messages: Vec<Message>,
        tool_defs: Vec<openwork_protocol::model::ToolDefinition>,
        first_step: usize,
        mut recent: VecDeque<(String, String)>,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> Result<RunResult, AgentError> {
        for step_index in first_step..=self.config.max_steps {
            if self.config.cancel.is_cancelled() {
                return Err(AgentError::Cancelled(messages[1..].to_vec()));
            }
            let step_id = StepId::new(new_runtime_id("step"));
            self.record(vec![TurnRecordedEvent::StepStarted(StepStarted {
                step_id: step_id.clone(),
                step_index,
            })])
            .await?;
            on_event(AgentEvent::Step {
                id: step_id.clone(),
                index: step_index,
            });

            let req = ModelRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                temperature: None,
                max_output_tokens: None,
                thinking: None,
                tools: tool_defs.clone(),
            };

            let response = match self.stream_once(step_index, req, on_event).await {
                Ok(response) => response,
                Err(AgentError::Cancelled(_)) => {
                    self.record_step_failed(&step_id, step_index, "cancelled")
                        .await?;
                    return Err(AgentError::Cancelled(messages[1..].to_vec()));
                }
                Err(error @ AgentError::Record(_)) => return Err(error),
                Err(error) => {
                    self.record_step_failed(&step_id, step_index, &error.to_string())
                        .await?;
                    return Err(error);
                }
            };

            let assistant_content = assistant_content(&response);
            if !assistant_content.is_empty() {
                self.record(vec![TurnRecordedEvent::AssistantMessageRecorded(
                    AssistantMessageRecorded {
                        message_id: new_runtime_id("msg"),
                        step_id: step_id.clone(),
                        parts: assistant_content.clone(),
                    },
                )])
                .await?;
                messages.push(Message {
                    role: Role::Assistant,
                    content: assistant_content,
                });
            }

            // 无工具调用 → 结束。返回完整轨迹(跳过 system prompt)。
            if response.tool_calls.is_empty() {
                self.record(vec![TurnRecordedEvent::StepCompleted(StepCompleted {
                    step_id,
                    step_index,
                })])
                .await?;
                on_event(AgentEvent::Finished(response.text.clone()));
                return Ok(RunResult {
                    text: response.text,
                    messages: messages[1..].to_vec(),
                });
            }

            // 执行工具并回填结果(含审批决策)。
            for tc in response.tool_calls {
                if self.config.cancel.is_cancelled() {
                    self.record_step_failed(&step_id, step_index, "cancelled")
                        .await?;
                    return Err(AgentError::Cancelled(messages[1..].to_vec()));
                }
                let input = parse_tool_input(&tc);

                // doom-loop 检测:连续 N 次同名 + 规范化同参 → 停止。
                let key = (tc.name.clone(), normalize_json(&input));
                recent.push_back(key.clone());
                if recent.len() > DOOM_LOOP_THRESHOLD {
                    recent.pop_front();
                }
                if recent.len() == DOOM_LOOP_THRESHOLD && recent.iter().all(|k| k == &key) {
                    self.record_step_failed(
                        &step_id,
                        step_index,
                        &format!("doom loop detected for tool {}", tc.name),
                    )
                    .await?;
                    on_event(AgentEvent::DoomLoopDetected {
                        repeated: tc.name.clone(),
                    });
                    return Err(AgentError::DoomLoop(
                        tc.name.clone(),
                        messages[1..].to_vec(),
                    ));
                }

                let (tool_run_id, output) = self
                    .execute_new_tool_call(&step_id, &tc, input, on_event)
                    .await?;
                if self.config.cancel.is_cancelled() {
                    // The terminal tool observation is still recorded below;
                    // cancellation must not erase what already happened.
                }
                self.finish_tool_run(&mut messages, &step_id, &tool_run_id, &tc, output, on_event)
                    .await?;
                if self.config.cancel.is_cancelled() {
                    self.record_step_failed(&step_id, step_index, "cancelled")
                        .await?;
                    return Err(AgentError::Cancelled(messages[1..].to_vec()));
                }
            }
            self.record(vec![TurnRecordedEvent::StepCompleted(StepCompleted {
                step_id,
                step_index,
            })])
            .await?;
        }

        Err(AgentError::MaxStepsExceeded(self.config.max_steps))
    }

    /// 单次流式调用 provider,实时转发事件,返回累积响应。
    async fn stream_once(
        &self,
        step_index: usize,
        req: ModelRequest,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> Result<ModelResponse, AgentError> {
        let mut provider_stream = self
            .config
            .provider
            .invoke(req, ModelCallOptions::new(new_runtime_id("attempt")))
            .await?;
        let mut lifecycle = StreamLifecycle::new();
        on_event(AgentEvent::LlmStepStart { index: step_index });

        let response = loop {
            tokio::select! {
                biased;
                _ = self.config.cancel.cancelled() => return Err(AgentError::Cancelled(Vec::new())),
                item = provider_stream.next() => match item {
                    Some(Ok(ModelEvent::ResponseCompleted { response })) => break *response,
                    Some(Ok(event)) => forward_event(event, on_event, &mut lifecycle),
                    Some(Err(error)) => return Err(error.into()),
                    None => return Err(ModelError::protocol("provider stream ended without ResponseCompleted").into()),
                },
            }
        };
        lifecycle.close_open(on_event);
        let reason = response.finish_reason.as_str().to_string();
        let usage = response.usage;
        on_event(AgentEvent::LlmStepFinish {
            index: step_index,
            reason: reason.clone(),
            usage,
        });
        on_event(AgentEvent::LlmFinish { reason, usage });
        Ok(response)
    }

    async fn execute_new_tool_call(
        &mut self,
        step_id: &StepId,
        tc: &ToolCallBlock,
        input: serde_json::Value,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> Result<(ToolRunId, Observation), AgentError> {
        let tool_run_id = ToolRunId::new(new_runtime_id("tool-run"));
        self.record(vec![TurnRecordedEvent::ToolRunRequested(
            ToolRunRequested {
                step_id: step_id.clone(),
                tool_run_id: tool_run_id.clone(),
                provider_tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                input: input.clone(),
            },
        )])
        .await?;
        let action = ActionRequest::new(&tc.name, input.clone());
        let observation = match self
            .config
            .execution
            .authorize(&action, self.config.approval_policy)
            .await
        {
            ExecutionPolicyDecision::Allow => {
                self.record(vec![TurnRecordedEvent::ToolRunStarted(ToolRunStarted {
                    step_id: step_id.clone(),
                    tool_run_id: tool_run_id.clone(),
                })])
                .await?;
                self.run_tool(&tc.name, input).await
            }
            ExecutionPolicyDecision::Deny { reason } => Observation::denied(reason),
            ExecutionPolicyDecision::RequireApproval { reason } => {
                let request = ApprovalRequested {
                    approval_id: ApprovalId::new(new_runtime_id("approval")),
                    turn_id: self.config.turn_id.clone(),
                    step_id: step_id.clone(),
                    tool_run_id: tool_run_id.clone(),
                    tool_name: tc.name.clone(),
                    input: input.clone(),
                    reason,
                };
                if let Err(error) = self
                    .config
                    .approval_commands
                    .begin_approval(request.clone())
                {
                    return Ok((tool_run_id, Observation::approval_denied(error.to_string())));
                }
                self.record(vec![TurnRecordedEvent::ApprovalRequested(request.clone())])
                    .await?;
                on_event(AgentEvent::ApprovalRequested(request.clone()));

                let outcome = self
                    .config
                    .approval_commands
                    .wait_for_resolution(&self.config.cancel)
                    .await;
                if let ApprovalWaitOutcome::Resolved(resolution) = &outcome {
                    let resolved = ApprovalResolved {
                        approval_id: request.approval_id.clone(),
                        turn_id: request.turn_id.clone(),
                        step_id: request.step_id.clone(),
                        tool_run_id: request.tool_run_id.clone(),
                        resolution: resolution.clone(),
                    };
                    let mut events = vec![TurnRecordedEvent::ApprovalResolved(resolved.clone())];
                    if matches!(resolution, ApprovalResolution::Allow) {
                        events.push(TurnRecordedEvent::ToolRunStarted(ToolRunStarted {
                            step_id: step_id.clone(),
                            tool_run_id: tool_run_id.clone(),
                        }));
                    }
                    self.record(events).await?;
                    on_event(AgentEvent::ApprovalResolved(resolved));
                }

                match outcome {
                    ApprovalWaitOutcome::Resolved(ApprovalResolution::Allow) => {
                        self.run_tool(&tc.name, input).await
                    }
                    ApprovalWaitOutcome::Resolved(ApprovalResolution::Deny { reason }) => {
                        Observation::approval_denied(reason)
                    }
                    ApprovalWaitOutcome::Cancelled => Observation::cancelled("cancelled"),
                    ApprovalWaitOutcome::CommandChannelClosed => {
                        Observation::approval_denied("approval command channel closed")
                    }
                    ApprovalWaitOutcome::StateUnavailable => {
                        Observation::approval_denied("approval state unavailable")
                    }
                }
            }
        };
        Ok((tool_run_id, observation))
    }

    async fn execute_recovered_tool_call(
        &mut self,
        tc: &ToolCallBlock,
        request: &ApprovalRequested,
        resolution: ApprovalResolution,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> Result<Observation, AgentError> {
        let resolved = ApprovalResolved {
            approval_id: request.approval_id.clone(),
            turn_id: request.turn_id.clone(),
            step_id: request.step_id.clone(),
            tool_run_id: request.tool_run_id.clone(),
            resolution: resolution.clone(),
        };
        let mut events = vec![TurnRecordedEvent::ApprovalResolved(resolved.clone())];
        if matches!(resolution, ApprovalResolution::Allow) {
            events.push(TurnRecordedEvent::ToolRunStarted(ToolRunStarted {
                step_id: request.step_id.clone(),
                tool_run_id: request.tool_run_id.clone(),
            }));
        }
        self.record(events).await?;
        on_event(AgentEvent::ApprovalResolved(resolved));

        Ok(match resolution {
            ApprovalResolution::Allow => self.run_tool(&tc.name, request.input.clone()).await,
            ApprovalResolution::Deny { reason } => Observation::approval_denied(reason),
        })
    }

    async fn finish_tool_run(
        &self,
        messages: &mut Vec<Message>,
        step_id: &StepId,
        tool_run_id: &ToolRunId,
        tc: &ToolCallBlock,
        observation: Observation,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> Result<(), AgentError> {
        mark_tool_call_finished(messages, &tc.id);
        let model_output = observation_content(&observation);
        let parts = vec![ContentBlock::ToolResult(ToolResultBlock {
            id: tc.id.clone(),
            name: tc.name.clone(),
            output: model_output.clone(),
            state: tool_result_state(observation.status),
        })];
        self.record(vec![
            TurnRecordedEvent::ToolRunFinished(ToolRunFinished {
                step_id: step_id.clone(),
                tool_run_id: tool_run_id.clone(),
                observation: observation.clone(),
            }),
            TurnRecordedEvent::ToolMessageRecorded(ToolMessageRecorded {
                message_id: new_runtime_id("msg"),
                step_id: step_id.clone(),
                tool_run_id: tool_run_id.clone(),
                parts: parts.clone(),
            }),
        ])
        .await?;
        on_event(AgentEvent::ToolResult {
            id: tc.id.clone(),
            tool_run_id: tool_run_id.clone(),
            name: tc.name.clone(),
            output: model_output,
            is_error: observation.is_error(),
        });
        messages.push(Message {
            role: Role::Tool,
            content: parts,
        });
        Ok(())
    }

    async fn record_step_failed(
        &self,
        step_id: &StepId,
        step_index: usize,
        message: &str,
    ) -> Result<(), AgentError> {
        self.record(vec![TurnRecordedEvent::StepFailed(StepFailed {
            step_id: step_id.clone(),
            step_index,
            message: message.to_string(),
        })])
        .await
    }

    async fn record(&self, events: Vec<TurnRecordedEvent>) -> Result<(), AgentError> {
        self.config.recorder.append(events).await?;
        Ok(())
    }

    async fn tool_definitions(
        &self,
    ) -> Result<Vec<openwork_protocol::model::ToolDefinition>, AgentError> {
        Ok(self
            .config
            .capabilities
            .list()
            .await?
            .iter()
            .map(|spec| spec.model_definition())
            .collect())
    }

    /// 真正执行 Action(无审批),只依赖稳定的 ExecutionPort。
    async fn run_tool(&self, name: &str, input: serde_json::Value) -> Observation {
        let request = ActionRequest::new(name, input);
        tokio::select! {
            biased;
            _ = self.config.cancel.cancelled() => Observation::cancelled("cancelled"),
            observation = self.config.execution.execute(request) => observation,
        }
    }
}

fn assistant_content(response: &ModelResponse) -> Vec<ContentBlock> {
    let mut content = Vec::new();
    if response.provider_opaque_blocks.is_empty() {
        if let Some(reasoning) = &response.reasoning_text
            && !reasoning.is_empty()
        {
            content.push(ContentBlock::thinking(reasoning.clone()));
        }
    } else {
        content.extend(
            response
                .provider_opaque_blocks
                .iter()
                .cloned()
                .map(ContentBlock::ProviderOpaque),
        );
    }
    if !response.text.is_empty() {
        content.push(ContentBlock::text(response.text.clone()));
    }
    content.extend(response.tool_calls.iter().map(|tool_call| {
        ContentBlock::ToolCall(ToolCallBlock {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            input: tool_call.input.clone(),
            state: ToolCallState::Submitted,
        })
    }));
    content
}

fn parse_tool_input(tool_call: &ToolCallBlock) -> serde_json::Value {
    serde_json::from_str(&tool_call.input).unwrap_or(serde_json::Value::Null)
}

fn tool_result_ids(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result.id.clone()),
            _ => None,
        })
        .collect()
}

fn tool_result_state(status: ObservationStatus) -> ToolResultState {
    match status {
        ObservationStatus::Succeeded => ToolResultState::Success,
        ObservationStatus::Denied => ToolResultState::Denied,
        ObservationStatus::Cancelled => ToolResultState::Interrupted,
        ObservationStatus::Failed | ObservationStatus::OutcomeUnknown => ToolResultState::Error,
    }
}

fn new_runtime_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn observation_content(observation: &Observation) -> Vec<ContentBlock> {
    observation
        .content
        .iter()
        .map(|content| match content {
            ObservationContent::Text { text } => ContentBlock::text(text),
        })
        .collect()
}

fn mark_tool_call_finished(messages: &mut [Message], id: &str) {
    for message in messages.iter_mut().rev() {
        if message.role != Role::Assistant {
            continue;
        }
        for block in &mut message.content {
            if let ContentBlock::ToolCall(tool_call) = block
                && tool_call.id == id
            {
                tool_call.state = ToolCallState::Finished;
                return;
            }
        }
    }
}

struct StreamLifecycle {
    text_open: bool,
    reasoning_open: bool,
}

impl StreamLifecycle {
    fn new() -> Self {
        Self {
            text_open: false,
            reasoning_open: false,
        }
    }

    fn close_open(&mut self, on_event: &mut impl FnMut(AgentEvent)) {
        if self.reasoning_open {
            self.reasoning_open = false;
            on_event(AgentEvent::ReasoningEnd {
                id: "reasoning-0".to_string(),
            });
        }
        if self.text_open {
            self.text_open = false;
            on_event(AgentEvent::TextEnd {
                id: "text-0".to_string(),
            });
        }
    }
}

fn forward_event(
    event: ModelEvent,
    on_event: &mut impl FnMut(AgentEvent),
    lifecycle: &mut StreamLifecycle,
) {
    match event {
        ModelEvent::TextStart { id, .. } => {
            lifecycle.text_open = true;
            on_event(AgentEvent::TextStart { id });
        }
        ModelEvent::TextDelta { delta, .. } => {
            if !lifecycle.text_open {
                lifecycle.text_open = true;
                on_event(AgentEvent::TextStart {
                    id: "text-0".to_string(),
                });
            }
            on_event(AgentEvent::TextDelta(delta));
        }
        ModelEvent::TextEnd { id, .. } => {
            lifecycle.text_open = false;
            on_event(AgentEvent::TextEnd { id });
        }
        ModelEvent::ReasoningStart { id, .. } => {
            lifecycle.reasoning_open = true;
            on_event(AgentEvent::ReasoningStart { id });
        }
        ModelEvent::ReasoningDelta { delta, .. } => {
            if !lifecycle.reasoning_open {
                lifecycle.reasoning_open = true;
                on_event(AgentEvent::ReasoningStart {
                    id: "reasoning-0".to_string(),
                });
            }
            on_event(AgentEvent::ReasoningDelta(delta))
        }
        ModelEvent::ReasoningEnd { id, .. } => {
            lifecycle.reasoning_open = false;
            on_event(AgentEvent::ReasoningEnd { id });
        }
        ModelEvent::ToolCallStart { id, name, .. } => {
            on_event(AgentEvent::ToolCallStart { id, name })
        }
        ModelEvent::ToolCallDelta {
            id, partial_input, ..
        } => on_event(AgentEvent::ToolCallDelta { id, partial_input }),
        ModelEvent::ToolCallEnd { id, .. } => on_event(AgentEvent::ToolCallEnd { id }),
        ModelEvent::ResponseCompleted { .. } => {}
    }
}

/// 把 JSON 规范化为字符串(顶层 key 排序),用于 doom-loop 的同参比较。
/// `serde_json` 保插入序,排序后 `{"a":1,"b":2}` 与 `{"b":2,"a":1}` 视为相同。
fn normalize_json(value: &serde_json::Value) -> String {
    if let serde_json::Value::Object(map) = value {
        let sorted: std::collections::BTreeMap<String, serde_json::Value> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        serde_json::to_string(&sorted).unwrap_or_default()
    } else {
        serde_json::to_string(value).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures_util::stream;
    use openwork_protocol::approval::ResolveApproval;
    use openwork_protocol::capability::{CapabilityRiskHint, CapabilitySpec, ObservationErrorCode};
    use openwork_protocol::model::{FinishReason, ModelStream};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    /// 按 `invoke` 调用顺序依次返回预设响应的假 provider。
    struct FakeProvider {
        responses: tokio::sync::Mutex<VecDeque<ModelResponse>>,
    }

    impl FakeProvider {
        fn new(responses: Vec<ModelResponse>) -> Self {
            Self {
                responses: tokio::sync::Mutex::new(responses.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl ModelPort for FakeProvider {
        async fn invoke(
            &self,
            _req: ModelRequest,
            _options: ModelCallOptions,
        ) -> Result<ModelStream, ModelError> {
            let response = self
                .responses
                .lock()
                .await
                .pop_front()
                .ok_or_else(|| ModelError::invalid_request("FakeProvider exhausted"))?;
            Ok(Box::pin(stream::iter([Ok(
                ModelEvent::ResponseCompleted {
                    response: Box::new(response),
                },
            )])))
        }
    }

    struct FakeCapabilities;

    #[async_trait]
    impl CapabilityResolverPort for FakeCapabilities {
        async fn list(&self) -> Result<Vec<CapabilitySpec>, CapabilityResolveError> {
            Ok(vec![bash_spec()])
        }

        async fn resolve(
            &self,
            name: &str,
        ) -> Result<Option<CapabilitySpec>, CapabilityResolveError> {
            Ok((name == "bash").then(bash_spec))
        }
    }

    struct FakeExecution;

    #[async_trait]
    impl ExecutionPort for FakeExecution {
        async fn authorize(
            &self,
            _request: &ActionRequest,
            policy: ApprovalPolicy,
        ) -> ExecutionPolicyDecision {
            match policy {
                ApprovalPolicy::Untrusted => ExecutionPolicyDecision::RequireApproval {
                    reason: "process execution requires user approval".to_string(),
                },
                ApprovalPolicy::Never => ExecutionPolicyDecision::Allow,
            }
        }

        async fn execute(&self, request: ActionRequest) -> Observation {
            if request.name == "bash" {
                Observation::succeeded("executed")
            } else {
                Observation::failed(
                    ObservationErrorCode::CapabilityNotFound,
                    format!("capability not found: {}", request.name),
                    false,
                )
            }
        }
    }

    struct FakeRecorder;

    #[async_trait]
    impl TurnRecorderPort for FakeRecorder {
        async fn append(&self, _events: Vec<TurnRecordedEvent>) -> Result<(), TurnRecordError> {
            Ok(())
        }
    }

    fn bash_spec() -> CapabilitySpec {
        CapabilitySpec {
            name: "bash".to_string(),
            description: "Run a shell command".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {"command": {"type": "string"}},
                "required": ["command"]
            }),
            risk_hint: CapabilityRiskHint::ProcessExecution,
        }
    }

    fn test_config_with_handle(
        provider: Box<dyn ModelPort>,
    ) -> (AgentConfig, crate::TurnCommandHandle) {
        let turn_id = TurnId::new("turn-test");
        let (handle, inbox) = crate::turn_command_channel(turn_id.clone());
        (
            AgentConfig::new(
                provider,
                "fake",
                AgentPorts::new(
                    Arc::new(FakeCapabilities),
                    Arc::new(FakeExecution),
                    Arc::new(FakeRecorder),
                ),
                turn_id,
                inbox,
                CancellationToken::new(),
            ),
            handle,
        )
    }

    fn test_config(provider: Box<dyn ModelPort>) -> AgentConfig {
        test_config_with_handle(provider).0
    }

    fn tool_call_response(id: &str, name: &str, input: serde_json::Value) -> ModelResponse {
        ModelResponse {
            response_id: None,
            provider_request_id: None,
            model: Some("fake".to_string()),
            text: String::new(),
            reasoning_text: None,
            tool_calls: vec![ToolCallBlock {
                id: id.to_string(),
                name: name.to_string(),
                input: input.to_string(),
                state: ToolCallState::Submitted,
            }],
            provider_opaque_blocks: Vec::new(),
            finish_reason: FinishReason::ToolUse,
            raw_finish_reason: Some("tool_use".to_string()),
            usage: None,
        }
    }

    fn text_response(text: &str) -> ModelResponse {
        ModelResponse {
            response_id: None,
            provider_request_id: None,
            model: Some("fake".to_string()),
            text: text.to_string(),
            reasoning_text: None,
            tool_calls: Vec::new(),
            provider_opaque_blocks: Vec::new(),
            finish_reason: FinishReason::Stop,
            raw_finish_reason: Some("stop".to_string()),
            usage: None,
        }
    }

    fn untrusted_config(provider: Box<dyn ModelPort>) -> (AgentConfig, crate::TurnCommandHandle) {
        let (mut config, handle) = test_config_with_handle(provider);
        config.approval_policy = ApprovalPolicy::Untrusted;
        (config, handle)
    }

    fn extract_text(blocks: Vec<ContentBlock>) -> String {
        blocks
            .into_iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 构造 on_event 闭包:遇到 ApprovalRequested 时将命令路由回当前 Turn。
    fn auto_resolve_on_event(
        handle: crate::TurnCommandHandle,
        resolution: ApprovalResolution,
    ) -> (impl FnMut(AgentEvent), Arc<StdMutex<bool>>) {
        let seen = Arc::new(StdMutex::new(false));
        let seen_cb = seen.clone();
        let closure = move |event: AgentEvent| {
            if let AgentEvent::ApprovalRequested(request) = event {
                *seen_cb.lock().unwrap() = true;
                let handle = handle.clone();
                let resolution = resolution.clone();
                tokio::spawn(async move {
                    let _ = handle
                        .resolve(ResolveApproval {
                            turn_id: request.turn_id,
                            approval_id: request.approval_id,
                            resolution,
                        })
                        .await;
                });
            }
        };
        (closure, seen)
    }

    #[tokio::test]
    async fn untrusted_user_allow_executes_tool_then_finishes() {
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "true"})),
            text_response("done"),
        ]);
        let (config, handle) = untrusted_config(Box::new(provider));
        let mut agent = Agent::new(config);

        let (mut on_event, seen) = auto_resolve_on_event(handle, ApprovalResolution::Allow);
        let result = agent
            .run(vec![Message::text(Role::User, "run it")], &mut on_event)
            .await
            .unwrap();

        assert_eq!(result.text, "done");
        assert!(
            *seen.lock().unwrap(),
            "Untrusted+User must request approval before executing"
        );
    }

    #[tokio::test]
    async fn untrusted_user_deny_blocks_tool_execution() {
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "echo SHOULD_NOT_RUN"})),
            text_response("ok"),
        ]);
        let (config, handle) = untrusted_config(Box::new(provider));
        let mut agent = Agent::new(config);

        let tool_results: Arc<StdMutex<Vec<(bool, String)>>> = Arc::new(StdMutex::new(Vec::new()));
        let results_cb = tool_results.clone();
        let handle_cb = handle.clone();
        let result = agent
            .run(
                vec![Message::text(Role::User, "run it")],
                move |event| match event {
                    AgentEvent::ApprovalRequested(request) => {
                        let handle = handle_cb.clone();
                        tokio::spawn(async move {
                            let _ = handle
                                .resolve(ResolveApproval {
                                    turn_id: request.turn_id,
                                    approval_id: request.approval_id,
                                    resolution: ApprovalResolution::Deny {
                                        reason: "user said no".into(),
                                    },
                                })
                                .await;
                        });
                    }
                    AgentEvent::ToolResult {
                        is_error, output, ..
                    } => {
                        results_cb
                            .lock()
                            .unwrap()
                            .push((is_error, extract_text(output)));
                    }
                    _ => {}
                },
            )
            .await
            .unwrap();

        assert_eq!(result.text, "ok");
        let results = tool_results.lock().unwrap().clone();
        assert_eq!(results.len(), 1, "expected one tool result");
        assert!(results[0].0, "denied tool result must be an error");
        assert!(
            !results[0].1.contains("SHOULD_NOT_RUN"),
            "denied command must not have executed"
        );
    }

    #[tokio::test]
    async fn never_policy_runs_tool_directly_without_approval() {
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "true"})),
            text_response("done"),
        ]);
        let mut config = test_config(Box::new(provider));
        config.approval_policy = ApprovalPolicy::Never;
        // Never 路径不应进入 Core 的等待审批状态。
        let mut agent = Agent::new(config);

        let result = agent
            .run(vec![Message::text(Role::User, "run it")], |event| {
                assert!(
                    !matches!(event, AgentEvent::ApprovalRequested(_)),
                    "Never policy must not request approval"
                );
            })
            .await
            .unwrap();

        assert_eq!(result.text, "done");
    }

    #[tokio::test]
    async fn run_returns_full_message_trace() {
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "true"})),
            text_response("done"),
        ]);
        let (config, handle) = untrusted_config(Box::new(provider));
        let mut agent = Agent::new(config);

        let (mut on_event, _seen) = auto_resolve_on_event(handle, ApprovalResolution::Allow);
        let result = agent
            .run(vec![Message::text(Role::User, "run it")], &mut on_event)
            .await
            .unwrap();

        // user + assistant(tool call) + tool(result) + assistant(text)。
        assert_eq!(result.messages.len(), 4);
        assert_eq!(result.messages[0].role, Role::User);
        assert_eq!(result.messages[1].role, Role::Assistant);
        assert!(
            result.messages[1]
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolCall(_)))
        );
        assert_eq!(result.messages[2].role, Role::Tool);
        assert_eq!(result.messages[3].role, Role::Assistant);
        assert_eq!(extract_text(result.messages[3].content.clone()), "done");
        assert_eq!(result.text, "done");
    }

    #[tokio::test]
    async fn cancel_before_run_returns_cancelled_with_trace() {
        let provider = FakeProvider::new(vec![text_response("done")]);
        let config = test_config(Box::new(provider));
        config.cancel.cancel();
        let mut agent = Agent::new(config);

        let history = vec![Message::text(Role::User, "hello")];
        let result = agent.run(history, |_| {}).await;

        match result {
            Err(AgentError::Cancelled(msgs)) => {
                // 截止取消时的轨迹 = history(仅 user)。
                assert_eq!(msgs.len(), 1);
                assert_eq!(msgs[0].role, Role::User);
            }
            other => panic!("expected Cancelled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn doom_loop_detected_on_repeated_identical_calls() {
        // 三个完全相同的 bash 调用 → 第三个触发 doom-loop。
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "echo hi"})),
            tool_call_response("call-2", "bash", json!({"command": "echo hi"})),
            tool_call_response("call-3", "bash", json!({"command": "echo hi"})),
        ]);
        let (config, handle) = untrusted_config(Box::new(provider));
        let mut agent = Agent::new(config);

        let (mut on_event, _seen) = auto_resolve_on_event(handle, ApprovalResolution::Allow);
        let result = agent
            .run(vec![Message::text(Role::User, "loop it")], &mut on_event)
            .await;

        match result {
            Err(AgentError::DoomLoop(name, _msgs)) => assert_eq!(name, "bash"),
            other => panic!("expected DoomLoop, got {other:?}"),
        }
    }
}
