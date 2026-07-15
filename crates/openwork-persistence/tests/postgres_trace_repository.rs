mod common;

use common::{connect_test_pool, test_config};
use openwork_persistence::{PostgresPersistence, PostgresTraceRepository};
use openwork_protocol::trace::{TraceRepository, TraceSpan, TraceSpanKind, TraceSpanStatus};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn trace_span_upsert_preserves_start_and_merges_diagnostic_attributes() {
    let Some(config) = test_config(2) else {
        return;
    };
    PostgresPersistence::migrate_database(config.clone())
        .await
        .unwrap();
    let pool = connect_test_pool(&config).await.unwrap();
    let repository = PostgresTraceRepository::new(pool.clone());
    let suffix = Uuid::new_v4();
    let turn_id = format!("trace-test-turn-{suffix}");
    let session_id = format!("trace-test-session-{suffix}");

    let mut span = TraceSpan {
        trace_id: turn_id.clone(),
        span_id: turn_id.clone(),
        parent_span_id: None,
        span_kind: TraceSpanKind::Turn,
        span_name: "turn.run".to_string(),
        status: TraceSpanStatus::Running,
        session_id: session_id.clone(),
        turn_id: turn_id.clone(),
        step_id: None,
        tool_run_id: None,
        started_at_unix_ms: 1_720_000_000_123,
        ended_at_unix_ms: None,
        attributes: json!({"model": "glm-5.1"}),
        error_type: None,
        error_code: None,
        error_message: None,
    };
    repository.upsert_span(span.clone()).await.unwrap();

    span.status = TraceSpanStatus::Succeeded;
    span.started_at_unix_ms += 1_000;
    span.ended_at_unix_ms = Some(1_720_000_018_723);
    span.attributes = json!({"outcome": "completed"});
    repository.upsert_span(span).await.unwrap();

    let stored = repository.load_turn(&turn_id).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].started_at_unix_ms, 1_720_000_000_123);
    assert_eq!(stored[0].duration_ms(), Some(18_600));
    assert_eq!(stored[0].attributes["model"], "glm-5.1");
    assert_eq!(stored[0].attributes["outcome"], "completed");

    sqlx::query("DELETE FROM trace_spans WHERE session_id = $1")
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();
}
