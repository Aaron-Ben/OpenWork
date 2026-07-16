use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use openwork_protocol::trace::{
    TraceRecorderPort, TraceRepository, TraceSignal, TraceSpan, TraceSpanStart, TraceSpanUpdate,
};
use serde_json::{Map, Value};
use tokio::{
    sync::{mpsc, oneshot},
    time::timeout,
};

const MAX_ERROR_CHARS: usize = 2_000;
const DEFAULT_QUEUE_CAPACITY: usize = 2_048;
const DEFAULT_FLUSH_TIMEOUT: Duration = Duration::from_millis(500);

enum TraceCommand {
    Signal(Box<TraceSignal>),
    Flush(oneshot::Sender<()>),
}

/// Non-blocking recorder backed by one ordered worker. The worker serializes
/// updates for a span and treats repository failures as diagnostic loss only.
#[derive(Clone)]
pub struct TraceRuntime {
    sender: mpsc::Sender<TraceCommand>,
    flush_timeout: Duration,
    stats: Arc<TraceRuntimeStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceRuntimeConfig {
    pub queue_capacity: usize,
    pub flush_timeout: Duration,
}

impl Default for TraceRuntimeConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            flush_timeout: DEFAULT_FLUSH_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraceRuntimeStatsSnapshot {
    pub dropped_signals: u64,
    pub persistence_errors: u64,
    pub orphan_updates: u64,
    pub flush_timeouts: u64,
    pub active_spans: usize,
}

#[derive(Default)]
struct TraceRuntimeStats {
    dropped_signals: AtomicU64,
    persistence_errors: AtomicU64,
    orphan_updates: AtomicU64,
    flush_timeouts: AtomicU64,
    active_spans: AtomicUsize,
}

impl TraceRuntime {
    pub fn new(repository: Arc<dyn TraceRepository>) -> Self {
        Self::with_config(repository, TraceRuntimeConfig::default())
    }

    pub fn with_config(repository: Arc<dyn TraceRepository>, config: TraceRuntimeConfig) -> Self {
        let capacity = config.queue_capacity.max(1);
        let (sender, receiver) = mpsc::channel(capacity);
        let stats = Arc::new(TraceRuntimeStats::default());
        tokio::spawn(run_worker(repository, receiver, Arc::clone(&stats)));
        Self {
            sender,
            flush_timeout: config.flush_timeout,
            stats,
        }
    }

    /// Waits until every signal sent before this call has been processed.
    /// Repository failures remain best-effort and are deliberately not returned.
    pub async fn flush(&self) {
        let (acknowledge, wait) = oneshot::channel();
        match timeout(
            self.flush_timeout,
            self.sender.send(TraceCommand::Flush(acknowledge)),
        )
        .await
        {
            Ok(Ok(())) => {
                if timeout(self.flush_timeout, wait).await.is_err() {
                    self.stats.flush_timeouts.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {
                self.stats.flush_timeouts.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Err(_)) => {}
        }
    }

    pub fn stats(&self) -> TraceRuntimeStatsSnapshot {
        TraceRuntimeStatsSnapshot {
            dropped_signals: self.stats.dropped_signals.load(Ordering::Relaxed),
            persistence_errors: self.stats.persistence_errors.load(Ordering::Relaxed),
            orphan_updates: self.stats.orphan_updates.load(Ordering::Relaxed),
            flush_timeouts: self.stats.flush_timeouts.load(Ordering::Relaxed),
            active_spans: self.stats.active_spans.load(Ordering::Relaxed),
        }
    }
}

impl TraceRecorderPort for TraceRuntime {
    fn record(&self, signal: TraceSignal) {
        if self
            .sender
            .try_send(TraceCommand::Signal(Box::new(signal)))
            .is_err()
        {
            self.stats.dropped_signals.fetch_add(1, Ordering::Relaxed);
        }
    }
}

async fn run_worker(
    repository: Arc<dyn TraceRepository>,
    mut receiver: mpsc::Receiver<TraceCommand>,
    stats: Arc<TraceRuntimeStats>,
) {
    let mut spans = HashMap::<String, TraceSpan>::new();
    while let Some(command) = receiver.recv().await {
        match command {
            TraceCommand::Signal(signal) => {
                if let Some(span) =
                    apply_signal(repository.as_ref(), &mut spans, *signal, stats.as_ref()).await
                {
                    let terminal = span.ended_at_unix_ms.is_some();
                    let span_id = span.span_id.clone();
                    if repository.upsert_span(span).await.is_err() {
                        stats.persistence_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    if terminal {
                        spans.remove(&span_id);
                    }
                } else {
                    stats.orphan_updates.fetch_add(1, Ordering::Relaxed);
                }
                stats.active_spans.store(spans.len(), Ordering::Relaxed);
            }
            TraceCommand::Flush(acknowledge) => {
                let _ = acknowledge.send(());
            }
        }
    }
}

async fn apply_signal(
    repository: &dyn TraceRepository,
    spans: &mut HashMap<String, TraceSpan>,
    signal: TraceSignal,
    stats: &TraceRuntimeStats,
) -> Option<TraceSpan> {
    match signal {
        TraceSignal::Start(start) => {
            let span = start_span(repository, spans, start).await;
            spans.insert(span.span_id.clone(), span.clone());
            Some(span)
        }
        TraceSignal::Update(update) => {
            if !spans.contains_key(&update.span_id) {
                match repository.load_span(&update.span_id).await {
                    Ok(Some(existing)) => {
                        spans.insert(update.span_id.clone(), existing);
                    }
                    Ok(None) => {}
                    Err(_) => {
                        stats.persistence_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let span = spans.get_mut(&update.span_id)?;
            apply_update(span, update);
            Some(span.clone())
        }
    }
}

async fn start_span(
    repository: &dyn TraceRepository,
    spans: &HashMap<String, TraceSpan>,
    start: TraceSpanStart,
) -> TraceSpan {
    let existing = match spans.get(&start.span_id) {
        Some(span) => Some(span.clone()),
        None => repository.load_span(&start.span_id).await.ok().flatten(),
    };
    let mut span = existing.unwrap_or_else(|| TraceSpan {
        trace_id: start.trace_id.clone(),
        span_id: start.span_id.clone(),
        parent_span_id: start.parent_span_id.clone(),
        span_kind: start.span_kind,
        span_name: start.span_name.clone(),
        status: start.status,
        session_id: start.session_id.clone(),
        turn_id: start.turn_id.clone(),
        step_id: start.step_id.clone(),
        tool_run_id: start.tool_run_id.clone(),
        started_at_unix_ms: start.started_at_unix_ms,
        ended_at_unix_ms: None,
        attributes: Value::Object(Map::new()),
        error_type: None,
        error_code: None,
        error_message: None,
    });
    span.started_at_unix_ms = span.started_at_unix_ms.min(start.started_at_unix_ms);
    if span.ended_at_unix_ms.is_none() {
        span.status = start.status;
    }
    merge_attributes(&mut span.attributes, start.attributes);
    span
}

fn apply_update(span: &mut TraceSpan, update: TraceSpanUpdate) {
    span.status = update.status;
    if update.ended {
        span.ended_at_unix_ms = Some(update.occurred_at_unix_ms.max(span.started_at_unix_ms));
    }
    merge_attributes(&mut span.attributes, update.attributes);
    span.error_type = update.error_type.or_else(|| span.error_type.take());
    span.error_code = update.error_code.or_else(|| span.error_code.take());
    span.error_message = update
        .error_message
        .map(|message| truncate(&message, MAX_ERROR_CHARS))
        .or_else(|| span.error_message.take());
}

fn merge_attributes(target: &mut Value, incoming: Value) {
    let Some(target) = target.as_object_mut() else {
        *target = Value::Object(Map::new());
        return merge_attributes(target, incoming);
    };
    if let Value::Object(incoming) = incoming {
        target.extend(incoming);
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let shortened = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}
