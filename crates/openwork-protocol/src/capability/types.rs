use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::model::ToolDefinition;

/// 声明侧风险提示。最终权限结论必须由 Execution 结合实际参数计算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRiskHint {
    ReadOnly,
    WorkspaceMutation,
    ProcessExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub risk_hint: CapabilityRiskHint,
}

impl CapabilitySpec {
    pub fn model_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub name: String,
    pub input: Value,
}

impl ActionRequest {
    pub fn new(name: impl Into<String>, input: Value) -> Self {
        Self {
            name: name.into(),
            input,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Succeeded,
    Failed,
    Denied,
    Cancelled,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObservationContent {
    Text { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationErrorCode {
    CapabilityNotFound,
    InvalidArguments,
    HandlerNotFound,
    ApprovalDenied,
    PermissionDenied,
    Cancelled,
    Timeout,
    ExecutionFailed,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationError {
    pub code: ObservationErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub status: ObservationStatus,
    pub content: Vec<ObservationContent>,
    pub error: Option<ObservationError>,
}

impl Observation {
    pub fn succeeded(text: impl Into<String>) -> Self {
        Self {
            status: ObservationStatus::Succeeded,
            content: vec![ObservationContent::Text { text: text.into() }],
            error: None,
        }
    }

    pub fn failed(code: ObservationErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self::terminal(ObservationStatus::Failed, code, message, retryable)
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::terminal(
            ObservationStatus::Denied,
            ObservationErrorCode::PermissionDenied,
            message,
            false,
        )
    }

    pub fn approval_denied(message: impl Into<String>) -> Self {
        Self::terminal(
            ObservationStatus::Denied,
            ObservationErrorCode::ApprovalDenied,
            message,
            false,
        )
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::terminal(
            ObservationStatus::Cancelled,
            ObservationErrorCode::Cancelled,
            message,
            false,
        )
    }

    pub fn outcome_unknown(message: impl Into<String>) -> Self {
        Self::terminal(
            ObservationStatus::OutcomeUnknown,
            ObservationErrorCode::OutcomeUnknown,
            message,
            false,
        )
    }

    pub fn is_error(&self) -> bool {
        self.status != ObservationStatus::Succeeded
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .map(|content| match content {
                ObservationContent::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn terminal(
        status: ObservationStatus,
        code: ObservationErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        let message = message.into();
        Self {
            status,
            content: vec![ObservationContent::Text {
                text: message.clone(),
            }],
            error: Some(ObservationError {
                code,
                message,
                retryable,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CapabilityResolveError {
    #[error("capability resolver unavailable: {0}")]
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActionInvokeError {
    #[error("action handler not found: {0}")]
    HandlerNotFound(String),
    #[error("action invocation failed: {0}")]
    Failed(String),
}
