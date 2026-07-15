use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use openwork_app::TraceApplicationService;
use openwork_protocol::trace::{
    TraceRepository, TraceRepositoryError, TraceSpan, TraceSpanKind, TraceSpanStatus,
};
use serde_json::json;

#[derive(Default)]
struct MemoryTraceRepository {
    spans: Mutex<HashMap<String, TraceSpan>>,
}

#[async_trait]
impl TraceRepository for MemoryTraceRepository {
    async fn upsert_span(&self, span: TraceSpan) -> Result<(), TraceRepositoryError> {
        self.spans
            .lock()
            .unwrap()
            .insert(span.span_id.clone(), span);
        Ok(())
    }
    async fn load_span(&self, span_id: &str) -> Result<Option<TraceSpan>, TraceRepositoryError> {
        Ok(self.spans.lock().unwrap().get(span_id).cloned())
    }
    async fn load_turn(&self, turn_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(self
            .spans
            .lock()
            .unwrap()
            .values()
            .filter(|span| span.turn_id == turn_id)
            .cloned()
            .collect())
    }
    async fn load_session(&self, session_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(self
            .spans
            .lock()
            .unwrap()
            .values()
            .filter(|span| span.session_id == session_id)
            .cloned()
            .collect())
    }
    async fn load_recent_turns(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        let spans = self.spans.lock().unwrap();
        let mut roots = spans
            .values()
            .filter(|span| span.span_kind == TraceSpanKind::Turn)
            .cloned()
            .collect::<Vec<_>>();
        roots.sort_by(|left, right| right.started_at_unix_ms.cmp(&left.started_at_unix_ms));
        let turn_ids = roots
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|span| span.turn_id)
            .collect::<Vec<_>>();
        Ok(spans
            .values()
            .filter(|span| turn_ids.contains(&span.turn_id))
            .cloned()
            .collect())
    }
}

fn span(
    id: &str,
    kind: TraceSpanKind,
    parent: Option<&str>,
    attributes: serde_json::Value,
) -> TraceSpan {
    TraceSpan {
        trace_id: "turn-1".to_string(),
        span_id: id.to_string(),
        parent_span_id: parent.map(str::to_string),
        span_kind: kind,
        span_name: kind.as_str().to_string(),
        status: TraceSpanStatus::Succeeded,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        step_id: None,
        tool_run_id: None,
        started_at_unix_ms: 1_000,
        ended_at_unix_ms: Some(2_000),
        attributes,
        error_type: None,
        error_code: None,
        error_message: None,
    }
}

#[tokio::test]
async fn service_builds_one_lightweight_turn_summary() {
    let repository = Arc::new(MemoryTraceRepository::default());
    for item in [
        span(
            "turn-1",
            TraceSpanKind::Turn,
            None,
            json!({"model": "glm-5.1"}),
        ),
        span("step-1", TraceSpanKind::Step, Some("turn-1"), json!({})),
        span(
            "model-1",
            TraceSpanKind::ModelAttempt,
            Some("step-1"),
            json!({"inputTokens": 120, "outputTokens": 30}),
        ),
        span(
            "transport-1",
            TraceSpanKind::TransportAttempt,
            Some("model-1"),
            json!({}),
        ),
        span(
            "transport-2",
            TraceSpanKind::TransportAttempt,
            Some("model-1"),
            json!({}),
        ),
        span("tool-1", TraceSpanKind::ToolRun, Some("step-1"), json!({})),
    ] {
        repository.upsert_span(item).await.unwrap();
    }
    let service = TraceApplicationService::new(repository);

    let summaries = service.list_session("session-1").await.unwrap();

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].step_count, 1);
    assert_eq!(summaries[0].model_attempt_count, 1);
    assert_eq!(summaries[0].transport_attempt_count, 2);
    assert_eq!(summaries[0].retry_count, 1);
    assert_eq!(summaries[0].input_tokens, 120);
    assert_eq!(summaries[0].output_tokens, 30);
}

#[tokio::test]
async fn service_pages_recent_turns_without_splitting_one_trace() {
    let repository = Arc::new(MemoryTraceRepository::default());
    for (turn_id, started_at) in [("turn-old", 1_000), ("turn-new", 2_000)] {
        repository
            .upsert_span(TraceSpan {
                trace_id: turn_id.to_string(),
                span_id: turn_id.to_string(),
                parent_span_id: None,
                span_kind: TraceSpanKind::Turn,
                span_name: "turn.run".to_string(),
                status: TraceSpanStatus::Succeeded,
                session_id: "session-1".to_string(),
                turn_id: turn_id.to_string(),
                step_id: None,
                tool_run_id: None,
                started_at_unix_ms: started_at,
                ended_at_unix_ms: Some(started_at + 500),
                attributes: json!({"model": "glm-5.1"}),
                error_type: None,
                error_code: None,
                error_message: None,
            })
            .await
            .unwrap();
    }
    let service = TraceApplicationService::new(repository);

    let first = service.list_recent(1, 0).await.unwrap();
    assert_eq!(first.items[0].turn_id, "turn-new");
    assert_eq!(first.next_offset, Some(1));

    let second = service
        .list_recent(1, first.next_offset.unwrap())
        .await
        .unwrap();
    assert_eq!(second.items[0].turn_id, "turn-old");
    assert_eq!(second.next_offset, None);
}
