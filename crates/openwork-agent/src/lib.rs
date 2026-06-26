//! Agent 主循环:取消息 → 喂模型(流式)→ 解析工具调用 → 执行 → 回填 → 重复,
//! 直到模型返回不带工具调用的纯文本。

use std::collections::VecDeque;
use std::path::PathBuf;

use openwork_permissions::{
    ApprovalBridge, ApprovalDecision, ApprovalPolicy, ApprovalsReviewer, PermissionProfile,
};
use openwork_protocol::ai::{
    ContentBlock, GenerateRequest, GenerateResponse, GenerateStreamCallback, GenerateStreamEvent,
    LlmProvider, Message, ProviderError, Role, ToolCallBlock, ToolCallState, ToolResultBlock,
    ToolResultState,
};
use openwork_tools::{ToolContext, ToolOutput, ToolRegistry};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_STEPS: usize = 20;
/// 连续同名 + 规范化同参的工具调用达到此次数,判定为 doom loop。
const DOOM_LOOP_THRESHOLD: usize = 3;

const AGENT_SYSTEM_PROMPT: &str = "\
You are OpenWork, an autonomous coding agent operating in a working directory on the user's machine.\n\
You have tools to read and write files, list directories, and run shell commands. \
Call a tool whenever it helps you make progress toward the task. \
When the task is done or you have a final answer, respond with plain text and no tool calls.";

pub struct AgentConfig {
    pub provider: Box<dyn LlmProvider>,
    pub model: String,
    pub tools: ToolRegistry,
    pub working_dir: PathBuf,
    /// 何时需要对工具调用发起审批(对齐 codex `AskForApproval`)。
    pub approval_policy: ApprovalPolicy,
    /// 需要审批时由谁来审(对齐 codex `ApprovalsReviewer`)。
    pub approvals_reviewer: ApprovalsReviewer,
    /// 异步审批回传桥:`User` 审阅者经此等待宿主确认。
    pub approval_bridge: ApprovalBridge,
    /// 取消令牌:外部触发 `cancel()` 后,agent loop 在下一个 await 点终止并返回
    /// [`AgentError::Cancelled`],携带截止当前的对话轨迹。
    pub cancel: CancellationToken,
    /// 工具执行权限数据模型。当前用于应用层路径检查;未来可映射到真实沙箱。
    pub permission_profile: PermissionProfile,
    pub max_steps: usize,
}

impl AgentConfig {
    /// 用内置工具(read/write/list/bash)与默认审批配置构造。
    /// 默认 `Untrusted` + `User`:每次工具调用都需宿主确认。
    pub fn new(
        provider: Box<dyn LlmProvider>,
        model: impl Into<String>,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            tools: ToolRegistry::with_builtin(),
            working_dir: working_dir.clone(),
            approval_policy: ApprovalPolicy::Untrusted,
            approvals_reviewer: ApprovalsReviewer::User,
            approval_bridge: ApprovalBridge::new(),
            cancel: CancellationToken::new(),
            permission_profile: PermissionProfile::workspace_write(working_dir),
            max_steps: DEFAULT_MAX_STEPS,
        }
    }
}

/// agent loop 向外发出的事件,供调用方(UI/测试)消费。
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Step(usize),
    LlmStepStart {
        index: usize,
    },
    LlmStepFinish {
        index: usize,
        reason: String,
        usage: Option<openwork_protocol::ai::TokenUsage>,
    },
    LlmFinish {
        reason: String,
        usage: Option<openwork_protocol::ai::TokenUsage>,
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
        name: String,
        output: Vec<ContentBlock>,
        is_error: bool,
    },
    /// 工具调用需要人工审批时发出;宿主须用相同 `id` 通过
    /// `ApprovalBridge::resolve` 回传决定,否则 agent 会一直 await。
    ApprovalRequest {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Finished(String),
    /// 检测到 doom loop(连续重复同名同参工具调用),agent 即将停止。
    DoomLoopDetected {
        repeated: String,
    },
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
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
        &self,
        history: Vec<Message>,
        mut on_event: impl FnMut(AgentEvent),
    ) -> Result<RunResult, AgentError> {
        let mut messages = vec![Message::text(Role::System, AGENT_SYSTEM_PROMPT)];
        messages.extend(history);
        let tool_defs = self.config.tools.definitions();
        // 最近若干次工具调用的 (name, 规范化 input),用于 doom-loop 检测。
        let mut recent: VecDeque<(String, String)> = VecDeque::new();

        for step in 1..=self.config.max_steps {
            if self.config.cancel.is_cancelled() {
                return Err(AgentError::Cancelled(messages[1..].to_vec()));
            }
            on_event(AgentEvent::Step(step));

            let req = GenerateRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                temperature: None,
                max_tokens: None,
                stream: true,
                thinking: None,
                tools: tool_defs.clone(),
            };

            let response = match self.stream_once(req, &mut on_event).await {
                Ok(response) => response,
                Err(AgentError::Cancelled(_)) => {
                    return Err(AgentError::Cancelled(messages[1..].to_vec()));
                }
                Err(error) => return Err(error),
            };

            // 记录 assistant 消息(thinking? + text + tool calls)。
            // thinking 放最前,与流式渲染顺序一致;落库后才不会在 reload 后丢失。
            let mut assistant_content: Vec<ContentBlock> = Vec::new();
            if let Some(reasoning) = &response.reasoning_text
                && !reasoning.is_empty()
            {
                assistant_content.push(ContentBlock::thinking(reasoning.clone()));
            }
            if !response.text.is_empty() {
                assistant_content.push(ContentBlock::text(response.text.clone()));
            }
            for tc in &response.tool_calls {
                assistant_content.push(ContentBlock::ToolCall(ToolCallBlock {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                    state: ToolCallState::Submitted,
                }));
            }
            if !assistant_content.is_empty() {
                messages.push(Message {
                    role: Role::Assistant,
                    content: assistant_content,
                });
            }

            // 无工具调用 → 结束。返回完整轨迹(跳过 system prompt)。
            if response.tool_calls.is_empty() {
                on_event(AgentEvent::Finished(response.text.clone()));
                return Ok(RunResult {
                    text: response.text,
                    messages: messages[1..].to_vec(),
                });
            }

            // 执行工具并回填结果(含审批决策)。
            for tc in response.tool_calls {
                if self.config.cancel.is_cancelled() {
                    return Err(AgentError::Cancelled(messages[1..].to_vec()));
                }
                let input = serde_json::from_str::<serde_json::Value>(&tc.input)
                    .unwrap_or(serde_json::Value::Null);

                // doom-loop 检测:连续 N 次同名 + 规范化同参 → 停止。
                let key = (tc.name.clone(), normalize_json(&input));
                recent.push_back(key.clone());
                if recent.len() > DOOM_LOOP_THRESHOLD {
                    recent.pop_front();
                }
                if recent.len() == DOOM_LOOP_THRESHOLD && recent.iter().all(|k| k == &key) {
                    on_event(AgentEvent::DoomLoopDetected {
                        repeated: tc.name.clone(),
                    });
                    return Err(AgentError::DoomLoop(
                        tc.name.clone(),
                        messages[1..].to_vec(),
                    ));
                }

                let output = self.execute_tool_call(&tc, input, &mut on_event).await;
                if self.config.cancel.is_cancelled() {
                    return Err(AgentError::Cancelled(messages[1..].to_vec()));
                }
                mark_tool_call_finished(&mut messages, &tc.id);
                on_event(AgentEvent::ToolResult {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    output: output.content.clone(),
                    is_error: output.is_error,
                });
                messages.push(Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult(ToolResultBlock {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        output: output.content,
                        state: if output.is_error {
                            ToolResultState::Error
                        } else {
                            ToolResultState::Success
                        },
                    })],
                });
            }
        }

        Err(AgentError::MaxStepsExceeded(self.config.max_steps))
    }

    /// 单次流式调用 provider,实时转发事件,返回累积响应。
    async fn stream_once(
        &self,
        req: GenerateRequest,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> Result<GenerateResponse, AgentError> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<GenerateStreamEvent>();
        let callback: GenerateStreamCallback = Box::new(move |event| {
            let _ = tx.send(event);
        });

        let provider_fut = self.config.provider.stream_generate(req, callback);
        tokio::pin!(provider_fut);
        let mut lifecycle = StreamLifecycle::new();
        forward_event(
            GenerateStreamEvent::StepStart { index: 0 },
            on_event,
            &mut lifecycle,
        );

        let response = loop {
            tokio::select! {
                biased;
                _ = self.config.cancel.cancelled() => return Err(AgentError::Cancelled(Vec::new())),
                Some(event) = rx.recv() => forward_event(event, on_event, &mut lifecycle),
                result = &mut provider_fut => break result?,
            }
        };
        while let Ok(event) = rx.try_recv() {
            forward_event(event, on_event, &mut lifecycle);
        }
        lifecycle.close_open(on_event);
        let reason = if response.tool_calls.is_empty() {
            "stop"
        } else {
            "tool_calls"
        }
        .to_string();
        let usage = response.usage;
        forward_event(
            GenerateStreamEvent::StepFinish {
                index: 0,
                reason: reason.clone(),
                usage,
            },
            on_event,
            &mut lifecycle,
        );
        forward_event(
            GenerateStreamEvent::Finish { reason, usage },
            on_event,
            &mut lifecycle,
        );
        Ok(response)
    }

    /// 对单个工具调用做审批决策并执行,返回工具输出。
    async fn execute_tool_call(
        &self,
        tc: &ToolCallBlock,
        input: serde_json::Value,
        on_event: &mut impl FnMut(AgentEvent),
    ) -> ToolOutput {
        let policy = self.config.approval_policy;
        let reviewer = self.config.approvals_reviewer;

        // 1. 该调用是否需要审批?
        if !policy.requires_approval(&tc.name, &input) {
            return self.run_tool(&tc.name, input).await;
        }

        // 2. 需要审批 —— 谁来审?
        let decision = match reviewer {
            ApprovalsReviewer::AutoReview => {
                // 留接口:guardian 风格 LLM 自动审暂未实现,保守拒绝(不静默放行)。
                ApprovalDecision::Deny("auto_review not yet implemented".to_string())
            }
            ApprovalsReviewer::User => {
                // 经桥异步等待宿主(前端 UI / 测试)确认。
                on_event(AgentEvent::ApprovalRequest {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: input.clone(),
                });
                let rx = self.config.approval_bridge.register(&tc.id).await;
                tokio::select! {
                    biased;
                    _ = self.config.cancel.cancelled() => {
                        return ToolOutput::error("cancelled");
                    }
                    decision = rx => match decision {
                        Ok(decision) => decision,
                        Err(_) => ApprovalDecision::Deny("approval cancelled".to_string()),
                    },
                }
            }
        };

        match decision {
            ApprovalDecision::Allow => self.run_tool(&tc.name, input).await,
            ApprovalDecision::Deny(reason) => ToolOutput::error(reason),
        }
    }

    /// 真正执行工具(无审批),负责构造 ToolContext 与错误兜底。
    async fn run_tool(&self, name: &str, input: serde_json::Value) -> ToolOutput {
        let ctx = ToolContext {
            working_dir: self.config.working_dir.clone(),
            permissions: self.config.permission_profile.clone(),
            cancel: self.config.cancel.clone(),
        };
        tokio::select! {
            biased;
            _ = self.config.cancel.cancelled() => ToolOutput::error("cancelled"),
            result = self.config.tools.execute(name, input, &ctx) => match result {
                Ok(out) => out,
                Err(err) => ToolOutput::error(err.to_string()),
            },
        }
    }
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
    event: GenerateStreamEvent,
    on_event: &mut impl FnMut(AgentEvent),
    lifecycle: &mut StreamLifecycle,
) {
    match event {
        GenerateStreamEvent::StepStart { index } => on_event(AgentEvent::LlmStepStart { index }),
        GenerateStreamEvent::StepFinish {
            index,
            reason,
            usage,
        } => on_event(AgentEvent::LlmStepFinish {
            index,
            reason,
            usage,
        }),
        GenerateStreamEvent::Finish { reason, usage } => {
            on_event(AgentEvent::LlmFinish { reason, usage })
        }
        GenerateStreamEvent::TextStart { id } => {
            lifecycle.text_open = true;
            on_event(AgentEvent::TextStart { id });
        }
        GenerateStreamEvent::TextDelta { delta } => {
            if !lifecycle.text_open {
                lifecycle.text_open = true;
                on_event(AgentEvent::TextStart {
                    id: "text-0".to_string(),
                });
            }
            on_event(AgentEvent::TextDelta(delta));
        }
        GenerateStreamEvent::TextEnd { id } => {
            lifecycle.text_open = false;
            on_event(AgentEvent::TextEnd { id });
        }
        GenerateStreamEvent::ReasoningStart { id } => {
            lifecycle.reasoning_open = true;
            on_event(AgentEvent::ReasoningStart { id });
        }
        GenerateStreamEvent::ReasoningDelta { delta } => {
            if !lifecycle.reasoning_open {
                lifecycle.reasoning_open = true;
                on_event(AgentEvent::ReasoningStart {
                    id: "reasoning-0".to_string(),
                });
            }
            on_event(AgentEvent::ReasoningDelta(delta))
        }
        GenerateStreamEvent::ReasoningEnd { id } => {
            lifecycle.reasoning_open = false;
            on_event(AgentEvent::ReasoningEnd { id });
        }
        GenerateStreamEvent::ToolCallStart { id, name } => {
            on_event(AgentEvent::ToolCallStart { id, name })
        }
        GenerateStreamEvent::ToolCallDelta { id, partial_input } => {
            on_event(AgentEvent::ToolCallDelta { id, partial_input })
        }
        GenerateStreamEvent::ToolCallEnd { id } => on_event(AgentEvent::ToolCallEnd { id }),
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
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    /// 按 `stream_generate` 调用顺序依次返回预设响应的假 provider。
    struct FakeProvider {
        responses: tokio::sync::Mutex<VecDeque<GenerateResponse>>,
    }

    impl FakeProvider {
        fn new(responses: Vec<GenerateResponse>) -> Self {
            Self {
                responses: tokio::sync::Mutex::new(responses.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for FakeProvider {
        async fn generate(&self, _req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
            Err(ProviderError::InvalidRequest {
                message: "FakeProvider only supports stream_generate".to_string(),
            })
        }

        async fn stream_generate(
            &self,
            _req: GenerateRequest,
            _on_event: GenerateStreamCallback,
        ) -> Result<GenerateResponse, ProviderError> {
            self.responses
                .lock()
                .await
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidRequest {
                    message: "FakeProvider exhausted".to_string(),
                })
        }
    }

    fn tool_call_response(id: &str, name: &str, input: serde_json::Value) -> GenerateResponse {
        GenerateResponse {
            text: String::new(),
            reasoning_text: None,
            usage: None,
            raw: serde_json::Value::Null,
            tool_calls: vec![ToolCallBlock {
                id: id.to_string(),
                name: name.to_string(),
                input: input.to_string(),
                state: ToolCallState::Submitted,
            }],
        }
    }

    fn text_response(text: &str) -> GenerateResponse {
        GenerateResponse {
            text: text.to_string(),
            reasoning_text: None,
            usage: None,
            raw: serde_json::Value::Null,
            tool_calls: Vec::new(),
        }
    }

    fn untrusted_user_config(provider: Box<dyn LlmProvider>) -> AgentConfig {
        let mut config = AgentConfig::new(provider, "fake", PathBuf::from("."));
        config.approval_policy = ApprovalPolicy::Untrusted;
        config.approvals_reviewer = ApprovalsReviewer::User;
        config
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

    /// 构造 on_event 闭包:遇到 ApprovalRequest 时用 bridge 自动 resolve(给定决定),
    /// 并记录是否出现过 ApprovalRequest。返回 (闭包, seen 标志)。
    fn auto_resolve_on_event(
        bridge: ApprovalBridge,
        decision: ApprovalDecision,
    ) -> (impl FnMut(AgentEvent), Arc<StdMutex<bool>>) {
        let seen = Arc::new(StdMutex::new(false));
        let seen_cb = seen.clone();
        let closure = move |event: AgentEvent| {
            if let AgentEvent::ApprovalRequest { id, .. } = event {
                *seen_cb.lock().unwrap() = true;
                let bridge = bridge.clone();
                let id = id.clone();
                let decision = decision.clone();
                tokio::spawn(async move {
                    let _ = bridge.resolve(&id, decision).await;
                });
            }
        };
        (closure, seen)
    }

    #[tokio::test]
    async fn untrusted_user_allow_executes_tool_then_finishes() {
        let bridge = ApprovalBridge::new();
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "true"})),
            text_response("done"),
        ]);
        let mut config = untrusted_user_config(Box::new(provider));
        config.approval_bridge = bridge.clone();
        let agent = Agent::new(config);

        let (mut on_event, seen) = auto_resolve_on_event(bridge, ApprovalDecision::Allow);
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
        let bridge = ApprovalBridge::new();
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "echo SHOULD_NOT_RUN"})),
            text_response("ok"),
        ]);
        let mut config = untrusted_user_config(Box::new(provider));
        config.approval_bridge = bridge.clone();
        let agent = Agent::new(config);

        let tool_results: Arc<StdMutex<Vec<(bool, String)>>> = Arc::new(StdMutex::new(Vec::new()));
        let results_cb = tool_results.clone();
        let bridge_cb = bridge.clone();
        let result = agent
            .run(
                vec![Message::text(Role::User, "run it")],
                move |event| match event {
                    AgentEvent::ApprovalRequest { id, .. } => {
                        let bridge = bridge_cb.clone();
                        tokio::spawn(async move {
                            let _ = bridge
                                .resolve(&id, ApprovalDecision::Deny("user said no".into()))
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
        let mut config = AgentConfig::new(Box::new(provider), "fake", PathBuf::from("."));
        config.approval_policy = ApprovalPolicy::Never;
        // 不注入有效 bridge:Never 不应触碰它;若误发 ApprovalRequest 会因无 resolve 而卡死,
        // 测试会在超时/死锁暴露 —— 但这里用默认空 bridge,Never 路径根本不会 await 它。
        let agent = Agent::new(config);

        let result = agent
            .run(vec![Message::text(Role::User, "run it")], |event| {
                assert!(
                    !matches!(event, AgentEvent::ApprovalRequest { .. }),
                    "Never policy must not request approval"
                );
            })
            .await
            .unwrap();

        assert_eq!(result.text, "done");
    }

    #[tokio::test]
    async fn autoreview_reviewer_denies_without_bridge_or_execution() {
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "echo SHOULD_NOT_RUN"})),
            text_response("done"),
        ]);
        let mut config = AgentConfig::new(Box::new(provider), "fake", PathBuf::from("."));
        config.approval_policy = ApprovalPolicy::Untrusted;
        config.approvals_reviewer = ApprovalsReviewer::AutoReview;
        let agent = Agent::new(config);

        let tool_results: Arc<StdMutex<Vec<(bool, String)>>> = Arc::new(StdMutex::new(Vec::new()));
        let results_cb = tool_results.clone();
        let result = agent
            .run(
                vec![Message::text(Role::User, "run it")],
                move |event| match event {
                    AgentEvent::ApprovalRequest { .. } => {
                        panic!("AutoReview must not emit ApprovalRequest");
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

        assert_eq!(result.text, "done");
        let results = tool_results.lock().unwrap().clone();
        assert_eq!(results.len(), 1);
        assert!(results[0].0, "AutoReview should deny (error result)");
        assert!(
            !results[0].1.contains("SHOULD_NOT_RUN"),
            "AutoReview-denied command must not execute"
        );
    }

    #[tokio::test]
    async fn run_returns_full_message_trace() {
        let bridge = ApprovalBridge::new();
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "true"})),
            text_response("done"),
        ]);
        let mut config = untrusted_user_config(Box::new(provider));
        config.approval_bridge = bridge.clone();
        let agent = Agent::new(config);

        let (mut on_event, _seen) = auto_resolve_on_event(bridge, ApprovalDecision::Allow);
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
        let config = AgentConfig::new(Box::new(provider), "fake", PathBuf::from("."));
        config.cancel.cancel();
        let agent = Agent::new(config);

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
        let bridge = ApprovalBridge::new();
        // 三个完全相同的 bash 调用 → 第三个触发 doom-loop。
        let provider = FakeProvider::new(vec![
            tool_call_response("call-1", "bash", json!({"command": "echo hi"})),
            tool_call_response("call-2", "bash", json!({"command": "echo hi"})),
            tool_call_response("call-3", "bash", json!({"command": "echo hi"})),
        ]);
        let mut config = untrusted_user_config(Box::new(provider));
        config.approval_bridge = bridge.clone();
        let agent = Agent::new(config);

        let (mut on_event, _seen) = auto_resolve_on_event(bridge, ApprovalDecision::Allow);
        let result = agent
            .run(vec![Message::text(Role::User, "loop it")], &mut on_event)
            .await;

        match result {
            Err(AgentError::DoomLoop(name, _msgs)) => assert_eq!(name, "bash"),
            other => panic!("expected DoomLoop, got {other:?}"),
        }
    }
}
