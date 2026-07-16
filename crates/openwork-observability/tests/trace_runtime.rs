use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use openwork_observability::{TraceRuntime, TraceRuntimeConfig};
use openwork_protocol::trace::{
    TraceRecorderPort, TraceRepository, TraceRepositoryError, TraceSignal, TraceSpan,
    TraceSpanKind, TraceSpanStart, TraceSpanStatus, TraceSpanUpdate,
};
use serde_json::json;
use tokio::sync::Notify;

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
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(self.spans.lock().unwrap().values().cloned().collect())
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
    assert_eq!(runtime.stats().active_spans, 0);
}

struct BlockingTraceRepository {
    spans: Mutex<HashMap<String, TraceSpan>>,
    block_writes: AtomicBool,
    write_started: Notify,
    release_write: Notify,
}

impl BlockingTraceRepository {
    fn new(block_writes: bool) -> Self {
        Self {
            spans: Mutex::new(HashMap::new()),
            block_writes: AtomicBool::new(block_writes),
            write_started: Notify::new(),
            release_write: Notify::new(),
        }
    }
}

#[async_trait]
impl TraceRepository for BlockingTraceRepository {
    async fn upsert_span(&self, span: TraceSpan) -> Result<(), TraceRepositoryError> {
        if self.block_writes.load(Ordering::SeqCst) {
            self.write_started.notify_one();
            self.release_write.notified().await;
        }
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
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(self.spans.lock().unwrap().values().cloned().collect())
    }
}

#[tokio::test]
async fn bounded_runtime_drops_excess_signals_and_flush_never_waits_forever() {
    let repository = Arc::new(BlockingTraceRepository::new(true));
    let runtime = TraceRuntime::with_config(
        repository.clone(),
        TraceRuntimeConfig {
            queue_capacity: 1,
            flush_timeout: Duration::from_millis(25),
        },
    );

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
        attributes: json!({}),
    }));
    tokio::time::timeout(Duration::from_secs(1), repository.write_started.notified())
        .await
        .expect("worker should reach repository");

    for occurred_at_unix_ms in 1_001..1_100 {
        runtime.record(TraceSignal::Update(TraceSpanUpdate {
            span_id: "step-1".to_string(),
            status: TraceSpanStatus::Running,
            occurred_at_unix_ms,
            ended: false,
            attributes: json!({"heartbeat": occurred_at_unix_ms}),
            error_type: None,
            error_code: None,
            error_message: None,
        }));
    }

    let started = Instant::now();
    runtime.flush().await;
    assert!(started.elapsed() < Duration::from_millis(250));
    let stats = runtime.stats();
    assert!(stats.dropped_signals > 0);
    assert_eq!(stats.flush_timeouts, 1);

    repository.block_writes.store(false, Ordering::SeqCst);
    repository.release_write.notify_waiters();
}

struct FailingTraceRepository;

#[async_trait]
impl TraceRepository for FailingTraceRepository {
    async fn upsert_span(&self, _span: TraceSpan) -> Result<(), TraceRepositoryError> {
        Err(TraceRepositoryError::Persistence {
            message: "offline".to_string(),
        })
    }

    async fn load_span(&self, _span_id: &str) -> Result<Option<TraceSpan>, TraceRepositoryError> {
        Ok(None)
    }

    async fn load_turn(&self, _turn_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(Vec::new())
    }

    async fn load_session(
        &self,
        _session_id: &str,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(Vec::new())
    }

    async fn load_recent_turns(
        &self,
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn runtime_counts_persistence_failures_and_orphan_updates() {
    let runtime = TraceRuntime::new(Arc::new(FailingTraceRepository));
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
        attributes: json!({}),
    }));
    runtime.record(TraceSignal::Update(TraceSpanUpdate {
        span_id: "missing-span".to_string(),
        status: TraceSpanStatus::Failed,
        occurred_at_unix_ms: 2_000,
        ended: true,
        attributes: json!({}),
        error_type: Some("missing".to_string()),
        error_code: None,
        error_message: None,
    }));
    runtime.flush().await;

    let stats = runtime.stats();
    assert_eq!(stats.persistence_errors, 1);
    assert_eq!(stats.orphan_updates, 1);
}
