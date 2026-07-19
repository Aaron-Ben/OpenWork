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
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ToolExecutionError {
    pub status: ToolResultStatus,
    pub code: ToolErrorCode,
    pub message: String,
    pub retryable: bool,
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
        }
    }

    pub fn succeeded_with_artifact(text: impl Into<String>, artifact: ToolResultArtifact) -> Self {
        Self {
            status: ToolResultStatus::Succeeded,
            content: vec![ToolResultContent::Text { text: text.into() }],
            artifacts: vec![artifact],
            error: None,
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
        Self::terminal(error.status, error.code, error.message, error.retryable)
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
        }
    }
}
