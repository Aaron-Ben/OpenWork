use std::{collections::HashMap, sync::Arc, time::SystemTime};

use openwork_protocol::trace::{TraceRepository, TraceSpan, TraceSpanKind, TraceSpanStatus};
use serde::Serialize;

use crate::ApplicationError;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTraceSummary {
    pub trace_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub status: TraceSpanStatus,
    pub model: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: i64,
    pub step_count: usize,
    pub model_attempt_count: usize,
    pub transport_attempt_count: usize,
    pub tool_run_count: usize,
    pub approval_count: usize,
    pub retry_count: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub error_count: usize,
    pub recovered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTrace {
    pub summary: TurnTraceSummary,
    pub spans: Vec<TraceSpanView>,
}

/// Host-facing view adds the derived duration without changing the protocol
/// persistence model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpanView {
    #[serde(flatten)]
    pub span: TraceSpan,
    pub duration_ms: Option<i64>,
}

pub struct TraceApplicationService {
    repository: Arc<dyn TraceRepository>,
}

impl TraceApplicationService {
    pub fn new(repository: Arc<dyn TraceRepository>) -> Self {
        Self { repository }
    }

    pub async fn load_turn(&self, turn_id: &str) -> Result<TurnTrace, ApplicationError> {
        let spans = self.repository.load_turn(turn_id).await?;
        let summary = summarize(turn_id, &spans);
        Ok(TurnTrace {
            summary,
            spans: spans
                .into_iter()
                .map(|span| TraceSpanView {
                    duration_ms: span.duration_ms(),
                    span,
                })
                .collect(),
        })
    }

    pub async fn list_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<TurnTraceSummary>, ApplicationError> {
        let mut by_turn = HashMap::<String, Vec<TraceSpan>>::new();
        for span in self.repository.load_session(session_id).await? {
            by_turn.entry(span.turn_id.clone()).or_default().push(span);
        }
        let mut summaries = by_turn
            .into_iter()
            .map(|(turn_id, spans)| summarize(&turn_id, &spans))
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| left.turn_id.cmp(&right.turn_id))
        });
        Ok(summaries)
    }
}

fn summarize(turn_id: &str, spans: &[TraceSpan]) -> TurnTraceSummary {
    let root = spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn);
    let started_at = root
        .map(|span| span.started_at_unix_ms)
        .or_else(|| spans.iter().map(|span| span.started_at_unix_ms).min())
        .unwrap_or(0);
    let ended_at = root
        .and_then(|span| span.ended_at_unix_ms)
        .or_else(|| spans.iter().filter_map(|span| span.ended_at_unix_ms).max());
    let duration_end = ended_at.unwrap_or_else(now_unix_ms);
    let model_attempt_count = count_kind(spans, TraceSpanKind::ModelAttempt);
    let transport_attempt_count = count_kind(spans, TraceSpanKind::TransportAttempt);
    let (input_tokens, output_tokens) = spans
        .iter()
        .filter(|span| span.span_kind == TraceSpanKind::ModelAttempt)
        .fold((0_u64, 0_u64), |(inputs, outputs), span| {
            (
                inputs.saturating_add(attribute_u64(span, "inputTokens")),
                outputs.saturating_add(attribute_u64(span, "outputTokens")),
            )
        });

    TurnTraceSummary {
        trace_id: root
            .map(|span| span.trace_id.clone())
            .unwrap_or_else(|| turn_id.to_string()),
        turn_id: turn_id.to_string(),
        session_id: root
            .map(|span| span.session_id.clone())
            .or_else(|| spans.first().map(|span| span.session_id.clone()))
            .unwrap_or_default(),
        status: root
            .map(|span| span.status)
            .unwrap_or(TraceSpanStatus::Running),
        model: root
            .and_then(|span| span.attributes.get("model"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        started_at,
        ended_at,
        duration_ms: duration_end.saturating_sub(started_at).max(0),
        step_count: count_kind(spans, TraceSpanKind::Step),
        model_attempt_count,
        transport_attempt_count,
        tool_run_count: count_kind(spans, TraceSpanKind::ToolRun),
        approval_count: count_kind(spans, TraceSpanKind::Approval),
        retry_count: transport_attempt_count.saturating_sub(model_attempt_count),
        input_tokens,
        output_tokens,
        error_count: spans
            .iter()
            .filter(|span| span.status == TraceSpanStatus::Failed)
            .count(),
        recovered: spans
            .iter()
            .any(|span| span.span_kind == TraceSpanKind::Recovery),
    }
}

fn count_kind(spans: &[TraceSpan], kind: TraceSpanKind) -> usize {
    spans.iter().filter(|span| span.span_kind == kind).count()
}

fn attribute_u64(span: &TraceSpan, key: &str) -> u64 {
    span.attributes
        .get(key)
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}
