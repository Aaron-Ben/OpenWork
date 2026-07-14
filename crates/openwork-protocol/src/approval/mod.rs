//! Human approval commands, events, and execution-policy outcomes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{ApprovalId, StepId, ToolRunId, TurnId};

/// User-selected policy supplied to the execution policy evaluator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    /// Every executable action requires explicit user approval.
    #[default]
    Untrusted,
    /// The execution policy may allow actions without human approval.
    Never,
}

/// Final outcome returned by Execution before an action is invoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionPolicyDecision {
    Allow,
    Deny { reason: String },
    RequireApproval { reason: String },
}

/// Recorded/live fact emitted when a Turn pauses for user approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequested {
    pub approval_id: ApprovalId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub tool_run_id: ToolRunId,
    pub tool_name: String,
    pub input: Value,
    pub reason: String,
}

/// User resolution for one pending approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalResolution {
    Allow,
    Deny { reason: String },
}

/// Command routed by the application layer back to the owning Turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveApproval {
    pub turn_id: TurnId,
    pub approval_id: ApprovalId,
    pub resolution: ApprovalResolution,
}

/// Recorded/live fact emitted after Core applies a user resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResolved {
    pub approval_id: ApprovalId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub tool_run_id: ToolRunId,
    pub resolution: ApprovalResolution,
}
