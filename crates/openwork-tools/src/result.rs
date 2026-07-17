use serde::{Deserialize, Serialize};

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
    HandlerNotFound,
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
    pub error: Option<ToolError>,
}

impl ToolResult {
    pub fn succeeded(text: impl Into<String>) -> Self {
        Self {
            status: ToolResultStatus::Succeeded,
            content: vec![ToolResultContent::Text { text: text.into() }],
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
            error: Some(ToolError {
                code,
                message,
                retryable,
            }),
        }
    }
}
