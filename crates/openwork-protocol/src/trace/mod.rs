//! Diagnostic trace contracts shared by Core, Providers, Persistence and hosts.
//!
//! Trace data is best-effort observability. It never replaces durable Recorded Events.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceSpanKind {
    Turn,
    Step,
    ModelAttempt,
    TransportAttempt,
    ToolRun,
    Approval,
    Recovery,
}

impl TraceSpanKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Step => "step",
            Self::ModelAttempt => "model_attempt",
            Self::TransportAttempt => "transport_attempt",
            Self::ToolRun => "tool_run",
            Self::Approval => "approval",
            Self::Recovery => "recovery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceSpanStatus {
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Denied,
    OutcomeUnknown,
}

impl TraceSpanStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Denied => "denied",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }
}

/// Query model persisted by an observability Repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub span_kind: TraceSpanKind,
    pub span_name: String,
    pub status: TraceSpanStatus,
    pub session_id: String,
    pub turn_id: String,
    pub step_id: Option<String>,
    pub tool_run_id: Option<String>,
    #[serde(rename = "startedAt")]
    pub started_at_unix_ms: i64,
    #[serde(rename = "endedAt")]
    pub ended_at_unix_ms: Option<i64>,
    pub attributes: Value,
    pub error_type: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

impl TraceSpan {
    pub fn duration_ms(&self) -> Option<i64> {
        self.ended_at_unix_ms
            .map(|ended| ended.saturating_sub(self.started_at_unix_ms).max(0))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceSpanStart {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub span_kind: TraceSpanKind,
    pub span_name: String,
    pub status: TraceSpanStatus,
    pub session_id: String,
    pub turn_id: String,
    pub step_id: Option<String>,
    pub tool_run_id: Option<String>,
    pub started_at_unix_ms: i64,
    pub attributes: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceSpanUpdate {
    pub span_id: String,
    pub status: TraceSpanStatus,
    pub occurred_at_unix_ms: i64,
    pub ended: bool,
    pub attributes: Value,
    pub error_type: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraceSignal {
    Start(TraceSpanStart),
    Update(TraceSpanUpdate),
}

/// Non-blocking signal boundary used by runtime code. Implementations must not
/// let trace persistence failures change Agent control flow.
pub trait TraceRecorderPort: Send + Sync {
    fn record(&self, signal: TraceSignal);
}

#[derive(Debug, Default)]
pub struct NoopTraceRecorder;

impl TraceRecorderPort for NoopTraceRecorder {
    fn record(&self, _signal: TraceSignal) {}
}

#[derive(Debug, Error)]
pub enum TraceRepositoryError {
    #[error("trace persistence error: {message}")]
    Persistence { message: String },
    #[error("invalid trace span: {message}")]
    InvalidSpan { message: String },
}

#[async_trait]
pub trait TraceRepository: Send + Sync {
    async fn upsert_span(&self, span: TraceSpan) -> Result<(), TraceRepositoryError>;

    async fn load_span(&self, span_id: &str) -> Result<Option<TraceSpan>, TraceRepositoryError>;

    async fn load_turn(&self, turn_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError>;

    async fn load_session(&self, session_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError>;

    /// Loads complete traces for a page of recent root turn spans. Pagination
    /// applies to turns rather than individual spans, so a trace is never split
    /// across two pages.
    async fn load_recent_turns(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError>;
}
