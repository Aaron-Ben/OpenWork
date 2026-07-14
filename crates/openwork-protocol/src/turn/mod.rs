//! Durable facts emitted by one Turn and the Port used to record them.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    approval::{ApprovalRequested, ApprovalResolved},
    capability::{Observation, ObservationStatus},
    domain::{StepId, ToolRunId},
    model::ContentBlock,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStarted {
    pub step_id: StepId,
    pub step_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageRecorded {
    pub message_id: String,
    pub step_id: StepId,
    pub parts: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunRequested {
    pub step_id: StepId,
    pub tool_run_id: ToolRunId,
    /// ID supplied by the model provider. It is correlation data, not runtime identity.
    pub provider_tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunStarted {
    pub step_id: StepId,
    pub tool_run_id: ToolRunId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunFinished {
    pub step_id: StepId,
    pub tool_run_id: ToolRunId,
    pub observation: Observation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMessageRecorded {
    pub message_id: String,
    pub step_id: StepId,
    pub tool_run_id: ToolRunId,
    pub parts: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepCompleted {
    pub step_id: StepId,
    pub step_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFailed {
    pub step_id: StepId,
    pub step_index: usize,
    pub message: String,
}

/// Facts whose successful append is part of Turn control flow.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnRecordedEvent {
    StepStarted(StepStarted),
    AssistantMessageRecorded(AssistantMessageRecorded),
    ToolRunRequested(ToolRunRequested),
    ApprovalRequested(ApprovalRequested),
    ApprovalResolved(ApprovalResolved),
    ToolRunStarted(ToolRunStarted),
    ToolRunFinished(ToolRunFinished),
    ToolMessageRecorded(ToolMessageRecorded),
    StepCompleted(StepCompleted),
    StepFailed(StepFailed),
}

impl TurnRecordedEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::StepStarted(_) => "step_started",
            Self::AssistantMessageRecorded(_) => "assistant_message_recorded",
            Self::ToolRunRequested(_) => "tool_run_requested",
            Self::ApprovalRequested(_) => "approval_requested",
            Self::ApprovalResolved(_) => "approval_resolved",
            Self::ToolRunStarted(_) => "tool_run_started",
            Self::ToolRunFinished(event) => match event.observation.status {
                ObservationStatus::Succeeded => "tool_run_completed",
                ObservationStatus::Failed => "tool_run_failed",
                ObservationStatus::Denied => "tool_run_denied",
                ObservationStatus::Cancelled => "tool_run_cancelled",
                ObservationStatus::OutcomeUnknown => "tool_run_outcome_unknown",
            },
            Self::ToolMessageRecorded(_) => "tool_message_recorded",
            Self::StepCompleted(_) => "step_completed",
            Self::StepFailed(_) => "step_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TurnRecordError {
    #[error("turn lifecycle persistence unavailable: {message}")]
    Unavailable { message: String },
    #[error("invalid turn lifecycle event: {message}")]
    InvalidEvent { message: String },
    #[error("turn lifecycle changed concurrently: {message}")]
    Conflict { message: String },
}

/// Persistence boundary supplied by the host. Recorded events are awaited;
/// best-effort UI streaming does not implement this Port.
#[async_trait]
pub trait TurnRecorderPort: Send + Sync {
    async fn append(&self, events: Vec<TurnRecordedEvent>) -> Result<(), TurnRecordError>;
}
