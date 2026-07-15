use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use openwork_observability::TraceRuntime;
use openwork_protocol::trace::{
    TraceRecorderPort, TraceRepository, TraceRepositoryError, TraceSignal, TraceSpan,
    TraceSpanKind, TraceSpanStart, TraceSpanStatus, TraceSpanUpdate,
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
}

#[tokio::test]
async fn runtime_merges_start_and_finish_without_blocking_the_caller() {
    let repository = Arc::new(MemoryTraceRepository::default());
    let runtime = TraceRuntime::new(repository.clone());

    runtime.record(TraceSignal::Start(TraceSpanStart {
        trace_id: "turn-1".to_string(),
        span_id: "step-1".to_string(),
        parent_span_id: Some("turn-1".to_string()),
        span_kind: TraceSpanKind::Step,
        span_name: "step.run".to_string(),
        status: TraceSpanStatus::Running,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        step_id: Some("step-1".to_string()),
        tool_run_id: None,
        started_at_unix_ms: 1_000,
        attributes: json!({"stepIndex": 1}),
    }));
    runtime.record(TraceSignal::Update(TraceSpanUpdate {
        span_id: "step-1".to_string(),
        status: TraceSpanStatus::Succeeded,
        occurred_at_unix_ms: 2_500,
        ended: true,
        attributes: json!({"finishReason": "stop"}),
        error_type: None,
        error_code: None,
        error_message: None,
    }));
    runtime.flush().await;

    let span = repository.load_span("step-1").await.unwrap().unwrap();
    assert_eq!(span.status, TraceSpanStatus::Succeeded);
    assert_eq!(span.duration_ms(), Some(1_500));
    assert_eq!(span.attributes["stepIndex"], 1);
    assert_eq!(span.attributes["finishReason"], "stop");
}
