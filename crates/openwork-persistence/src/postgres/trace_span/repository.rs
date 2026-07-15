use async_trait::async_trait;
use openwork_protocol::trace::{TraceRepository, TraceRepositoryError, TraceSpan};
use sqlx::PgPool;

use super::record::TraceSpanRecord;

const SPAN_SELECT: &str = r#"
    trace_id,
    span_id,
    parent_span_id,
    span_kind,
    span_name,
    status,
    session_id,
    turn_id,
    step_id,
    tool_run_id,
    (EXTRACT(EPOCH FROM (started_at AT TIME ZONE 'Asia/Shanghai')) * 1000)::BIGINT
        AS started_at_unix_ms,
    CASE WHEN ended_at IS NULL THEN NULL ELSE
        (EXTRACT(EPOCH FROM (ended_at AT TIME ZONE 'Asia/Shanghai')) * 1000)::BIGINT
    END AS ended_at_unix_ms,
    attributes_json,
    error_type,
    error_code,
    error_message
"#;

#[derive(Clone)]
pub struct PostgresTraceRepository {
    pool: PgPool,
}

impl PostgresTraceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TraceRepository for PostgresTraceRepository {
    async fn upsert_span(&self, span: TraceSpan) -> Result<(), TraceRepositoryError> {
        validate_span(&span)?;
        sqlx::query(
            r#"INSERT INTO trace_spans (
                 trace_id, span_id, parent_span_id, span_kind, span_name, status,
                 session_id, turn_id, step_id, tool_run_id, started_at, ended_at,
                 attributes_json, error_type, error_code, error_message
               ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 to_timestamp($11::DOUBLE PRECISION / 1000.0) AT TIME ZONE 'Asia/Shanghai',
                 CASE WHEN $12::BIGINT IS NULL THEN NULL ELSE
                   to_timestamp($12::DOUBLE PRECISION / 1000.0) AT TIME ZONE 'Asia/Shanghai'
                 END,
                 $13, $14, $15, $16
               )
               ON CONFLICT (span_id) DO UPDATE SET
                 status = EXCLUDED.status,
                 started_at = LEAST(trace_spans.started_at, EXCLUDED.started_at),
                 ended_at = COALESCE(EXCLUDED.ended_at, trace_spans.ended_at),
                 attributes_json = trace_spans.attributes_json || EXCLUDED.attributes_json,
                 error_type = COALESCE(EXCLUDED.error_type, trace_spans.error_type),
                 error_code = COALESCE(EXCLUDED.error_code, trace_spans.error_code),
                 error_message = COALESCE(EXCLUDED.error_message, trace_spans.error_message),
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'"#,
        )
        .bind(&span.trace_id)
        .bind(&span.span_id)
        .bind(&span.parent_span_id)
        .bind(span.span_kind.as_str())
        .bind(&span.span_name)
        .bind(span.status.as_str())
        .bind(&span.session_id)
        .bind(&span.turn_id)
        .bind(&span.step_id)
        .bind(&span.tool_run_id)
        .bind(span.started_at_unix_ms)
        .bind(span.ended_at_unix_ms)
        .bind(&span.attributes)
        .bind(&span.error_type)
        .bind(&span.error_code)
        .bind(&span.error_message)
        .execute(&self.pool)
        .await
        .map_err(persistence_error)?;
        Ok(())
    }

    async fn load_span(&self, span_id: &str) -> Result<Option<TraceSpan>, TraceRepositoryError> {
        if span_id.trim().is_empty() {
            return Err(TraceRepositoryError::InvalidSpan {
                message: "span_id must not be blank".to_string(),
            });
        }
        let sql = format!("SELECT {SPAN_SELECT} FROM trace_spans WHERE span_id = $1");
        sqlx::query_as::<_, TraceSpanRecord>(&sql)
            .bind(span_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(persistence_error)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn load_turn(&self, turn_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        load(&self.pool, "turn_id", turn_id).await
    }

    async fn load_session(&self, session_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        load(&self.pool, "session_id", session_id).await
    }
}

async fn load(
    pool: &PgPool,
    field: &str,
    value: &str,
) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
    if value.trim().is_empty() {
        return Err(TraceRepositoryError::InvalidSpan {
            message: format!("{field} must not be blank"),
        });
    }
    let sql = format!(
        "SELECT {SPAN_SELECT} FROM trace_spans WHERE {field} = $1 ORDER BY started_at, span_id"
    );
    sqlx::query_as::<_, TraceSpanRecord>(&sql)
        .bind(value)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
}

fn validate_span(span: &TraceSpan) -> Result<(), TraceRepositoryError> {
    for (name, value) in [
        ("trace_id", span.trace_id.as_str()),
        ("span_id", span.span_id.as_str()),
        ("span_name", span.span_name.as_str()),
        ("session_id", span.session_id.as_str()),
        ("turn_id", span.turn_id.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(TraceRepositoryError::InvalidSpan {
                message: format!("{name} must not be blank"),
            });
        }
    }
    if !span.attributes.is_object() {
        return Err(TraceRepositoryError::InvalidSpan {
            message: "attributes must be a JSON object".to_string(),
        });
    }
    if span
        .ended_at_unix_ms
        .is_some_and(|ended| ended < span.started_at_unix_ms)
    {
        return Err(TraceRepositoryError::InvalidSpan {
            message: "ended_at must not be before started_at".to_string(),
        });
    }
    Ok(())
}

fn persistence_error(error: impl std::fmt::Display) -> TraceRepositoryError {
    TraceRepositoryError::Persistence {
        message: error.to_string(),
    }
}
