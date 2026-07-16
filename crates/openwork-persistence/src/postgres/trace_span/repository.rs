use async_trait::async_trait;
use openwork_protocol::trace::{TraceRepository, TraceRepositoryError, TraceRootFilter, TraceSpan};
use sqlx::{PgPool, Postgres, QueryBuilder};

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

    async fn load_recent_turns(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let sql = format!(
            r#"SELECT {SPAN_SELECT}
               FROM trace_spans
               WHERE turn_id IN (
                 SELECT turn_id
                 FROM trace_spans
                 WHERE span_kind = 'turn'
                 ORDER BY started_at DESC, turn_id
                 LIMIT $1 OFFSET $2
               )
               ORDER BY (
                 SELECT root.started_at
                 FROM trace_spans AS root
                 WHERE root.turn_id = trace_spans.turn_id
                   AND root.span_kind = 'turn'
                 LIMIT 1
               ) DESC, started_at, span_id"#
        );
        sqlx::query_as::<_, TraceSpanRecord>(&sql)
            .bind(i64::from(limit))
            .bind(i64::from(offset))
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    async fn load_recent_turns_filtered(
        &self,
        filter: &TraceRootFilter,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut roots = QueryBuilder::<Postgres>::new(
            "SELECT root.turn_id FROM trace_spans AS root WHERE root.span_kind = 'turn'",
        );
        if let Some(session_ids) = &filter.session_ids {
            if session_ids.is_empty() {
                return Ok(Vec::new());
            }
            roots
                .push(" AND root.session_id = ANY(")
                .push_bind(session_ids.clone())
                .push(")");
        }
        if let Some(status) = filter.status {
            roots.push(" AND root.status = ").push_bind(status.as_str());
        }
        if let Some(model) = non_blank(filter.model.as_deref()) {
            roots
                .push(" AND LOWER(COALESCE(root.attributes_json ->> 'model', '')) = ")
                .push_bind(model.to_lowercase());
        }
        if let Some(after) = filter.started_after_unix_ms {
            roots
                .push(" AND root.started_at >= to_timestamp(")
                .push_bind(after as f64 / 1_000.0)
                .push(") AT TIME ZONE 'Asia/Shanghai'");
        }
        if let Some(before) = filter.started_before_unix_ms {
            roots
                .push(" AND root.started_at <= to_timestamp(")
                .push_bind(before as f64 / 1_000.0)
                .push(") AT TIME ZONE 'Asia/Shanghai'");
        }
        if let Some(has_error) = filter.has_error {
            roots.push(if has_error {
                " AND EXISTS (SELECT 1 FROM trace_spans AS failed WHERE failed.turn_id = root.turn_id AND failed.status = 'failed')"
            } else {
                " AND NOT EXISTS (SELECT 1 FROM trace_spans AS failed WHERE failed.turn_id = root.turn_id AND failed.status = 'failed')"
            });
        }
        if let Some(has_retry) = filter.has_retry {
            roots.push(if has_retry {
                " AND (SELECT COUNT(*) FROM trace_spans AS transport WHERE transport.turn_id = root.turn_id AND transport.span_kind = 'transport_attempt') > (SELECT COUNT(*) FROM trace_spans AS model WHERE model.turn_id = root.turn_id AND model.span_kind = 'model_attempt')"
            } else {
                " AND (SELECT COUNT(*) FROM trace_spans AS transport WHERE transport.turn_id = root.turn_id AND transport.span_kind = 'transport_attempt') <= (SELECT COUNT(*) FROM trace_spans AS model WHERE model.turn_id = root.turn_id AND model.span_kind = 'model_attempt')"
            });
        }
        if let Some(query) = non_blank(filter.query.as_deref()) {
            let pattern = format!("%{}%", query.to_lowercase());
            roots
                .push(" AND (LOWER(root.turn_id) LIKE ")
                .push_bind(pattern.clone())
                .push(" OR LOWER(root.trace_id) LIKE ")
                .push_bind(pattern.clone())
                .push(" OR LOWER(root.session_id) LIKE ")
                .push_bind(pattern.clone())
                .push(" OR LOWER(COALESCE(root.attributes_json ->> 'model', '')) LIKE ")
                .push_bind(pattern);
            if !filter.search_session_ids.is_empty() {
                roots
                    .push(" OR root.session_id = ANY(")
                    .push_bind(filter.search_session_ids.clone())
                    .push(")");
            }
            if !filter.search_turn_ids.is_empty() {
                roots
                    .push(" OR root.turn_id = ANY(")
                    .push_bind(filter.search_turn_ids.clone())
                    .push(")");
            }
            roots.push(")");
        }
        roots
            .push(" ORDER BY root.started_at DESC, root.turn_id LIMIT ")
            .push_bind(i64::from(limit))
            .push(" OFFSET ")
            .push_bind(i64::from(offset));

        let turn_ids = roots
            .build_query_scalar::<String>()
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        if turn_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut spans = QueryBuilder::<Postgres>::new(format!(
            "SELECT {SPAN_SELECT} FROM trace_spans WHERE turn_id = ANY("
        ));
        spans
            .push_bind(turn_ids)
            .push(") ORDER BY started_at, span_id");
        spans
            .build_query_as::<TraceSpanRecord>()
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    async fn prune_ended_turns_before(
        &self,
        ended_before_unix_ms: i64,
        turn_limit: u32,
    ) -> Result<u64, TraceRepositoryError> {
        if turn_limit == 0 {
            return Ok(0);
        }
        let deleted = sqlx::query_scalar::<_, i64>(
            r#"WITH expired_turns AS (
                 SELECT turn_id
                 FROM trace_spans
                 GROUP BY turn_id
                 HAVING BOOL_AND(ended_at IS NOT NULL)
                    AND MAX(ended_at) <
                        to_timestamp($1::DOUBLE PRECISION / 1000.0)
                        AT TIME ZONE 'Asia/Shanghai'
                 ORDER BY MAX(ended_at), turn_id
                 LIMIT $2
               ), deleted AS (
                 DELETE FROM trace_spans AS spans
                 USING expired_turns
                 WHERE spans.turn_id = expired_turns.turn_id
                 RETURNING spans.span_id
               )
               SELECT COUNT(*)::BIGINT FROM deleted"#,
        )
        .bind(ended_before_unix_ms)
        .bind(i64::from(turn_limit))
        .fetch_one(&self.pool)
        .await
        .map_err(persistence_error)?;
        u64::try_from(deleted).map_err(|_| TraceRepositoryError::Persistence {
            message: "retention returned a negative deleted row count".to_string(),
        })
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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
