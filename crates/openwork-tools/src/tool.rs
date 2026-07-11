use std::path::PathBuf;

use async_trait::async_trait;
use openwork_permissions::{AccessKind, PermissionProfile};
use openwork_protocol::model::{ContentBlock, ToolDefinition};
use thiserror::Error;

/// 工具契约:可被 agent 调用的能力单元。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数的 JSON Schema。
    fn parameters(&self) -> serde_json::Value;

    /// 派生给模型的工具声明（喂给 `ModelRequest::tools`）。
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput;
}

/// 工具执行上下文:工作目录。每次执行由 agent loop 注入。
/// 审批不再由工具内部处理,统一收归编排层(agent loop);能进入 `execute` 即已获批准。
pub struct ToolContext {
    pub working_dir: PathBuf,
    pub permissions: PermissionProfile,
    pub cancel: tokio_util::sync::CancellationToken,
}

impl ToolContext {
    pub fn check_path(&self, path: &std::path::Path, kind: AccessKind) -> Result<(), String> {
        self.permissions.allows(path, kind)
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
