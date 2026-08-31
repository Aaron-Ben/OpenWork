use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};

use crate::protocol::{
    AppendRunEventsRequest, RunEventInput, RunEventView, RunSummaryView, RunTraceView,
};

use super::auth::{AgentClaims, authorize_agent_transaction};

const MAX_EVENT_BATCH: usize = 32;
const MAX_EVENT_DATA_BYTES: usize = 32 * 1024;

const SUMMARY_SELECT: &str =
    "SELECT run.id, run.agent_id, run.runtime_session_id, run.trigger, run.status,
            run.engine_id, run.main_model_id,
            (SELECT event.data ->> 'observedModelId'
             FROM collab_run_events event
             WHERE event.run_id = run.id AND event.kind = 'engine.completed'
             ORDER BY event.created_at DESC, event.id DESC LIMIT 1) AS observed_model_id,
            run.outcome, run.room_id, run.focus_card_id, run.trigger_reason,
            run.error_code, run.error_message,
            CASE WHEN run.status = 'running' THEN COALESCE(
                (SELECT event.kind FROM collab_run_events event
                 WHERE event.run_id = run.id
                 ORDER BY event.created_at DESC, event.id DESC LIMIT 1),
                'run.opened'
            ) ELSE 'run.' || run.status END AS stage,
            to_char(run.started_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.MS') || '+08:00' AS started_at,
            to_char(run.heartbeat_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.MS') || '+08:00' AS heartbeat_at,
            CASE WHEN run.ended_at IS NULL THEN NULL ELSE
                to_char(run.ended_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.MS') || '+08:00'
            END AS ended_at,
            ROUND(EXTRACT(EPOCH FROM (
                COALESCE(run.ended_at, CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                - run.started_at
            )) * 1000)::BIGINT AS duration_ms,
            run.input_tokens, run.cached_input_tokens,
            (SELECT NULLIF(event.data #>> '{usage,cacheCreationInputTokens}', '')::BIGINT
             FROM collab_run_events event
             WHERE event.run_id = run.id AND event.kind = 'engine.completed'
             ORDER BY event.created_at DESC, event.id DESC LIMIT 1)
                AS cache_creation_input_tokens,
            run.output_tokens, run.rate_limit_percent,
            (SELECT COUNT(*) FROM collab_command_requests request
             WHERE request.run_id = run.id AND request.result IS NOT NULL) AS tool_calls,
            1
              + CASE WHEN run.ended_at IS NULL THEN 0 ELSE 1 END
              + (SELECT COUNT(*) FROM collab_run_events event WHERE event.run_id = run.id)
              + (SELECT COUNT(*) FROM collab_triages triage WHERE triage.run_id = run.id)
              + (SELECT COUNT(*) FROM collab_command_requests request
                 WHERE request.run_id = run.id AND request.result IS NOT NULL) AS event_count,
            COALESCE((SELECT SUM(delivery.up_to_seq - delivery.from_seq + 1)
                      FROM collab_run_deliveries delivery
                      WHERE delivery.run_id = run.id), 0)::BIGINT AS inbox_message_count
     FROM collab_runs run";

#[derive(Clone)]
pub(crate) struct Observability {
    pool: PgPool,
}

impl Observability {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn list_runs(
        &self,
        agent_id: Option<&str>,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<RunSummaryView>, sqlx::Error> {
        validate_status_filter(status)?;
        let mut query = QueryBuilder::<Postgres>::new(SUMMARY_SELECT);
        query.push(" WHERE TRUE");
        if let Some(agent_id) = agent_id.filter(|value| !value.is_empty()) {
            query.push(" AND run.agent_id = ").push_bind(agent_id);
        }
        if let Some(status) = status.filter(|value| !value.is_empty()) {
            query.push(" AND run.status = ").push_bind(status);
        }
        query
            .push(" ORDER BY run.started_at DESC, run.id DESC LIMIT ")
            .push_bind(i64::from(limit.clamp(1, 200)));
        query
            .build_query_as::<RunSummaryRow>()
            .fetch_all(&self.pool)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn trace(&self, run_id: &str) -> Result<RunTraceView, sqlx::Error> {
        let run = self.run(run_id).await?;
        let mut events = Vec::new();
        events.push(RunEventView {
            id: format!("{}:opened", run.id),
            source: "server".to_string(),
            kind: "run.opened".to_string(),
            level: "info".to_string(),
            data: json!({
                "trigger": run.trigger,
                "roomId": run.room_id,
                "focusCardId": run.focus_card_id,
                "triggerReason": run.trigger_reason,
                "inboxMessageCount": run.inbox_message_count,
            }),
            created_at: run.started_at.clone(),
        });
        events.extend(self.recorded_events(run_id).await?);
        events.extend(self.triage_events(run_id).await?);
        events.extend(self.command_events(run_id).await?);
        if let Some(ended_at) = &run.ended_at {
            events.push(RunEventView {
                id: format!("{}:terminal", run.id),
                source: "server".to_string(),
                kind: format!("run.{}", run.status),
                level: if run.status == "completed" {
                    "info"
                } else {
                    "error"
                }
                .to_string(),
                data: json!({
                    "outcome": run.outcome,
                    "errorCode": run.error_code,
                    "errorMessage": run.error_message,
                    "durationMs": run.duration_ms,
                    "usage": {
                        "inputTokens": run.input_tokens,
                        "cachedInputTokens": run.cached_input_tokens,
                        "cacheCreationInputTokens": run.cache_creation_input_tokens,
                        "outputTokens": run.output_tokens,
                    }
                }),
                created_at: ended_at.clone(),
            });
        }
        events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| event_rank(&left.kind).cmp(&event_rank(&right.kind)))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(RunTraceView { run, events })
    }

    pub(crate) async fn append(
        &self,
        claims: &AgentClaims,
        run_id: &str,
        request: AppendRunEventsRequest,
    ) -> Result<(), sqlx::Error> {
        validate_events(&request.events)?;
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_runs
                WHERE id = $1 AND agent_id = $2 AND runtime_session_id = $3
                  AND status = 'running'
             )",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !active {
            return Err(sqlx::Error::RowNotFound);
        }
        for event in request.events {
            let (source, level) = event_shape(&event.kind)
                .expect("event kind was checked before the transaction opened");
            sqlx::query(
                "INSERT INTO collab_run_events (id, run_id, source, kind, level, data)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(event.id)
            .bind(run_id)
            .bind(source)
            .bind(event.kind)
            .bind(level)
            .bind(event.data)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    async fn run(&self, run_id: &str) -> Result<RunSummaryView, sqlx::Error> {
        let mut query = QueryBuilder::<Postgres>::new(SUMMARY_SELECT);
        query.push(" WHERE run.id = ").push_bind(run_id);
        query
            .build_query_as::<RunSummaryRow>()
            .fetch_one(&self.pool)
            .await
            .map(Into::into)
    }

    async fn recorded_events(&self, run_id: &str) -> Result<Vec<RunEventView>, sqlx::Error> {
        sqlx::query_as::<_, RunEventRow>(
            "SELECT id, source, kind, level, data,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.MS') || '+08:00'
                        AS created_at
             FROM collab_run_events
             WHERE run_id = $1 ORDER BY created_at, id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    async fn triage_events(&self, run_id: &str) -> Result<Vec<RunEventView>, sqlx::Error> {
        sqlx::query_as::<_, RunEventRow>(
            "SELECT id, 'server'::TEXT AS source, 'triage.completed'::TEXT AS kind,
                    'info'::TEXT AS level,
                    jsonb_build_object(
                        'roomId', room_id,
                        'actionable', actionable,
                        'source', source,
                        'reason', reason,
                        'modelId', model_id,
                        'inputTokens', input_tokens,
                        'outputTokens', output_tokens,
                        'latencyMs', latency_ms
                    ) AS data,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.MS') || '+08:00'
                        AS created_at
             FROM collab_triages
             WHERE run_id = $1 ORDER BY created_at, id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    async fn command_events(&self, run_id: &str) -> Result<Vec<RunEventView>, sqlx::Error> {
        sqlx::query_as::<_, RunEventRow>(
            "SELECT id, 'server'::TEXT AS source, 'command.completed'::TEXT AS kind,
                    CASE WHEN result #>> '{result,type}' = 'error'
                         THEN 'error' ELSE 'info' END::TEXT AS level,
                    jsonb_build_object('requestId', request_id, 'response', result) AS data,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.MS') || '+08:00'
                        AS created_at
             FROM collab_command_requests
             WHERE run_id = $1 AND result IS NOT NULL ORDER BY created_at, id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }
}

#[derive(FromRow)]
struct RunSummaryRow {
    id: String,
    agent_id: String,
    runtime_session_id: String,
    trigger: String,
    status: String,
    engine_id: String,
    main_model_id: String,
    observed_model_id: Option<String>,
    outcome: Option<String>,
    room_id: Option<String>,
    focus_card_id: Option<String>,
    trigger_reason: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    stage: String,
    started_at: String,
    heartbeat_at: String,
    ended_at: Option<String>,
    duration_ms: i64,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    rate_limit_percent: Option<f64>,
    tool_calls: i64,
    event_count: i64,
    inbox_message_count: i64,
}

impl From<RunSummaryRow> for RunSummaryView {
    fn from(row: RunSummaryRow) -> Self {
        Self {
            id: row.id,
            agent_id: row.agent_id,
            runtime_session_id: row.runtime_session_id,
            trigger: row.trigger,
            status: row.status,
            engine_id: row.engine_id,
            main_model_id: row.main_model_id,
            observed_model_id: row.observed_model_id,
            outcome: row.outcome,
            room_id: row.room_id,
            focus_card_id: row.focus_card_id,
            trigger_reason: row.trigger_reason,
            error_code: row.error_code,
            error_message: row.error_message,
            stage: row.stage,
            started_at: row.started_at,
            heartbeat_at: row.heartbeat_at,
            ended_at: row.ended_at,
            duration_ms: row.duration_ms,
            input_tokens: row.input_tokens,
            cached_input_tokens: row.cached_input_tokens,
            cache_creation_input_tokens: row.cache_creation_input_tokens,
            output_tokens: row.output_tokens,
            rate_limit_percent: row.rate_limit_percent,
            tool_calls: row.tool_calls,
            event_count: row.event_count,
            inbox_message_count: row.inbox_message_count,
        }
    }
}

#[derive(FromRow)]
struct RunEventRow {
    id: String,
    source: String,
    kind: String,
    level: String,
    data: Value,
    created_at: String,
}

impl From<RunEventRow> for RunEventView {
    fn from(row: RunEventRow) -> Self {
        Self {
            id: row.id,
            source: row.source,
            kind: row.kind,
            level: row.level,
            data: row.data,
            created_at: row.created_at,
        }
    }
}

fn validate_status_filter(status: Option<&str>) -> Result<(), sqlx::Error> {
    if status
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            !matches!(
                value,
                "running" | "completed" | "failed" | "cancelled" | "interrupted"
            )
        })
    {
        return Err(protocol_error(
            "INVALID_ARGUMENT: invalid run status filter",
        ));
    }
    Ok(())
}

fn validate_events(events: &[RunEventInput]) -> Result<(), sqlx::Error> {
    if events.is_empty() || events.len() > MAX_EVENT_BATCH {
        return Err(protocol_error(
            "INVALID_ARGUMENT: invalid run event batch size",
        ));
    }
    for event in events {
        let valid_id = event.id.len() == 38
            && event.id.starts_with("event-")
            && event.id[6..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if !valid_id || event_shape(&event.kind).is_none() || !event.data.is_object() {
            return Err(protocol_error("INVALID_ARGUMENT: invalid run event"));
        }
        let bytes = serde_json::to_vec(&event.data)
            .map_err(|error| protocol_error(&format!("INVALID_ARGUMENT: {error}")))?;
        if bytes.len() > MAX_EVENT_DATA_BYTES {
            return Err(protocol_error(
                "INVALID_ARGUMENT: run event data is too large",
            ));
        }
    }
    Ok(())
}

fn event_shape(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "triage.started" => Some(("runner", "info")),
        "engine.started" | "engine.completed" => Some(("engine", "info")),
        "engine.failed" => Some(("engine", "error")),
        "engine.cancelled" => Some(("engine", "warning")),
        _ => None,
    }
}

fn event_rank(kind: &str) -> u8 {
    match kind {
        "run.opened" => 0,
        "triage.started" => 1,
        "triage.completed" => 2,
        "engine.started" => 3,
        "command.completed" => 4,
        "engine.completed" | "engine.failed" | "engine.cancelled" => 5,
        kind if kind.starts_with("run.") => 6,
        _ => 4,
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_interface_accepts_only_bounded_engine_neutral_events() {
        let valid = RunEventInput {
            id: format!("event-{}", "a".repeat(32)),
            kind: "engine.started".to_string(),
            data: json!({"configuredModelId": "deepseek/v4-flash"}),
        };
        assert!(validate_events(std::slice::from_ref(&valid)).is_ok());

        let mut unknown = valid.clone();
        unknown.kind = "opencode.raw.chunk".to_string();
        assert!(validate_events(&[unknown]).is_err());

        let mut oversized = valid;
        oversized.data = json!({"text": "x".repeat(MAX_EVENT_DATA_BYTES)});
        assert!(validate_events(&[oversized]).is_err());
    }
}
