use openwork_persistence::TRACE_SPAN_MIGRATIONS;

#[test]
fn trace_span_migration_is_a_diagnostic_projection_not_a_second_transcript() {
    assert_eq!(TRACE_SPAN_MIGRATIONS.len(), 1);
    let sql = TRACE_SPAN_MIGRATIONS[0].statements.join("\n");

    assert!(sql.contains("CREATE TABLE IF NOT EXISTS trace_spans"));
    assert!(sql.contains("span_id TEXT PRIMARY KEY"));
    assert!(sql.contains("trace_id TEXT NOT NULL"));
    assert!(sql.contains("parent_span_id TEXT"));
    assert!(sql.contains("span_kind TEXT NOT NULL"));
    assert!(sql.contains("status TEXT NOT NULL"));
    assert!(sql.contains("session_id TEXT NOT NULL"));
    assert!(sql.contains("turn_id TEXT NOT NULL"));
    assert!(sql.contains("step_id TEXT"));
    assert!(sql.contains("tool_run_id TEXT"));
    assert!(sql.contains("attributes_json JSONB NOT NULL DEFAULT '{}'::JSONB"));
    assert!(sql.contains("TIMESTAMP WITHOUT TIME ZONE"));
    assert!(sql.contains("CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'"));
    assert!(sql.contains("jsonb_typeof(attributes_json) = 'object'"));
    assert!(sql.contains("idx_trace_spans_session_started"));
    assert!(sql.contains("idx_trace_spans_turn_started"));

    for forbidden in ["api_key", "authorization", "system_prompt", "request_body", "response_body"] {
        assert!(!sql.to_ascii_lowercase().contains(forbidden));
    }
    assert!(!sql.contains("TIMESTAMPTZ"));
    assert!(!sql.contains("is_deleted"));
}
