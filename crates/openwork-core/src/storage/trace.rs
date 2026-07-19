use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};
use tokio::sync::{mpsc, oneshot};

use crate::session::{
    ModelCallFinished, ModelCallStarted, ToolCallFinished, ToolCallStarted, TraceFlushResult,
    TraceRecorder, TraceSignal, TurnId,
};

const DEFAULT_TRACE_QUEUE_CAPACITY: usize = 1_024;
const MAX_BATCH_SIZE: usize = 64;
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

enum TraceCommand {
    Signal(Box<TraceSignal>),
    Flush(oneshot::Sender<()>),
}

#[derive(Debug, Default)]
struct TraceMetrics {
    dropped_signals: AtomicU64,
    write_failures: AtomicU64,
}

#[derive(Clone)]
pub struct PostgresTraceRecorder {
    sender: mpsc::Sender<TraceCommand>,
    metrics: Arc<TraceMetrics>,
}

impl PostgresTraceRecorder {
    pub fn spawn(pool: PgPool) -> Self {
        Self::spawn_with_capacity(pool, DEFAULT_TRACE_QUEUE_CAPACITY)
    }

    pub fn spawn_with_capacity(pool: PgPool, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        let metrics = Arc::new(TraceMetrics::default());
        tokio::spawn(run_writer(pool, receiver, Arc::clone(&metrics)));
        Self { sender, metrics }
    }

    pub fn metrics(&self) -> TraceFlushResult {
        TraceFlushResult {
            flushed: false,
            dropped_signals: self.metrics.dropped_signals.load(Ordering::Relaxed),
            write_failures: self.metrics.write_failures.load(Ordering::Relaxed),
        }
    }
}

#[async_trait]
impl TraceRecorder for PostgresTraceRecorder {
    fn record(&self, signal: TraceSignal) {
        if self
            .sender
            .try_send(TraceCommand::Signal(Box::new(signal)))
            .is_err()
        {
            self.metrics.dropped_signals.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn flush_turn(&self, _turn_id: &TurnId) -> TraceFlushResult {
        let (respond_to, response) = oneshot::channel();
        let flushed = match tokio::time::timeout(
            FLUSH_TIMEOUT,
            self.sender.send(TraceCommand::Flush(respond_to)),
        )
        .await
        {
            Ok(Ok(())) => tokio::time::timeout(FLUSH_TIMEOUT, response)
                .await
                .is_ok_and(|result| result.is_ok()),
            Ok(Err(_)) | Err(_) => false,
        };
        TraceFlushResult {
            flushed,
            dropped_signals: self.metrics.dropped_signals.load(Ordering::Relaxed),
            write_failures: self.metrics.write_failures.load(Ordering::Relaxed),
        }
    }
}

async fn run_writer(
    pool: PgPool,
    mut receiver: mpsc::Receiver<TraceCommand>,
    metrics: Arc<TraceMetrics>,
) {
    while let Some(first) = receiver.recv().await {
        let mut signals = Vec::with_capacity(MAX_BATCH_SIZE);
        let mut flushes = Vec::new();
        collect_command(first, &mut signals, &mut flushes);
        while signals.len() < MAX_BATCH_SIZE {
            let Ok(command) = receiver.try_recv() else {
                break;
            };
            collect_command(command, &mut signals, &mut flushes);
        }

        if !signals.is_empty() && write_batch(&pool, &signals).await.is_err() {
            metrics
                .write_failures
                .fetch_add(signals.len() as u64, Ordering::Relaxed);
        }
        for flush in flushes {
            let _ = flush.send(());
        }
    }
}

fn collect_command(
    command: TraceCommand,
    signals: &mut Vec<TraceSignal>,
    flushes: &mut Vec<oneshot::Sender<()>>,
) {
    match command {
        TraceCommand::Signal(signal) => signals.push(*signal),
        TraceCommand::Flush(respond_to) => flushes.push(respond_to),
    }
}

async fn write_batch(pool: &PgPool, signals: &[TraceSignal]) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    for signal in signals {
        write_signal(&mut transaction, signal).await?;
    }
    transaction.commit().await
}

async fn write_signal(
    transaction: &mut Transaction<'_, Postgres>,
    signal: &TraceSignal,
) -> Result<(), sqlx::Error> {
    match signal {
        TraceSignal::ModelCallStarted(started) => write_model_started(transaction, started).await,
        TraceSignal::ModelCallFinished(finished) => {
            write_model_finished(transaction, finished).await
        }
        TraceSignal::ToolCallStarted(started) => write_tool_started(transaction, started).await,
        TraceSignal::ToolCallFinished(finished) => write_tool_finished(transaction, finished).await,
    }
}

async fn write_model_started(
    transaction: &mut Transaction<'_, Postgres>,
    started: &ModelCallStarted,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trace_spans (
             id, turn_id, sequence, kind, name, status, model_id,
             resolved_model_name, started_at, attributes
         ) VALUES ($1, $2, $3, 'model_call', 'model.call', 'running', $4, $5, $6, $7)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&started.span_id)
    .bind(started.turn_id.as_str())
    .bind(started.sequence)
    .bind(&started.model_id)
    .bind(&started.resolved_model_name)
    .bind(utc_naive(started.started_at))
    .bind(Json(&started.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_model_finished(
    transaction: &mut Transaction<'_, Postgres>,
    finished: &ModelCallFinished,
) -> Result<(), sqlx::Error> {
    let usage = finished.usage;
    sqlx::query(
        "INSERT INTO trace_spans (
             id, turn_id, sequence, kind, name, status, model_id,
             resolved_model_name, provider_request_id, attempt_count,
             input_tokens, output_tokens, cached_input_tokens, reasoning_tokens,
             started_at, ended_at, error_code, error_message, attributes
         ) VALUES (
             $1, $2, $3, 'model_call', 'model.call', $4, $5,
             $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17
         )
         ON CONFLICT (id) DO UPDATE SET
             status = EXCLUDED.status,
             provider_request_id = EXCLUDED.provider_request_id,
             attempt_count = EXCLUDED.attempt_count,
             input_tokens = EXCLUDED.input_tokens,
             output_tokens = EXCLUDED.output_tokens,
             cached_input_tokens = EXCLUDED.cached_input_tokens,
             reasoning_tokens = EXCLUDED.reasoning_tokens,
             ended_at = EXCLUDED.ended_at,
             error_code = EXCLUDED.error_code,
             error_message = EXCLUDED.error_message,
             attributes = EXCLUDED.attributes",
    )
    .bind(&finished.started.span_id)
    .bind(finished.started.turn_id.as_str())
    .bind(finished.started.sequence)
    .bind(finished.status.as_str())
    .bind(&finished.started.model_id)
    .bind(&finished.started.resolved_model_name)
    .bind(&finished.provider_request_id)
    .bind(finished.attempt_count)
    .bind(usage.and_then(|value| token(value.input_tokens)))
    .bind(usage.and_then(|value| token(value.output_tokens)))
    .bind(usage.and_then(|value| token(value.cached_input_tokens)))
    .bind(usage.and_then(|value| token(value.reasoning_tokens)))
    .bind(utc_naive(finished.started.started_at))
    .bind(utc_naive(finished.ended_at))
    .bind(&finished.error_code)
    .bind(&finished.error_message)
    .bind(Json(&finished.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_tool_started(
    transaction: &mut Transaction<'_, Postgres>,
    started: &ToolCallStarted,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trace_spans (
             id, turn_id, parent_span_id, sequence, kind, name, status,
             provider_call_id, requested_tool_name, started_at, attributes
         ) VALUES ($1, $2, $3, $4, 'tool_call', 'tool.call', 'running', $5, $6, $7, $8)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&started.span_id)
    .bind(started.turn_id.as_str())
    .bind(&started.parent_span_id)
    .bind(started.sequence)
    .bind(&started.provider_call_id)
    .bind(&started.requested_tool_name)
    .bind(utc_naive(started.started_at))
    .bind(Json(&started.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_tool_finished(
    transaction: &mut Transaction<'_, Postgres>,
    finished: &ToolCallFinished,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trace_spans (
             id, turn_id, parent_span_id, sequence, kind, name, status,
             provider_call_id, requested_tool_name, resolved_tool_name,
             permission_wait_ms, started_at, ended_at, error_code, error_message, attributes
         ) VALUES (
             $1, $2, $3, $4, 'tool_call', 'tool.call', $5,
             $6, $7, $8, $9, $10, $11, $12, $13, $14
         )
         ON CONFLICT (id) DO UPDATE SET
             status = EXCLUDED.status,
             resolved_tool_name = EXCLUDED.resolved_tool_name,
             permission_wait_ms = EXCLUDED.permission_wait_ms,
             ended_at = EXCLUDED.ended_at,
             error_code = EXCLUDED.error_code,
             error_message = EXCLUDED.error_message,
             attributes = EXCLUDED.attributes",
    )
    .bind(&finished.started.span_id)
    .bind(finished.started.turn_id.as_str())
    .bind(&finished.started.parent_span_id)
    .bind(finished.started.sequence)
    .bind(finished.status.as_str())
    .bind(&finished.started.provider_call_id)
    .bind(&finished.started.requested_tool_name)
    .bind(&finished.resolved_tool_name)
    .bind(finished.permission_wait_ms)
    .bind(utc_naive(finished.started.started_at))
    .bind(utc_naive(finished.ended_at))
    .bind(&finished.error_code)
    .bind(&finished.error_message)
    .bind(Json(&finished.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn token(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| i64::try_from(value).ok())
}

fn utc_naive(value: OffsetDateTime) -> PrimitiveDateTime {
    let utc = value.to_offset(UtcOffset::UTC);
    PrimitiveDateTime::new(utc.date(), utc.time())
}

#[cfg(test)]
mod tests {
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    use super::utc_naive;

    #[test]
    fn converts_an_offset_timestamp_to_utc_without_timezone() {
        let source = OffsetDateTime::parse("2026-07-18T08:30:45+08:00", &Rfc3339).unwrap();

        let converted = utc_naive(source);

        assert_eq!(converted.to_string(), "2026-07-18 0:30:45.0");
    }
}
