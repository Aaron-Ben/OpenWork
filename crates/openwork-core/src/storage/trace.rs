use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgPool, Postgres, Transaction, types::Json};
use tokio::sync::{mpsc, oneshot};

use super::{TRACE_PAYLOAD_ADVISORY_LOCK, time::to_china};
use crate::session::{
    CompactionFinished, CompactionStarted, ModelCallFinished, ModelCallStarted, SessionId,
    ToolCallFinished, ToolCallStarted, TraceContentConfig, TraceContentPolicy, TraceFlushResult,
    TracePayloadSlot, TracePayloads, TraceRecorder, TraceSignal, TurnId,
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
    content_policy: Arc<AtomicU8>,
}

impl PostgresTraceRecorder {
    pub fn spawn(pool: PgPool) -> Self {
        Self::spawn_with_content_config(pool, TraceContentConfig::default())
    }

    pub fn spawn_with_capacity(pool: PgPool, capacity: usize) -> Self {
        Self::spawn_with_capacity_and_content_config(pool, capacity, TraceContentConfig::default())
    }

    pub fn spawn_with_content_config(pool: PgPool, content: TraceContentConfig) -> Self {
        Self::spawn_with_capacity_and_content_config(pool, DEFAULT_TRACE_QUEUE_CAPACITY, content)
    }

    pub fn spawn_with_capacity_and_content_config(
        pool: PgPool,
        capacity: usize,
        content: TraceContentConfig,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        let metrics = Arc::new(TraceMetrics::default());
        let content_policy = Arc::new(AtomicU8::new(encode_content_policy(content.policy())));
        tokio::spawn(run_writer(
            pool,
            receiver,
            Arc::clone(&metrics),
            content.slot_max_bytes(),
        ));
        Self {
            sender,
            metrics,
            content_policy,
        }
    }

    pub fn content_policy(&self) -> TraceContentPolicy {
        decode_content_policy(self.content_policy.load(Ordering::Acquire))
    }

    pub fn set_content_policy(&self, policy: TraceContentPolicy) {
        self.content_policy
            .store(encode_content_policy(policy), Ordering::Release);
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
    fn record(&self, mut signal: TraceSignal) {
        signal.apply_content_policy(self.content_policy());
        if self
            .sender
            .try_send(TraceCommand::Signal(Box::new(signal)))
            .is_err()
        {
            self.metrics.dropped_signals.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn flush_turn(&self, _turn_id: &TurnId) -> TraceFlushResult {
        self.flush().await
    }

    async fn flush_session(&self, _session_id: &SessionId) -> TraceFlushResult {
        self.flush().await
    }
}

impl PostgresTraceRecorder {
    async fn flush(&self) -> TraceFlushResult {
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
    slot_max_bytes: usize,
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

        if !signals.is_empty() {
            match write_batch(&pool, &signals, slot_max_bytes).await {
                Ok(payload_failures) => {
                    metrics
                        .write_failures
                        .fetch_add(payload_failures, Ordering::Relaxed);
                }
                Err(_) => {
                    metrics
                        .write_failures
                        .fetch_add(signals.len() as u64, Ordering::Relaxed);
                }
            }
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

async fn write_batch(
    pool: &PgPool,
    signals: &[TraceSignal],
    slot_max_bytes: usize,
) -> Result<u64, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    if signals.iter().any(signal_has_payloads) {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(TRACE_PAYLOAD_ADVISORY_LOCK)
            .execute(&mut *transaction)
            .await?;
    }
    let mut payload_failures = 0_u64;
    for signal in signals {
        write_span(&mut transaction, signal).await?;
        if signal_has_payloads(signal) {
            let mut payload_transaction = transaction.begin().await?;
            match write_signal_payloads(&mut payload_transaction, signal, slot_max_bytes).await {
                Ok(()) => payload_transaction.commit().await?,
                Err(_) => {
                    payload_transaction.rollback().await?;
                    payload_failures = payload_failures.saturating_add(1);
                }
            }
        }
    }
    transaction.commit().await?;
    Ok(payload_failures)
}

async fn write_span(
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
        TraceSignal::CompactionStarted(started) => {
            write_compaction_started(transaction, started).await
        }
        TraceSignal::CompactionFinished(finished) => {
            write_compaction_finished(transaction, finished).await
        }
    }
}

async fn write_model_started(
    transaction: &mut Transaction<'_, Postgres>,
    started: &ModelCallStarted,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trace_spans (
             id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
             model_id, resolved_model_name, started_at, attributes
         ) VALUES ($1, $2, $3, $4, $5, 'model_call', 'model.call', 'running',
                   $6, $7, $8, $9)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&started.span_id)
    .bind(&started.trace_id)
    .bind(started.session_id.as_str())
    .bind(started.turn_id.as_ref().map(TurnId::as_str))
    .bind(&started.parent_span_id)
    .bind(&started.model_id)
    .bind(&started.resolved_model_name)
    .bind(to_china(started.started_at))
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
             id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
             model_id, resolved_model_name, provider_request_id, attempt_count,
             input_tokens, output_tokens, cached_input_tokens, reasoning_tokens,
             response_message_id, started_at, ended_at, error_code, error_message, attributes
         ) VALUES ($1, $2, $3, $4, $5, 'model_call', 'model.call', $6, $7,
                   $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
         ON CONFLICT (id) DO UPDATE SET
             status = EXCLUDED.status,
             provider_request_id = EXCLUDED.provider_request_id,
             attempt_count = EXCLUDED.attempt_count,
             input_tokens = EXCLUDED.input_tokens,
             output_tokens = EXCLUDED.output_tokens,
             cached_input_tokens = EXCLUDED.cached_input_tokens,
             reasoning_tokens = EXCLUDED.reasoning_tokens,
             response_message_id = EXCLUDED.response_message_id,
             ended_at = EXCLUDED.ended_at,
             error_code = EXCLUDED.error_code,
             error_message = EXCLUDED.error_message,
             attributes = EXCLUDED.attributes",
    )
    .bind(&finished.started.span_id)
    .bind(&finished.started.trace_id)
    .bind(finished.started.session_id.as_str())
    .bind(finished.started.turn_id.as_ref().map(TurnId::as_str))
    .bind(&finished.started.parent_span_id)
    .bind(finished.status.as_str())
    .bind(&finished.started.model_id)
    .bind(&finished.started.resolved_model_name)
    .bind(&finished.provider_request_id)
    .bind(finished.attempt_count)
    .bind(usage.and_then(|value| token(value.input_tokens)))
    .bind(usage.and_then(|value| token(value.output_tokens)))
    .bind(usage.and_then(|value| token(value.cached_input_tokens)))
    .bind(usage.and_then(|value| token(value.reasoning_tokens)))
    .bind(&finished.response_message_id)
    .bind(to_china(finished.started.started_at))
    .bind(to_china(finished.ended_at))
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
             id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
             provider_call_id, requested_tool_name, started_at, attributes
         ) SELECT $1, $4, turn_row.session_id, $2, $3, 'tool_call', 'tool.call', 'running',
                  $5, $6, $7, $8
           FROM turns AS turn_row WHERE turn_row.id = $2
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&started.span_id)
    .bind(started.turn_id.as_str())
    .bind(&started.parent_span_id)
    .bind(&started.trace_id)
    .bind(&started.provider_call_id)
    .bind(&started.requested_tool_name)
    .bind(to_china(started.started_at))
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
             id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
             provider_call_id, requested_tool_name, resolved_tool_name,
             permission_wait_ms, started_at, ended_at, error_code, error_message, attributes
         ) SELECT $1, $4, turn_row.session_id, $2, $3, 'tool_call', 'tool.call', $5,
                  $6, $7, $8, $9, $10, $11, $12, $13, $14
           FROM turns AS turn_row WHERE turn_row.id = $2
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
    .bind(&finished.started.trace_id)
    .bind(finished.status.as_str())
    .bind(&finished.started.provider_call_id)
    .bind(&finished.started.requested_tool_name)
    .bind(&finished.resolved_tool_name)
    .bind(finished.permission_wait_ms)
    .bind(to_china(finished.started.started_at))
    .bind(to_china(finished.ended_at))
    .bind(&finished.error_code)
    .bind(&finished.error_message)
    .bind(Json(&finished.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_compaction_started(
    transaction: &mut Transaction<'_, Postgres>,
    started: &CompactionStarted,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trace_spans (
             id, trace_id, session_id, turn_id, kind, name, status, model_id,
             resolved_model_name, started_at, attributes
         ) VALUES ($1, $4, $2, $3, 'compaction', 'session.compact', 'running',
                   $5, $6, $7, $8)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&started.span_id)
    .bind(started.session_id.as_str())
    .bind(started.turn_id.as_ref().map(TurnId::as_str))
    .bind(&started.trace_id)
    .bind(&started.model_id)
    .bind(&started.resolved_model_name)
    .bind(to_china(started.started_at))
    .bind(Json(&started.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_compaction_finished(
    transaction: &mut Transaction<'_, Postgres>,
    finished: &CompactionFinished,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trace_spans (
             id, trace_id, session_id, turn_id, kind, name, status, model_id,
             resolved_model_name, attempt_count, input_tokens, output_tokens,
             started_at, ended_at, error_code, error_message, attributes
         ) VALUES ($1, $4, $2, $3, 'compaction', 'session.compact', $5, $6,
                   $7, $8, $9, $10, $11, $12, $13, $14, $15)
         ON CONFLICT (id) DO UPDATE SET
             status = EXCLUDED.status,
             attempt_count = EXCLUDED.attempt_count,
             input_tokens = EXCLUDED.input_tokens,
             output_tokens = EXCLUDED.output_tokens,
             ended_at = EXCLUDED.ended_at,
             error_code = EXCLUDED.error_code,
             error_message = EXCLUDED.error_message,
             attributes = EXCLUDED.attributes",
    )
    .bind(&finished.started.span_id)
    .bind(finished.started.session_id.as_str())
    .bind(finished.started.turn_id.as_ref().map(TurnId::as_str))
    .bind(&finished.started.trace_id)
    .bind(finished.status.as_str())
    .bind(&finished.started.model_id)
    .bind(&finished.started.resolved_model_name)
    .bind(finished.attempt_count)
    .bind(token(finished.input_tokens))
    .bind(token(finished.output_tokens))
    .bind(to_china(finished.started.started_at))
    .bind(to_china(finished.ended_at))
    .bind(&finished.error_code)
    .bind(&finished.error_message)
    .bind(Json(&finished.attributes))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn signal_has_payloads(signal: &TraceSignal) -> bool {
    match signal {
        TraceSignal::ModelCallStarted(started) => payloads_are_present(&started.payloads),
        TraceSignal::ModelCallFinished(finished) => {
            payloads_are_present(&finished.started.payloads) || finished.response_payload.is_some()
        }
        TraceSignal::ToolCallFinished(finished) => finished.response_payload.is_some(),
        TraceSignal::ToolCallStarted(_)
        | TraceSignal::CompactionStarted(_)
        | TraceSignal::CompactionFinished(_) => false,
    }
}

fn payloads_are_present(payloads: &TracePayloads) -> bool {
    payloads.request.is_some()
        || payloads.system_context.is_some()
        || payloads.tool_definitions.is_some()
}

async fn write_signal_payloads(
    transaction: &mut Transaction<'_, Postgres>,
    signal: &TraceSignal,
    slot_max_bytes: usize,
) -> Result<(), PayloadWriteError> {
    let (span_id, payloads, response) = match signal {
        TraceSignal::ModelCallStarted(started) => {
            (started.span_id.as_str(), Some(&started.payloads), None)
        }
        TraceSignal::ModelCallFinished(finished) => (
            finished.started.span_id.as_str(),
            Some(&finished.started.payloads),
            finished.response_payload.as_ref(),
        ),
        TraceSignal::ToolCallFinished(finished) => (
            finished.started.span_id.as_str(),
            None,
            finished.response_payload.as_ref(),
        ),
        TraceSignal::ToolCallStarted(_)
        | TraceSignal::CompactionStarted(_)
        | TraceSignal::CompactionFinished(_) => return Ok(()),
    };

    if let Some(payloads) = payloads {
        for (slot, body) in [
            (TracePayloadSlot::Request, payloads.request.as_ref()),
            (
                TracePayloadSlot::SystemContext,
                payloads.system_context.as_ref(),
            ),
            (
                TracePayloadSlot::ToolDefinitions,
                payloads.tool_definitions.as_ref(),
            ),
        ] {
            if let Some(body) = body {
                write_payload(transaction, span_id, slot, body, slot_max_bytes).await?;
            }
        }
    }
    if let Some(response) = response {
        write_payload(
            transaction,
            span_id,
            TracePayloadSlot::Response,
            response,
            slot_max_bytes,
        )
        .await?;
    }
    Ok(())
}

fn encode_content_policy(policy: TraceContentPolicy) -> u8 {
    match policy {
        TraceContentPolicy::Full => 0,
        TraceContentPolicy::CompactionOnly => 1,
        TraceContentPolicy::Off => 2,
    }
}

fn decode_content_policy(value: u8) -> TraceContentPolicy {
    match value {
        1 => TraceContentPolicy::CompactionOnly,
        2 => TraceContentPolicy::Off,
        _ => TraceContentPolicy::Full,
    }
}

async fn write_payload(
    transaction: &mut Transaction<'_, Postgres>,
    span_id: &str,
    slot: TracePayloadSlot,
    body: &Value,
    max_bytes: usize,
) -> Result<(), PayloadWriteError> {
    let prepared = PreparedPayload::new(body, max_bytes)?;
    sqlx::query(
        "INSERT INTO trace_payloads (hash, body, byte_size)
         VALUES ($1, $2, $3)
         ON CONFLICT (hash) DO NOTHING",
    )
    .bind(&prepared.hash)
    .bind(Json(&prepared.body))
    .bind(prepared.byte_size)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO trace_span_payloads (
             span_id, slot, payload_hash, truncated, original_byte_size, redacted_count
         ) VALUES ($1, $2, $3, $4, $5, 0)
         ON CONFLICT (span_id, slot) DO NOTHING",
    )
    .bind(span_id)
    .bind(slot.as_str())
    .bind(&prepared.hash)
    .bind(prepared.truncated)
    .bind(prepared.original_byte_size)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum PayloadWriteError {
    #[error(transparent)]
    Serialize(#[from] serde_json::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

struct PreparedPayload {
    hash: String,
    body: Value,
    byte_size: i64,
    truncated: bool,
    original_byte_size: Option<i64>,
}

impl PreparedPayload {
    fn new(body: &Value, max_bytes: usize) -> Result<Self, serde_json::Error> {
        let original = serde_json::to_vec(body)?;
        let (body, truncated) = if original.len() > max_bytes {
            (truncated_preview(&original, max_bytes)?, true)
        } else {
            (body.clone(), false)
        };
        // Hash the canonical bytes exactly once before insertion. PostgreSQL
        // JSONB may normalize the stored representation, so callers must never
        // attempt to reconstruct this identity from a later read.
        let canonical = serde_json::to_vec(&body)?;
        let digest = Sha256::digest(&canonical);
        let mut hash = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write;
            let _ = write!(hash, "{byte:02x}");
        }
        Ok(Self {
            hash,
            body,
            byte_size: saturating_i64(canonical.len()),
            truncated,
            original_byte_size: truncated.then(|| saturating_i64(original.len())),
        })
    }
}

fn truncated_preview(bytes: &[u8], max_bytes: usize) -> Result<Value, serde_json::Error> {
    let text = std::str::from_utf8(bytes).expect("serde_json always emits UTF-8");
    let wrapped_empty = json!({"truncatedPreview": ""});
    let wrapped = serde_json::to_vec(&wrapped_empty)?.len() <= max_bytes;
    // A JSON string cannot encode more source bytes than its total serialized
    // byte budget. Limit the boundary index to the configured preview size so
    // a very large request does not allocate an index proportional to the full
    // untruncated body.
    let mut prefix_limit = max_bytes.min(text.len());
    while !text.is_char_boundary(prefix_limit) {
        prefix_limit -= 1;
    }
    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index < prefix_limit)
        .chain(std::iter::once(prefix_limit))
        .collect::<Vec<_>>();
    let mut low = 0_usize;
    let mut high = boundaries.len();
    let mut best = if wrapped {
        wrapped_empty
    } else {
        Value::String(String::new())
    };
    while low < high {
        let middle = low + (high - low) / 2;
        let prefix = &text[..boundaries[middle]];
        let candidate = if wrapped {
            json!({"truncatedPreview": prefix})
        } else {
            Value::String(prefix.to_string())
        };
        if serde_json::to_vec(&candidate)?.len() <= max_bytes {
            best = candidate;
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Ok(best)
}

fn saturating_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn token(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| i64::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    use super::{PreparedPayload, to_china};

    #[test]
    fn stores_span_timestamps_as_beijing_wall_clock() {
        let source = OffsetDateTime::parse("2026-07-18T00:30:45Z", &Rfc3339).unwrap();

        let converted = to_china(source);

        assert_eq!(converted.to_string(), "2026-07-18 8:30:45.0");
    }

    #[test]
    fn canonical_payload_hash_is_independent_of_json_object_key_order() {
        let first = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        let second = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();

        let first = PreparedPayload::new(&first, 1_024).unwrap();
        let second = PreparedPayload::new(&second, 1_024).unwrap();

        assert_eq!(first.hash, second.hash);
        assert_eq!(first.body, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn truncation_produces_valid_json_with_bounded_serialized_bytes() {
        let prepared = PreparedPayload::new(&json!({"source": "x".repeat(4_096)}), 96).unwrap();

        assert!(prepared.truncated);
        assert!(prepared.original_byte_size.is_some_and(|size| size > 96));
        assert!(prepared.byte_size <= 96);
        assert!(serde_json::to_vec(&prepared.body).is_ok());
    }
}
