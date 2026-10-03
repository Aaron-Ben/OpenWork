use serde::{Deserialize, Serialize};
use thiserror::Error;

use openwork_models::model::ToolResultArtifact;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Succeeded,
    Failed,
    Denied,
    Cancelled,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContent {
    Text { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorCode {
    ToolNotFound,
    InvalidArguments,
    PermissionDenied,
    Cancelled,
    Timeout,
    ExecutionFailed,
    OutcomeUnknown,
    /// 沙箱自检失败，bash 不执行（permissions.md §6）。
    SandboxUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolError {
    pub code: ToolErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub status: ToolResultStatus,
    pub content: Vec<ToolResultContent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ToolResultArtifact>,
    pub error: Option<ToolError>,
    /// 沙箱或文件工具围栏拒绝了其中的文件操作（permissions.md §7）。它是结果上的事实，
    /// 不是权限判定：命令照样执行了，退出码也照常给出。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sandbox_denied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ToolExecutionError {
    pub status: ToolResultStatus,
    pub code: ToolErrorCode,
    pub message: String,
    pub retryable: bool,
    /// 见 [`ToolResult::sandbox_denied`]。
    pub sandbox_denied: bool,
}

impl ToolExecutionError {
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new(
            ToolResultStatus::Failed,
            ToolErrorCode::InvalidArguments,
            message,
            false,
        )
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::new(
            ToolResultStatus::Denied,
            ToolErrorCode::PermissionDenied,
            message,
            false,
        )
    }

    /// 文件工具围栏按沙箱策略拒绝了路径；可以经越界重试。
    pub fn sandbox_denied(message: impl Into<String>) -> Self {
        Self {
            sandbox_denied: true,
            ..Self::denied(message)
        }
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(
            ToolResultStatus::Cancelled,
            ToolErrorCode::Cancelled,
            message,
            false,
        )
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(
            ToolResultStatus::Failed,
            ToolErrorCode::Timeout,
            message,
            false,
        )
    }

    pub fn execution(message: impl Into<String>) -> Self {
        Self::new(
            ToolResultStatus::Failed,
            ToolErrorCode::ExecutionFailed,
            message,
            false,
        )
    }

    pub fn outcome_unknown(message: impl Into<String>) -> Self {
        Self::new(
            ToolResultStatus::OutcomeUnknown,
            ToolErrorCode::OutcomeUnknown,
            message,
            false,
        )
    }

    fn new(
        status: ToolResultStatus,
        code: ToolErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retryable,
            sandbox_denied: false,
        }
    }
}

impl ToolResult {
    pub fn succeeded(text: impl Into<String>) -> Self {
        Self {
            status: ToolResultStatus::Succeeded,
            content: vec![ToolResultContent::Text { text: text.into() }],
            artifacts: Vec::new(),
            error: None,
            sandbox_denied: false,
        }
    }

    pub fn succeeded_with_artifact(text: impl Into<String>, artifact: ToolResultArtifact) -> Self {
        Self {
            status: ToolResultStatus::Succeeded,
            content: vec![ToolResultContent::Text { text: text.into() }],
            artifacts: vec![artifact],
            error: None,
            sandbox_denied: false,
        }
    }

    pub fn failed(code: ToolErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self::terminal(ToolResultStatus::Failed, code, message, retryable)
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::terminal(
            ToolResultStatus::Denied,
            ToolErrorCode::PermissionDenied,
            message,
            false,
        )
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::terminal(
            ToolResultStatus::Cancelled,
            ToolErrorCode::Cancelled,
            message,
            false,
        )
    }

    pub fn outcome_unknown(message: impl Into<String>) -> Self {
        Self::terminal(
            ToolResultStatus::OutcomeUnknown,
            ToolErrorCode::OutcomeUnknown,
            message,
            false,
        )
    }

    pub fn from_execution_error(error: ToolExecutionError) -> Self {
        let sandbox_denied = error.sandbox_denied;
        Self {
            sandbox_denied,
            ..Self::terminal(error.status, error.code, error.message, error.retryable)
        }
    }

    /// 标记沙箱拒绝了其中的文件操作（结果本身不变）。
    pub fn with_sandbox_denied(self) -> Self {
        Self {
            sandbox_denied: true,
            ..self
        }
    }

    pub fn is_error(&self) -> bool {
        self.status != ToolResultStatus::Succeeded
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .map(|content| match content {
                ToolResultContent::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn terminal(
        status: ToolResultStatus,
        code: ToolErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        let message = message.into();
        Self {
            status,
            content: vec![ToolResultContent::Text {
                text: message.clone(),
            }],
            artifacts: Vec::new(),
            error: Some(ToolError {
                code,
                message,
                retryable,
            }),
            sandbox_denied: false,
        }
    }
}
