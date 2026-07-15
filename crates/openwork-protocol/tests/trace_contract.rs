use openwork_protocol::trace::{TraceSpan, TraceSpanKind, TraceSpanStatus};
use serde_json::json;

#[test]
fn trace_span_contract_keeps_hierarchy_and_diagnostic_fields_explicit() {
    let span = TraceSpan {
        trace_id: "turn-1".to_string(),
        span_id: "model-attempt-1".to_string(),
        parent_span_id: Some("step-1".to_string()),
        span_kind: TraceSpanKind::ModelAttempt,
        span_name: "model.attempt".to_string(),
        status: TraceSpanStatus::Succeeded,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        step_id: Some("step-1".to_string()),
        tool_run_id: None,
        started_at_unix_ms: 1_000,
        ended_at_unix_ms: Some(2_500),
        attributes: json!({
            "model": "glm-5.1",
            "inputTokens": 120,
            "outputTokens": 40
        }),
        error_type: None,
        error_code: None,
        error_message: None,
    };

    let value = serde_json::to_value(&span).expect("trace span serializes");
    assert_eq!(value["traceId"], "turn-1");
    assert_eq!(value["parentSpanId"], "step-1");
    assert_eq!(value["spanKind"], "model_attempt");
    assert_eq!(value["status"], "succeeded");
    assert_eq!(span.duration_ms(), Some(1_500));
}

#[test]
fn trace_span_kinds_cover_the_v1_execution_tree() {
    assert_eq!(
        [
            TraceSpanKind::Turn,
            TraceSpanKind::Step,
            TraceSpanKind::ModelAttempt,
            TraceSpanKind::TransportAttempt,
            TraceSpanKind::ToolRun,
            TraceSpanKind::Approval,
            TraceSpanKind::Recovery,
        ]
        .len(),
        7
    );
}
