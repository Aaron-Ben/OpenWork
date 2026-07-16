//! Diagnostic trace contracts shared by Core, Providers, Persistence and hosts.
//!
//! Trace data is best-effort observability. It never replaces durable Recorded Events.

use std::collections::HashMap;

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

/// Root-Turn filters applied by a Trace repository before pagination. Context
/// search ids are supplied by the Application layer after joining Session and
/// Message projections; they participate in the same OR group as `query`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceRootFilter {
    pub session_ids: Option<Vec<String>>,
    pub query: Option<String>,
    pub search_session_ids: Vec<String>,
    pub search_turn_ids: Vec<String>,
    pub model: Option<String>,
    pub status: Option<TraceSpanStatus>,
    pub started_after_unix_ms: Option<i64>,
    pub started_before_unix_ms: Option<i64>,
    pub has_error: Option<bool>,
    pub has_retry: Option<bool>,
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

    /// Production repositories should override this method so filtering is
    /// performed by the data store. The fallback keeps in-memory adapters and
    /// tests source-compatible while preserving filter-before-page semantics.
    async fn load_recent_turns_filtered(
        &self,
        filter: &TraceRootFilter,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        let spans = self.load_recent_turns(u32::MAX, 0).await?;
        Ok(filter_recent_turns(spans, filter, limit, offset))
    }

    /// Deletes whole terminal Turns whose latest Span ended before the cutoff.
    /// The limit is measured in Turns; the return value is deleted Span rows.
    /// Adapters without retention support may keep the default no-op.
    async fn prune_ended_turns_before(
        &self,
        _ended_before_unix_ms: i64,
        _turn_limit: u32,
    ) -> Result<u64, TraceRepositoryError> {
        Ok(0)
    }
}

fn filter_recent_turns(
    spans: Vec<TraceSpan>,
    filter: &TraceRootFilter,
    limit: u32,
    offset: u32,
) -> Vec<TraceSpan> {
    if limit == 0 {
        return Vec::new();
    }
    let mut by_turn = HashMap::<String, Vec<TraceSpan>>::new();
    for span in spans {
        by_turn.entry(span.turn_id.clone()).or_default().push(span);
    }
    let mut traces = by_turn
        .into_values()
        .filter(|trace| trace_matches(trace, filter))
        .collect::<Vec<_>>();
    traces.sort_by(|left, right| {
        root_started_at(right)
            .cmp(&root_started_at(left))
            .then_with(|| root_turn_id(left).cmp(root_turn_id(right)))
    });
    traces
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .flatten()
        .collect()
}

fn trace_matches(spans: &[TraceSpan], filter: &TraceRootFilter) -> bool {
    let Some(root) = spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn)
    else {
        return false;
    };
    if let Some(session_ids) = &filter.session_ids
        && !session_ids.iter().any(|id| id == &root.session_id)
    {
        return false;
    }
    if filter.status.is_some_and(|status| root.status != status) {
        return false;
    }
    if filter
        .started_after_unix_ms
        .is_some_and(|after| root.started_at_unix_ms < after)
        || filter
            .started_before_unix_ms
            .is_some_and(|before| root.started_at_unix_ms > before)
    {
        return false;
    }
    if let Some(model) = normalized(filter.model.as_deref()) {
        let root_model = root
            .attributes
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        if root_model.as_deref() != Some(model.as_str()) {
            return false;
        }
    }
    let has_error = spans
        .iter()
        .any(|span| span.status == TraceSpanStatus::Failed);
    if filter
        .has_error
        .is_some_and(|expected| has_error != expected)
    {
        return false;
    }
    let model_attempts = spans
        .iter()
        .filter(|span| span.span_kind == TraceSpanKind::ModelAttempt)
        .count();
    let transport_attempts = spans
        .iter()
        .filter(|span| span.span_kind == TraceSpanKind::TransportAttempt)
        .count();
    let has_retry = transport_attempts > model_attempts;
    if filter
        .has_retry
        .is_some_and(|expected| has_retry != expected)
    {
        return false;
    }
    if let Some(query) = normalized(filter.query.as_deref()) {
        let root_match = [
            root.turn_id.as_str(),
            root.session_id.as_str(),
            root.trace_id.as_str(),
            root.attributes
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ]
        .iter()
        .any(|value| value.to_lowercase().contains(&query));
        let context_match = filter
            .search_session_ids
            .iter()
            .any(|id| id == &root.session_id)
            || filter.search_turn_ids.iter().any(|id| id == &root.turn_id);
        if !root_match && !context_match {
            return false;
        }
    }
    true
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

fn root_started_at(spans: &[TraceSpan]) -> i64 {
    spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn)
        .map(|span| span.started_at_unix_ms)
        .unwrap_or(0)
}

fn root_turn_id(spans: &[TraceSpan]) -> &str {
    spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn)
        .map(|span| span.turn_id.as_str())
        .unwrap_or_default()
}
