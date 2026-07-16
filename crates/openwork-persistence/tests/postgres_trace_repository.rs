mod common;

use common::{connect_test_pool, test_config};
use openwork_persistence::{PostgresPersistence, PostgresTraceRepository};
use openwork_protocol::trace::{
    TraceRepository, TraceRootFilter, TraceSpan, TraceSpanKind, TraceSpanStatus,
};
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

#[tokio::test]
async fn recent_turn_query_pages_root_turns_and_keeps_their_children() {
    let Some(config) = test_config(2) else {
        return;
    };
    PostgresPersistence::migrate_database(config.clone())
        .await
        .unwrap();
    let pool = connect_test_pool(&config).await.unwrap();
    let repository = PostgresTraceRepository::new(pool.clone());
    let suffix = Uuid::new_v4();
    let session_id = format!("trace-page-session-{suffix}");

    // Keep this global-recency fixture ahead of developer data that may share
    // the local integration database. The test deletes both rows afterwards.
    for (index, started_at) in [(1, 4_000_000_000_000), (2, 4_000_000_010_000)] {
        let turn_id = format!("trace-page-turn-{suffix}-{index}");
        repository
            .upsert_span(TraceSpan {
                trace_id: turn_id.clone(),
                span_id: turn_id.clone(),
                parent_span_id: None,
                span_kind: TraceSpanKind::Turn,
                span_name: "turn.run".to_string(),
                status: if index == 1 {
                    TraceSpanStatus::Failed
                } else {
                    TraceSpanStatus::Succeeded
                },
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                step_id: None,
                tool_run_id: None,
                started_at_unix_ms: started_at,
                ended_at_unix_ms: Some(started_at + 1_000),
                attributes: json!({"model": "glm-5.1"}),
                error_type: None,
                error_code: None,
                error_message: None,
            })
            .await
            .unwrap();
        repository
            .upsert_span(TraceSpan {
                trace_id: turn_id.clone(),
                span_id: format!("{turn_id}:step"),
                parent_span_id: Some(turn_id.clone()),
                span_kind: TraceSpanKind::Step,
                span_name: "step.run".to_string(),
                status: TraceSpanStatus::Succeeded,
                session_id: session_id.clone(),
                turn_id,
                step_id: Some(format!("step-{index}")),
                tool_run_id: None,
                started_at_unix_ms: started_at + 100,
                ended_at_unix_ms: Some(started_at + 900),
                attributes: json!({"stepIndex": index}),
                error_type: None,
                error_code: None,
                error_message: None,
            })
            .await
            .unwrap();
    }

    let page = repository.load_recent_turns(1, 0).await.unwrap();
    assert_eq!(page.len(), 2);
    assert!(page.iter().all(|span| span.turn_id.ends_with("-2")));

    let filtered = repository
        .load_recent_turns_filtered(
            &TraceRootFilter {
                status: Some(TraceSpanStatus::Failed),
                session_ids: Some(vec![session_id.clone()]),
                ..TraceRootFilter::default()
            },
            1,
            0,
        )
        .await
        .unwrap();
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|span| span.turn_id.ends_with("-1")));

    sqlx::query("DELETE FROM trace_spans WHERE session_id = $1")
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn retention_prunes_only_complete_expired_turns() {
    let Some(config) = test_config(2) else {
        return;
    };
    PostgresPersistence::migrate_database(config.clone())
        .await
        .unwrap();
    let pool = connect_test_pool(&config).await.unwrap();
    let repository = PostgresTraceRepository::new(pool.clone());
    let suffix = Uuid::new_v4();
    let session_id = format!("trace-retention-session-{suffix}");

    for (label, ended_at) in [("expired", Some(1_000)), ("running", None)] {
        let turn_id = format!("trace-retention-{label}-{suffix}");
        repository
            .upsert_span(TraceSpan {
                trace_id: turn_id.clone(),
                span_id: turn_id.clone(),
                parent_span_id: None,
                span_kind: TraceSpanKind::Turn,
                span_name: "turn.run".to_string(),
                status: if ended_at.is_some() {
                    TraceSpanStatus::Succeeded
                } else {
                    TraceSpanStatus::Running
                },
                session_id: session_id.clone(),
                turn_id,
                step_id: None,
                tool_run_id: None,
                started_at_unix_ms: 500,
                ended_at_unix_ms: ended_at,
                attributes: json!({"traceSchemaVersion": "1.1"}),
                error_type: None,
                error_code: None,
                error_message: None,
            })
            .await
            .unwrap();
    }

    let deleted = repository
        .prune_ended_turns_before(2_000, 10)
        .await
        .unwrap();

    assert_eq!(deleted, 1);
    assert!(
        repository
            .load_turn(&format!("trace-retention-expired-{suffix}"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repository
            .load_turn(&format!("trace-retention-running-{suffix}"))
            .await
            .unwrap()
            .len(),
        1
    );

    sqlx::query("DELETE FROM trace_spans WHERE session_id = $1")
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();
}
