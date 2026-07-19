use std::time::Duration;

use async_trait::async_trait;
use openwork_models::model::{ContentBlock, Message, Role, TokenUsage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Executor, PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

use crate::session::{
    ClientRequestId, ResolvedModel, SessionId, SessionStorage, TurnId, TurnOutcome,
};

const DEFAULT_DATABASE_URL: &str = "postgres://openwork:openwork@localhost:5432/openwork";

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInput {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub model_name: String,
    pub base_url: String,
    pub credential_ref: Option<String>,
    pub enabled: bool,
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ModelRecord {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub model_name: String,
    pub base_url: String,
    pub credential_ref: Option<String>,
    pub enabled: bool,
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    pub id: SessionId,
    pub title: Option<String>,
    pub working_directory: String,
    pub default_model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub title: Option<String>,
    pub working_directory: String,
    pub default_model_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_turn_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessageRecord {
    pub id: String,
    pub turn_id: Option<String>,
    pub sequence: i64,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TraceTurnSummary {
    pub turn_id: String,
    pub session_id: String,
    pub turn_sequence: i64,
    pub status: String,
    pub resolved_model_name: String,
    pub model_call_count: i32,
    pub tool_call_count: i32,
    pub span_count: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpanRecord {
    pub id: String,
    pub turn_id: String,
    pub parent_span_id: Option<String>,
    pub sequence: i64,
    pub kind: String,
    pub name: String,
    pub status: String,
    pub model_id: Option<String>,
    pub resolved_model_name: Option<String>,
    pub provider_request_id: Option<String>,
    pub provider_call_id: Option<String>,
    pub requested_tool_name: Option<String>,
    pub resolved_tool_name: Option<String>,
    pub attempt_count: Option<i32>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub permission_wait_ms: Option<i64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attributes: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceCompletenessState {
    Complete,
    Partial,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceCompleteness {
    pub expected_model_calls: u32,
    pub captured_model_calls: u32,
    pub expected_tool_calls: u32,
    pub captured_tool_calls: u32,
    pub orphan_tool_spans: u32,
    pub running_spans: u32,
    pub outcome_unknown_spans: u32,
    pub state: TraceCompletenessState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTrace {
    pub summary: TraceTurnSummary,
    pub spans: Vec<TraceSpanRecord>,
    pub completeness: TraceCompleteness,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid storage input: {0}")]
    InvalidInput(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("turn not found: {0}")]
    TurnNotFound(String),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Debug, Clone)]
pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    pub async fn connect(database_url: Option<&str>) -> Result<Self, StorageError> {
        let database_url = database_url.unwrap_or(DEFAULT_DATABASE_URL);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|connection, _metadata| {
                Box::pin(async move {
                    connection.execute("SET TIME ZONE 'UTC'").await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StorageError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub async fn upsert_model(&self, input: &ModelInput) -> Result<(), StorageError> {
        validate_model(input)?;
        sqlx::query(
            "INSERT INTO models (
                 id, display_name, provider_kind, model_name, base_url,
                 credential_ref, enabled, config
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET
                 display_name = EXCLUDED.display_name,
                 provider_kind = EXCLUDED.provider_kind,
                 model_name = EXCLUDED.model_name,
                 base_url = EXCLUDED.base_url,
                 credential_ref = EXCLUDED.credential_ref,
                 enabled = EXCLUDED.enabled,
                 config = EXCLUDED.config,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'",
        )
        .bind(&input.id)
        .bind(&input.display_name)
        .bind(&input.provider_kind)
        .bind(&input.model_name)
        .bind(&input.base_url)
        .bind(&input.credential_ref)
        .bind(input.enabled)
        .bind(&input.config)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_model(&self, id: &str) -> Result<Option<ModelRecord>, StorageError> {
        let model = sqlx::query_as::<_, ModelRecord>(
            "SELECT id, display_name, provider_kind, model_name, base_url,
                    credential_ref, enabled, config
             FROM models WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(model)
    }

    pub async fn create_session(
        &self,
        input: &SessionInput,
    ) -> Result<SessionRecord, StorageError> {
        validate_session(input)?;
        sqlx::query(
            "INSERT INTO sessions (id, title, working_directory, default_model_id)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(input.id.as_str())
        .bind(&input.title)
        .bind(&input.working_directory)
        .bind(&input.default_model_id)
        .execute(&self.pool)
        .await?;
        self.load_session(&input.id)
            .await?
            .ok_or_else(|| StorageError::SessionNotFound(input.id.to_string()))
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, StorageError> {
        let query = format!("{SESSION_COLUMNS} ORDER BY updated_at DESC, id");
        let sessions = sqlx::query_as::<_, SessionRecord>(&query)
            .fetch_all(&self.pool)
            .await?;
        Ok(sessions)
    }

    pub async fn load_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionRecord>, StorageError> {
        let query = format!("{SESSION_COLUMNS} WHERE id = $1");
        let session = sqlx::query_as::<_, SessionRecord>(&query)
            .bind(session_id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(session)
    }

    pub async fn rename_session(
        &self,
        session_id: &SessionId,
        title: &str,
    ) -> Result<SessionRecord, StorageError> {
        if title.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "session title must not be blank".to_string(),
            ));
        }
        let result = sqlx::query(
            "UPDATE sessions
                 SET title = $2, updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
                 WHERE id = $1",
        )
        .bind(session_id.as_str())
        .bind(title.trim())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::SessionNotFound(session_id.to_string()));
        }
        self.load_session(session_id)
            .await?
            .ok_or_else(|| StorageError::SessionNotFound(session_id.to_string()))
    }

    pub async fn delete_session(&self, session_id: &SessionId) -> Result<(), StorageError> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id.as_str())
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn load_messages(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<Message>, StorageError> {
        let rows: Vec<(String, Value)> = sqlx::query_as(
            "SELECT role, content
             FROM messages
             WHERE session_id = $1
             ORDER BY sequence",
        )
        .bind(session_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(role, content)| {
                Ok(Message {
                    role: parse_role(&role)?,
                    content: serde_json::from_value(content)?,
                })
            })
            .collect()
    }

    pub async fn load_message_records(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let rows: Vec<(String, Option<String>, i64, String, Value, String)> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at
             FROM messages
             WHERE session_id = $1
             ORDER BY sequence",
        )
        .bind(session_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(id, turn_id, sequence, role, content, created_at)| {
                Ok(StoredMessageRecord {
                    id,
                    turn_id,
                    sequence,
                    role: parse_role(&role)?,
                    content: serde_json::from_value(content)?,
                    created_at,
                })
            })
            .collect()
    }

    pub async fn replace_message_contents(
        &self,
        session_id: &SessionId,
        updates: &[(String, Vec<ContentBlock>)],
    ) -> Result<(), StorageError> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        for (message_id, content) in updates {
            let encoded = serde_json::to_value(content)?;
            let result = sqlx::query(
                "UPDATE messages
                 SET content = $3
                 WHERE id = $1 AND session_id = $2 AND role = 'tool'",
            )
            .bind(message_id)
            .bind(session_id.as_str())
            .bind(encoded)
            .execute(&mut *transaction)
            .await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::InvalidInput(format!(
                    "tool message not found for content update: {message_id}"
                )));
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn mark_running_interrupted(&self) -> Result<u64, StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE trace_spans
             SET status = 'outcome_unknown',
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC',
                 error_code = COALESCE(error_code, 'process_restart'),
                 error_message = COALESCE(error_message, 'process restarted before span completed')
             WHERE status = 'running'",
        )
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query(
            "UPDATE turns
             SET status = 'interrupted',
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC',
                 error_code = COALESCE(error_code, 'process_restart'),
                 error_message = COALESCE(error_message, 'process restarted before turn completed')
             WHERE status = 'running'",
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    pub async fn list_traces(
        &self,
        session_id: Option<&SessionId>,
        limit: i64,
    ) -> Result<Vec<TraceTurnSummary>, StorageError> {
        if !(1..=500).contains(&limit) {
            return Err(StorageError::InvalidInput(
                "trace list limit must be between 1 and 500".to_string(),
            ));
        }
        let traces = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT turns.id AS turn_id, turns.session_id,
                    turns.sequence AS turn_sequence, turns.status,
                    turns.resolved_model_name, turns.model_call_count,
                    turns.tool_call_count, COUNT(spans.id)::BIGINT AS span_count,
                    to_char(turns.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS started_at,
                    to_char(turns.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS ended_at
             FROM turns turns
             LEFT JOIN trace_spans spans ON spans.turn_id = turns.id
             WHERE ($1::TEXT IS NULL OR turns.session_id = $1)
             GROUP BY turns.id
             ORDER BY turns.started_at DESC, turns.id
             LIMIT $2",
        )
        .bind(session_id.map(SessionId::as_str))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(traces)
    }

    pub async fn get_trace(&self, turn_id: &TurnId) -> Result<TurnTrace, StorageError> {
        let mut summary = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT turns.id AS turn_id, turns.session_id,
                    turns.sequence AS turn_sequence, turns.status,
                    turns.resolved_model_name, turns.model_call_count,
                    turns.tool_call_count, COUNT(spans.id)::BIGINT AS span_count,
                    to_char(turns.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS started_at,
                    to_char(turns.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS ended_at
             FROM turns turns
             LEFT JOIN trace_spans spans ON spans.turn_id = turns.id
             WHERE turns.id = $1
             GROUP BY turns.id",
        )
        .bind(turn_id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::TurnNotFound(turn_id.to_string()))?;
        let spans = sqlx::query_as::<_, TraceSpanRecord>(
            "SELECT id, turn_id, parent_span_id, sequence, kind, name, status,
                    model_id, resolved_model_name, provider_request_id,
                    provider_call_id, requested_tool_name, resolved_tool_name,
                    attempt_count, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_tokens, total_tokens,
                    permission_wait_ms,
                    to_char(started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS started_at,
                    to_char(ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS ended_at,
                    error_code, error_message, attributes
             FROM trace_spans
             WHERE turn_id = $1
             ORDER BY sequence, id",
        )
        .bind(turn_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        summary.span_count = i64::try_from(spans.len()).unwrap_or(i64::MAX);
        let completeness = derive_trace_completeness(
            &summary.status,
            summary.model_call_count,
            summary.tool_call_count,
            &spans,
        );
        Ok(TurnTrace {
            summary,
            spans,
            completeness,
        })
    }
}

fn derive_trace_completeness(
    turn_status: &str,
    expected_model_calls: i32,
    expected_tool_calls: i32,
    spans: &[TraceSpanRecord],
) -> TraceCompleteness {
    use std::collections::HashSet;

    let expected_model_calls = u32::try_from(expected_model_calls.max(0)).unwrap_or(u32::MAX);
    let expected_tool_calls = u32::try_from(expected_tool_calls.max(0)).unwrap_or(u32::MAX);
    let model_ids = spans
        .iter()
        .filter(|span| span.kind == "model_call")
        .map(|span| span.id.as_str())
        .collect::<HashSet<_>>();
    let captured_model_calls = saturating_u32(
        spans
            .iter()
            .filter(|span| span.kind == "model_call")
            .count(),
    );
    let captured_tool_calls =
        saturating_u32(spans.iter().filter(|span| span.kind == "tool_call").count());
    let orphan_tool_spans = saturating_u32(
        spans
            .iter()
            .filter(|span| {
                span.kind == "tool_call"
                    && span
                        .parent_span_id
                        .as_deref()
                        .is_none_or(|parent| !model_ids.contains(parent))
            })
            .count(),
    );
    let running_spans =
        saturating_u32(spans.iter().filter(|span| span.status == "running").count());
    let outcome_unknown_spans = saturating_u32(
        spans
            .iter()
            .filter(|span| span.status == "outcome_unknown")
            .count(),
    );
    let expected_total = expected_model_calls.saturating_add(expected_tool_calls);
    let captured_total = captured_model_calls.saturating_add(captured_tool_calls);
    let state = if expected_total > 0 && captured_total == 0 {
        TraceCompletenessState::None
    } else if turn_status != "running"
        && captured_model_calls == expected_model_calls
        && captured_tool_calls == expected_tool_calls
        && orphan_tool_spans == 0
        && running_spans == 0
        && outcome_unknown_spans == 0
    {
        TraceCompletenessState::Complete
    } else {
        TraceCompletenessState::Partial
    };
    TraceCompleteness {
        expected_model_calls,
        captured_model_calls,
        expected_tool_calls,
        captured_tool_calls,
        orphan_tool_spans,
        running_spans,
        outcome_unknown_spans,
        state,
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

const SESSION_COLUMNS: &str = "SELECT id, title, working_directory, default_model_id, status,
            to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at,
            to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS updated_at,
            to_char(last_turn_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_turn_at
     FROM sessions";

#[async_trait]
impl SessionStorage for PostgresStorage {
    async fn begin_turn(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        client_request_id: &ClientRequestId,
        model: &ResolvedModel,
        user_message: &Message,
    ) -> Result<(), String> {
        self.begin_turn_inner(session_id, turn_id, client_request_id, model, user_message)
            .await
            .map_err(|error| error.to_string())
    }

    async fn begin_model_call(
        &self,
        turn_id: &TurnId,
        model_call_index: u32,
    ) -> Result<(), String> {
        let result = sqlx::query(
            "UPDATE turns
             SET model_call_count = GREATEST(model_call_count, $2),
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
             WHERE id = $1 AND status = 'running'",
        )
        .bind(turn_id.as_str())
        .bind(i32::try_from(model_call_index).map_err(|_| "model call index overflow")?)
        .execute(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        if result.rows_affected() == 0 {
            return Err(StorageError::TurnNotFound(turn_id.to_string()).to_string());
        }
        Ok(())
    }

    async fn append_assistant_message(
        &self,
        turn_id: &TurnId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<(), String> {
        self.append_assistant_inner(turn_id, message, usage)
            .await
            .map_err(|error| error.to_string())
    }

    async fn append_tool_result(&self, turn_id: &TurnId, message: &Message) -> Result<(), String> {
        self.append_tool_result_inner(turn_id, message)
            .await
            .map_err(|error| error.to_string())
    }

    async fn finish_turn(&self, turn_id: &TurnId, outcome: &TurnOutcome) -> Result<(), String> {
        self.finish_turn_inner(turn_id, outcome)
            .await
            .map_err(|error| error.to_string())
    }
}

impl PostgresStorage {
    async fn begin_turn_inner(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        client_request_id: &ClientRequestId,
        model: &ResolvedModel,
        user_message: &Message,
    ) -> Result<(), StorageError> {
        if user_message.role != Role::User {
            return Err(StorageError::InvalidInput(
                "begin_turn requires a user message".to_string(),
            ));
        }
        validate_resolved_model(model)?;
        let content = serde_json::to_value(&user_message.content)?;
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        let turn_sequence = next_turn_sequence(&mut transaction, session_id).await?;
        sqlx::query(
            "INSERT INTO turns (
                 id, session_id, client_request_id, sequence, model_id,
                 resolved_provider_kind, resolved_model_name, status
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'running')",
        )
        .bind(turn_id.as_str())
        .bind(session_id.as_str())
        .bind(client_request_id.as_str())
        .bind(turn_sequence)
        .bind(&model.model_id)
        .bind(&model.provider_kind)
        .bind(&model.model_name)
        .execute(&mut *transaction)
        .await?;
        insert_message(
            &mut transaction,
            session_id,
            Some(turn_id),
            Role::User,
            content,
            None,
            None,
        )
        .await?;
        sqlx::query(
            "UPDATE sessions
             SET updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC',
                 last_turn_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
             WHERE id = $1",
        )
        .bind(session_id.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn append_assistant_inner(
        &self,
        turn_id: &TurnId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<(), StorageError> {
        if message.role != Role::Assistant {
            return Err(StorageError::InvalidInput(
                "append_assistant_message requires an assistant message".to_string(),
            ));
        }
        let content = serde_json::to_value(&message.content)?;
        let tool_calls = message
            .content
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolCall(_)))
            .count();
        let tool_calls = i32::try_from(tool_calls)
            .map_err(|_| StorageError::InvalidInput("tool call count overflow".to_string()))?;
        let input_tokens = optional_token(usage.and_then(|value| value.input_tokens))?;
        let output_tokens = optional_token(usage.and_then(|value| value.output_tokens))?;
        let cached_input_tokens =
            optional_token(usage.and_then(|value| value.cached_input_tokens))?;
        let reasoning_tokens = optional_token(usage.and_then(|value| value.reasoning_tokens))?;

        let mut transaction = self.pool.begin().await?;
        let session_id = lock_turn(&mut transaction, turn_id).await?;
        insert_message(
            &mut transaction,
            &session_id,
            Some(turn_id),
            Role::Assistant,
            content,
            None,
            None,
        )
        .await?;
        sqlx::query(
            "UPDATE turns SET
                 tool_call_count = tool_call_count + $2,
                 input_tokens = CASE WHEN $3::BIGINT IS NULL THEN input_tokens
                     ELSE COALESCE(input_tokens, 0) + $3 END,
                 output_tokens = CASE WHEN $4::BIGINT IS NULL THEN output_tokens
                     ELSE COALESCE(output_tokens, 0) + $4 END,
                 cached_input_tokens = CASE WHEN $5::BIGINT IS NULL THEN cached_input_tokens
                     ELSE COALESCE(cached_input_tokens, 0) + $5 END,
                 reasoning_tokens = CASE WHEN $6::BIGINT IS NULL THEN reasoning_tokens
                     ELSE COALESCE(reasoning_tokens, 0) + $6 END,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
             WHERE id = $1",
        )
        .bind(turn_id.as_str())
        .bind(tool_calls)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cached_input_tokens)
        .bind(reasoning_tokens)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn append_tool_result_inner(
        &self,
        turn_id: &TurnId,
        message: &Message,
    ) -> Result<(), StorageError> {
        if message.role != Role::Tool {
            return Err(StorageError::InvalidInput(
                "append_tool_result requires a tool message".to_string(),
            ));
        }
        let mut tool_results = message.content.iter().filter_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result),
            _ => None,
        });
        let result = tool_results.next().ok_or_else(|| {
            StorageError::InvalidInput("tool message has no tool result block".to_string())
        })?;
        if tool_results.next().is_some() || message.content.len() != 1 {
            return Err(StorageError::InvalidInput(
                "tool message must contain exactly one tool result block".to_string(),
            ));
        }
        let content = serde_json::to_value(&message.content)?;
        let mut transaction = self.pool.begin().await?;
        let session_id = lock_turn(&mut transaction, turn_id).await?;
        insert_message(
            &mut transaction,
            &session_id,
            Some(turn_id),
            Role::Tool,
            content,
            Some(&result.id),
            Some(&result.name),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn finish_turn_inner(
        &self,
        turn_id: &TurnId,
        outcome: &TurnOutcome,
    ) -> Result<(), StorageError> {
        let (status, error_code, error_message) = match outcome {
            TurnOutcome::Completed { .. } => ("completed", None, None),
            TurnOutcome::Failed { code, message } => {
                ("failed", Some(code.as_str()), Some(message.as_str()))
            }
            TurnOutcome::Cancelled => ("cancelled", None, None),
        };
        let result = sqlx::query(
            "UPDATE turns
             SET status = $2, error_code = $3, error_message = $4,
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
             WHERE id = $1 AND status = 'running'",
        )
        .bind(turn_id.as_str())
        .bind(status)
        .bind(error_code)
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::TurnNotFound(turn_id.to_string()));
        }
        Ok(())
    }
}

async fn lock_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &SessionId,
) -> Result<(), StorageError> {
    let exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM sessions WHERE id = $1 FOR UPDATE")
            .bind(session_id.as_str())
            .fetch_optional(&mut **transaction)
            .await?;
    if exists.is_none() {
        return Err(StorageError::SessionNotFound(session_id.to_string()));
    }
    Ok(())
}

async fn lock_turn(
    transaction: &mut Transaction<'_, Postgres>,
    turn_id: &TurnId,
) -> Result<SessionId, StorageError> {
    let session_id: Option<String> =
        sqlx::query_scalar("SELECT session_id FROM turns WHERE id = $1 FOR UPDATE")
            .bind(turn_id.as_str())
            .fetch_optional(&mut **transaction)
            .await?;
    session_id
        .map(SessionId::new)
        .ok_or_else(|| StorageError::TurnNotFound(turn_id.to_string()))
}

async fn next_turn_sequence(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &SessionId,
) -> Result<i64, StorageError> {
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM turns WHERE session_id = $1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(sequence)
}

async fn insert_message(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &SessionId,
    turn_id: Option<&TurnId>,
    role: Role,
    content: Value,
    provider_call_id: Option<&str>,
    tool_name: Option<&str>,
) -> Result<(), StorageError> {
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE session_id = $1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    let message_id = format!("msg-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO messages (
             id, session_id, turn_id, sequence, role, content, provider_call_id, tool_name
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(message_id)
    .bind(session_id.as_str())
    .bind(turn_id.map(TurnId::as_str))
    .bind(sequence)
    .bind(role.as_provider_str())
    .bind(content)
    .bind(provider_call_id)
    .bind(tool_name)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_model(input: &ModelInput) -> Result<(), StorageError> {
    for (name, value) in [
        ("id", input.id.as_str()),
        ("display_name", input.display_name.as_str()),
        ("provider_kind", input.provider_kind.as_str()),
        ("model_name", input.model_name.as_str()),
        ("base_url", input.base_url.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(StorageError::InvalidInput(format!(
                "{name} must not be blank"
            )));
        }
    }
    if input
        .credential_ref
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(StorageError::InvalidInput(
            "credential_ref must not be blank".to_string(),
        ));
    }
    if !input.config.is_object() {
        return Err(StorageError::InvalidInput(
            "model config must be a JSON object".to_string(),
        ));
    }
    Ok(())
}

fn validate_session(input: &SessionInput) -> Result<(), StorageError> {
    if input.id.as_str().trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "session id must not be blank".to_string(),
        ));
    }
    if input.working_directory.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "working_directory must not be blank".to_string(),
        ));
    }
    if input
        .title
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(StorageError::InvalidInput(
            "session title must not be blank".to_string(),
        ));
    }
    Ok(())
}

fn validate_resolved_model(model: &ResolvedModel) -> Result<(), StorageError> {
    if model.provider_kind.trim().is_empty() || model.model_name.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "resolved provider kind and model name must not be blank".to_string(),
        ));
    }
    Ok(())
}

fn optional_token(value: Option<u64>) -> Result<Option<i64>, StorageError> {
    value
        .map(|value| {
            i64::try_from(value)
                .map_err(|_| StorageError::InvalidInput("token count overflow".to_string()))
        })
        .transpose()
}

fn parse_role(value: &str) -> Result<Role, StorageError> {
    match value {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        other => Err(StorageError::InvalidInput(format!(
            "unknown stored message role: {other}"
        ))),
    }
}

#[cfg(test)]
mod trace_completeness_tests {
    use serde_json::json;

    use super::*;

    fn span(id: &str, kind: &str, parent_span_id: Option<&str>, status: &str) -> TraceSpanRecord {
        TraceSpanRecord {
            id: id.to_string(),
            turn_id: "turn-1".to_string(),
            parent_span_id: parent_span_id.map(str::to_string),
            sequence: 1,
            kind: kind.to_string(),
            name: format!("{kind}.call"),
            status: status.to_string(),
            model_id: None,
            resolved_model_name: None,
            provider_request_id: None,
            provider_call_id: None,
            requested_tool_name: None,
            resolved_tool_name: None,
            attempt_count: None,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            permission_wait_ms: None,
            started_at: "2026-07-19T00:00:00.000000Z".to_string(),
            ended_at: Some("2026-07-19T00:00:01.000000Z".to_string()),
            error_code: None,
            error_message: None,
            attributes: json!({}),
        }
    }

    #[test]
    fn derives_complete_partial_and_none_without_persisting_another_status() {
        let complete_spans = vec![
            span("model-1", "model_call", None, "succeeded"),
            span("tool-1", "tool_call", Some("model-1"), "succeeded"),
        ];
        let complete = derive_trace_completeness("completed", 1, 1, &complete_spans);
        assert_eq!(complete.state, TraceCompletenessState::Complete);
        assert_eq!(complete.captured_model_calls, 1);
        assert_eq!(complete.captured_tool_calls, 1);

        let none = derive_trace_completeness("failed", 1, 1, &[]);
        assert_eq!(none.state, TraceCompletenessState::None);

        let orphan = vec![
            span("model-1", "model_call", None, "succeeded"),
            span("tool-1", "tool_call", Some("missing"), "succeeded"),
        ];
        let partial = derive_trace_completeness("completed", 1, 1, &orphan);
        assert_eq!(partial.state, TraceCompletenessState::Partial);
        assert_eq!(partial.orphan_tool_spans, 1);

        let running = derive_trace_completeness("running", 1, 1, &complete_spans);
        assert_eq!(running.state, TraceCompletenessState::Partial);
    }
}
