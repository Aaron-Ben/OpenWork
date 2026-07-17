use async_trait::async_trait;
use openwork_models::model::TokenUsage;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::TurnId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Running,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
    OutcomeUnknown,
}

impl TraceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCallStarted {
    pub span_id: String,
    pub turn_id: TurnId,
    pub sequence: i64,
    pub model_id: Option<String>,
    pub resolved_model_name: String,
    pub started_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ModelCallFinished {
    pub started: ModelCallStarted,
    pub status: TraceStatus,
    pub provider_request_id: Option<String>,
    pub attempt_count: i32,
    pub usage: Option<TokenUsage>,
    pub ended_at: OffsetDateTime,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolCallStarted {
    pub span_id: String,
    pub turn_id: TurnId,
    pub parent_span_id: String,
    pub sequence: i64,
    pub provider_call_id: String,
    pub requested_tool_name: String,
    pub started_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ToolCallFinished {
    pub started: ToolCallStarted,
    pub status: TraceStatus,
    pub resolved_tool_name: Option<String>,
    pub permission_wait_ms: Option<i64>,
    pub ended_at: OffsetDateTime,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TraceSignal {
    ModelCallStarted(ModelCallStarted),
    ModelCallFinished(ModelCallFinished),
    ToolCallStarted(ToolCallStarted),
    ToolCallFinished(ToolCallFinished),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceFlushResult {
    pub flushed: bool,
    pub dropped_signals: u64,
    pub write_failures: u64,
}

#[async_trait]
pub trait TraceRecorder: Send + Sync {
    fn record(&self, signal: TraceSignal);

    async fn flush_turn(&self, turn_id: &TurnId) -> TraceFlushResult;
}

#[derive(Debug, Default)]
pub struct NoopTraceRecorder;

#[async_trait]
impl TraceRecorder for NoopTraceRecorder {
    fn record(&self, _signal: TraceSignal) {}

    async fn flush_turn(&self, _turn_id: &TurnId) -> TraceFlushResult {
        TraceFlushResult {
            flushed: true,
            ..TraceFlushResult::default()
        }
    }
}
