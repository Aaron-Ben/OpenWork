use openwork_protocol::trace::{TraceRepositoryError, TraceSpan, TraceSpanKind, TraceSpanStatus};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub(super) struct TraceSpanRecord {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub span_kind: String,
    pub span_name: String,
    pub status: String,
    pub session_id: String,
    pub turn_id: String,
    pub step_id: Option<String>,
    pub tool_run_id: Option<String>,
    pub started_at_unix_ms: i64,
    pub ended_at_unix_ms: Option<i64>,
    pub attributes_json: serde_json::Value,
    pub error_type: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

impl TryFrom<TraceSpanRecord> for TraceSpan {
    type Error = TraceRepositoryError;

    fn try_from(record: TraceSpanRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            trace_id: record.trace_id,
            span_id: record.span_id,
            parent_span_id: record.parent_span_id,
            span_kind: parse_kind(&record.span_kind)?,
            span_name: record.span_name,
            status: parse_status(&record.status)?,
            session_id: record.session_id,
            turn_id: record.turn_id,
            step_id: record.step_id,
            tool_run_id: record.tool_run_id,
            started_at_unix_ms: record.started_at_unix_ms,
            ended_at_unix_ms: record.ended_at_unix_ms,
            attributes: record.attributes_json,
            error_type: record.error_type,
            error_code: record.error_code,
            error_message: record.error_message,
        })
    }
}

fn parse_kind(value: &str) -> Result<TraceSpanKind, TraceRepositoryError> {
    match value {
        "turn" => Ok(TraceSpanKind::Turn),
        "step" => Ok(TraceSpanKind::Step),
        "model_attempt" => Ok(TraceSpanKind::ModelAttempt),
        "transport_attempt" => Ok(TraceSpanKind::TransportAttempt),
        "tool_run" => Ok(TraceSpanKind::ToolRun),
        "approval" => Ok(TraceSpanKind::Approval),
        "recovery" => Ok(TraceSpanKind::Recovery),
        other => Err(TraceRepositoryError::Persistence {
            message: format!("unknown trace span kind: {other}"),
        }),
    }
}

fn parse_status(value: &str) -> Result<TraceSpanStatus, TraceRepositoryError> {
    match value {
        "running" => Ok(TraceSpanStatus::Running),
        "waiting" => Ok(TraceSpanStatus::Waiting),
        "succeeded" => Ok(TraceSpanStatus::Succeeded),
        "failed" => Ok(TraceSpanStatus::Failed),
        "cancelled" => Ok(TraceSpanStatus::Cancelled),
        "denied" => Ok(TraceSpanStatus::Denied),
        "outcome_unknown" => Ok(TraceSpanStatus::OutcomeUnknown),
        other => Err(TraceRepositoryError::Persistence {
            message: format!("unknown trace span status: {other}"),
        }),
    }
}
