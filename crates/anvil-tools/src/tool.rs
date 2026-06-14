use std::path::PathBuf;
use std::sync::Arc;

use anvil_core::ai::{ContentBlock, ToolDefinition};
use async_trait::async_trait;
use thiserror::Error;

/// 工具契约:可被 agent 调用的能力单元。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数的 JSON Schema。
    fn parameters(&self) -> serde_json::Value;

    /// 派生给模型的工具声明(喂给 `GenerateRequest::tools`)。
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput;
}

/// 工具执行上下文:工作目录 + 审批策略。每次执行由 agent loop 注入。
/// `approval` 用 `Arc` 以便跨多轮工具调用共享同一份策略。
pub struct ToolContext {
    pub working_dir: PathBuf,
    pub approval: Arc<dyn Approval>,
}

/// 工具执行前的审批钩子。默认实现放行;后续可接入交互式确认或权限规则。
pub trait Approval: Send + Sync {
    fn check(&self, tool: &str, input: &serde_json::Value) -> ApprovalDecision;
}

#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    Allow,
    Deny(String),
}

/// 默认放行所有工具调用(便于自动化测试与无交互场景)。
pub struct AllowAllApproval;

impl Approval for AllowAllApproval {
    fn check(&self, _tool: &str, _input: &serde_json::Value) -> ApprovalDecision {
        ApprovalDecision::Allow
    }
}

/// 工具执行结果。`content` 复用 ContentBlock(通常用 Text block 承载文本输出)。
pub struct ToolOutput {
    pub content: Vec<ContentBlock>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(text)],
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(message)],
            is_error: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
}
