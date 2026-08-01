use openwork_tools::{ApprovalCard, ToolResultStatus};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ClientRequestId, SessionId, ToolCallId, TurnId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModel {
    pub model_id: Option<String>,
    pub provider_kind: String,
    pub model_name: String,
}

impl ResolvedModel {
    pub fn new(
        model_id: Option<impl Into<String>>,
        provider_kind: impl Into<String>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.map(Into::into),
            provider_kind: provider_kind.into(),
            model_name: model_name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowSession,
    AcceptEdits,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub tool_call_id: ToolCallId,
    pub provider_call_id: String,
    pub tool_name: String,
    pub card: ApprovalCard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAccepted {
    pub turn_id: TurnId,
    pub client_request_id: ClientRequestId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TurnOutcome {
    Completed { final_text: String },
    Failed { code: String, message: String },
    Cancelled,
}

impl TurnOutcome {
    pub fn terminal_tool_status(&self) -> ToolResultStatus {
        match self {
            Self::Completed { .. } => ToolResultStatus::Succeeded,
            Self::Failed { .. } => ToolResultStatus::Failed,
            Self::Cancelled => ToolResultStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionError {
    #[error("session actor stopped")]
    ActorStopped,
    #[error("session already has an active turn: {0}")]
    Busy(TurnId),
    #[error("turn is not active: {0}")]
    TurnNotActive(TurnId),
    #[error("permission request is not pending for tool call: {0}")]
    PermissionNotPending(ToolCallId),
    #[error("permission decision is not available for tool call: {0}")]
    PermissionDecisionUnavailable(ToolCallId),
    #[error("turn input must not be empty")]
    EmptyInput,
    #[error("context window token capacity must be positive")]
    InvalidContextWindowTokens,
    #[error(
        "session Conversation checkpoint changed but the in-memory projection could not be installed; reload the Session"
    )]
    ReloadRequired,
}
