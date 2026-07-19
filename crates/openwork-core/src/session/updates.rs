use serde::{Deserialize, Serialize};
use serde_json::Value;

use openwork_models::model::ToolResultArtifact;

use super::{
    ClientRequestId, PermissionDecision, PermissionRequest, SessionId, ToolCallId, TurnId,
    TurnOutcome,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Starting,
    RunningModel,
    RunningTools,
    WaitingPermission,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveToolCall {
    pub tool_call_id: ToolCallId,
    pub provider_call_id: String,
    pub name: String,
    pub input: Value,
    pub status: String,
    pub output: Option<String>,
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ToolResultArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolProgressUpdate {
    Stdout { chunk: String },
    Stderr { chunk: String },
    Message { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SessionRuntimeSnapshot {
    Idle,
    Running {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        phase: SessionPhase,
        draft_text: String,
        draft_reasoning: String,
        tool_calls: Vec<LiveToolCall>,
        pending_permission: Option<Box<PermissionRequest>>,
    },
    Terminal {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        outcome: TurnOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub version: u16,
    pub session_id: SessionId,
    pub last_update_sequence: u64,
    pub runtime: SessionRuntimeSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SessionUpdate {
    TurnStarted {
        client_request_id: ClientRequestId,
    },
    PhaseChanged {
        phase: SessionPhase,
    },
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    DraftCleared,
    ToolCallStarted {
        tool_call: LiveToolCall,
    },
    ToolCallProgress {
        tool_call_id: ToolCallId,
        progress: ToolProgressUpdate,
    },
    ToolCallFinished {
        tool_call_id: ToolCallId,
        provider_call_id: String,
        tool_name: String,
        status: String,
        output: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<ToolResultArtifact>,
    },
    PermissionRequested {
        request: PermissionRequest,
    },
    PermissionResolved {
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    },
    TurnFinished {
        outcome: TurnOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateEnvelope {
    pub version: u16,
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub sequence: u64,
    pub occurred_at_ms: u64,
    pub update: SessionUpdate,
}
