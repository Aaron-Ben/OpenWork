//! Agent 主循环:取消息 → 喂模型(流式)→ 解析工具调用 → 执行 → 回填 → 重复,
//! 直到模型返回不带工具调用的纯文本。

use std::path::PathBuf;
use std::sync::Arc;

use anvil_core::ai::{
    ContentBlock, GenerateRequest, GenerateResponse, GenerateStreamCallback,
    GenerateStreamEvent, LlmProvider, Message, ProviderError, Role, ToolCallBlock, ToolCallState,
    ToolResultBlock, ToolResultState,
};
use anvil_tools::{AllowAllApproval, Approval, ApprovalDecision, ToolContext, ToolOutput, ToolRegistry};
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
    pub approval: Arc<dyn Approval>,
    pub max_steps: usize,
}

impl AgentConfig {
    /// 用内置工具(read/write/list/bash)与默认放行审批构造配置。
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
            approval: Arc::new(AllowAllApproval),
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

            // 执行工具并回填结果。
            for tc in response.tool_calls {
                let input = serde_json::from_str::<serde_json::Value>(&tc.input)
                    .unwrap_or(serde_json::Value::Null);
                let output = match self.config.approval.check(&tc.name, &input) {
                    ApprovalDecision::Deny(reason) => ToolOutput::error(reason),
                    ApprovalDecision::Allow => {
                        let ctx = ToolContext {
                            working_dir: self.config.working_dir.clone(),
                            approval: Arc::clone(&self.config.approval),
                        };
                        match self.config.tools.execute(&tc.name, input, &ctx).await {
                            Ok(out) => out,
                            Err(err) => ToolOutput::error(err.to_string()),
                        }
                    }
                };
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
