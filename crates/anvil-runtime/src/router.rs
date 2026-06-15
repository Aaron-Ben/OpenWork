//! Agent 主循环:取消息 → 喂模型(流式)→ 解析工具调用 → 执行 → 回填 → 重复,
//! 直到模型返回不带工具调用的纯文本。

use std::path::PathBuf;

use anvil_core::ai::{
    ContentBlock, GenerateRequest, GenerateResponse, GenerateStreamCallback,
    GenerateStreamEvent, LlmProvider, Message, ProviderError, Role, ToolCallBlock, ToolCallState,
    ToolResultBlock, ToolResultState,
};
use anvil_tools::{
    ApprovalBridge, ApprovalDecision, ApprovalPolicy, ApprovalsReviewer, ToolContext, ToolOutput,
    ToolRegistry,
};
use thiserror::Error;

const DEFAULT_MAX_STEPS: usize = 20;

const AGENT_SYSTEM_PROMPT: &str = "\
You are Anvil, an autonomous coding agent operating in a working directory on the user's machine.\n\
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
            working_dir,
            approval_policy: ApprovalPolicy::Untrusted,
            approvals_reviewer: ApprovalsReviewer::User,
            approval_bridge: ApprovalBridge::new(),
            max_steps: DEFAULT_MAX_STEPS,
        }
    }
}

/// agent loop 向外发出的事件,供调用方(UI/测试)消费。
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Step(usize),
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, partial_input: String },
    ToolCallEnd { id: String },
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
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("exceeded max steps ({0})")]
    MaxStepsExceeded(usize),
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
    ) -> Result<String, AgentError> {
        let mut messages = vec![Message::text(Role::System, AGENT_SYSTEM_PROMPT)];
        messages.extend(history);
        let tool_defs = self.config.tools.definitions();

        for step in 1..=self.config.max_steps {
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

            let response = self.stream_once(req, &mut on_event).await?;

            // 记录 assistant 消息(text + tool calls)。
            let mut assistant_content: Vec<ContentBlock> = Vec::new();
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

            // 无工具调用 → 结束。
            if response.tool_calls.is_empty() {
                on_event(AgentEvent::Finished(response.text.clone()));
                return Ok(response.text);
            }

            // 执行工具并回填结果(含审批决策)。
            for tc in response.tool_calls {
                let input = serde_json::from_str::<serde_json::Value>(&tc.input)
                    .unwrap_or(serde_json::Value::Null);
                let output = self.execute_tool_call(&tc, input, &mut on_event).await;
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

        let response = loop {
            tokio::select! {
                biased;
                Some(event) = rx.recv() => forward_event(event, on_event),
                result = &mut provider_fut => break result?,
            }
        };
        while let Ok(event) = rx.try_recv() {
            forward_event(event, on_event);
        }
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
                match rx.await {
                    Ok(decision) => decision,
                    Err(_) => ApprovalDecision::Deny("approval cancelled".to_string()),
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
        };
        match self.config.tools.execute(name, input, &ctx).await {
            Ok(out) => out,
            Err(err) => ToolOutput::error(err.to_string()),
        }
    }
}

fn forward_event(event: GenerateStreamEvent, on_event: &mut impl FnMut(AgentEvent)) {
    match event {
        GenerateStreamEvent::TextDelta { delta } => on_event(AgentEvent::TextDelta(delta)),
        GenerateStreamEvent::ReasoningDelta { delta } => {
            on_event(AgentEvent::ReasoningDelta(delta))
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
            self.responses.lock().await.pop_front().ok_or_else(|| {
                ProviderError::InvalidRequest {
                    message: "FakeProvider exhausted".to_string(),
                }
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

        assert_eq!(result, "done");
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

        assert_eq!(result, "ok");
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
            .run(
                vec![Message::text(Role::User, "run it")],
                |event| {
                    assert!(
                        !matches!(event, AgentEvent::ApprovalRequest { .. }),
                        "Never policy must not request approval"
                    );
                },
            )
            .await
            .unwrap();

        assert_eq!(result, "done");
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

        assert_eq!(result, "done");
        let results = tool_results.lock().unwrap().clone();
        assert_eq!(results.len(), 1);
        assert!(results[0].0, "AutoReview should deny (error result)");
        assert!(
            !results[0].1.contains("SHOULD_NOT_RUN"),
            "AutoReview-denied command must not execute"
        );
    }
}
