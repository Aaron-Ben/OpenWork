use std::time::Duration;

use async_trait::async_trait;
use openwork_chat_state::ConversationItem;
use openwork_models::model::{ContentBlock, Message, Role, TokenUsage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Acquire, Executor, PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

use crate::session::{
    ClientRequestId, CompactionRuntimeState, ConversationCompaction, ConversationCompactionKind,
    ConversationProjectionSelector, NewConversationCompaction, ResolvedModel, SessionId,
    SessionStorage, TracePayloadSlot, TurnId, TurnOutcome,
};
use crate::session::{
    compacted_items, compaction_summary_message, validate_summary_text, validate_system_reminder,
};

use super::TRACE_PAYLOAD_ADVISORY_LOCK;

const DEFAULT_DATABASE_URL: &str = "postgres://openwork:openwork@localhost:5432/openwork";

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

fn is_payload_reference_conflict(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(error) = error else {
        return false;
    };
    error.code().as_deref() == Some("23503")
        && error.constraint() == Some("trace_span_payloads_payload_hash_fkey")
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationProjectionRecord {
    pub selector: ConversationProjectionSelector,
    pub checkpoint_id: Option<String>,
    pub through_message_sequence: i64,
    pub messages: Vec<StoredMessageRecord>,
}

pub const DEFAULT_COMPACTION_TRANSCRIPT_PAGE_LIMIT: u32 = 20;
pub const MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT: u32 = 50;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTranscriptQuery {
    pub compaction_id: Option<String>,
    pub after_sequence: Option<i64>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTranscriptPage {
    pub session_id: String,
    pub compaction_id: String,
    pub through_message_sequence: i64,
    pub messages: Vec<StoredMessageRecord>,
    pub next_after_sequence: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct ConversationCompactionRow {
    id: String,
    session_id: String,
    sequence: i64,
    through_message_sequence: i64,
    replaced_through_message_sequence: i64,
    source_message_count: i32,
    checkpoint_format_version: i16,
    kind: String,
    summary_format_version: i16,
    last_user_message_id: Option<String>,
    last_user_message_sequence: Option<i64>,
    resolved_model_name: String,
    summary: String,
    runtime_state: Value,
    runtime_reminder_format_version: i16,
    runtime_reminder: String,
    trigger_turn_id: Option<String>,
    parent_compaction_id: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
/// 运行记录列表的一行，键是 `trace_id` 而不是 Turn。
///
/// 大多数 Trace 由一个 Turn 支撑，此时 Turn 字段齐全。手动压缩与 rewind 没有 Turn，
/// 它们各自是一条独立 Trace，`turn_id` / `turn_sequence` 为空、调用计数为 0——
/// 这是事实陈述，不是缺数据。若以 Turn 为列表根，这类 Trace 不可能出现。
pub struct TraceTurnSummary {
    pub trace_id: String,
    pub turn_id: Option<String>,
    pub session_id: String,
    pub turn_sequence: Option<i64>,
    pub status: String,
    pub resolved_model_name: String,
    pub model_call_count: i32,
    pub model_submission_count: i32,
    pub tool_call_count: i32,
    pub span_count: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpanRecord {
    pub id: String,
    pub trace_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub parent_span_id: Option<String>,
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
    pub response_message_id: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpanPayloadRecord {
    pub span_id: String,
    pub slot: String,
    pub body: Value,
    pub byte_size: i64,
    pub truncated: bool,
    pub original_byte_size: Option<i64>,
    pub redacted_count: i32,
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
    #[error("trace not found: {0}")]
    TraceNotFound(String),
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
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'",
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
                 SET title = $2, updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
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
        let mut transaction = self.pool.begin().await?;
        lock_trace_payload_mutations(&mut transaction).await?;
        // Block both new Span inserts (through the Session FK) and late finish
        // signals that could attach a payload to an already-created Span. The
        // candidate query must run only after in-flight Trace writes commit;
        // otherwise the Session cascade could leave their payload body orphaned.
        lock_session(&mut transaction, session_id).await?;
        let _: Vec<String> =
            sqlx::query_scalar("SELECT id FROM trace_spans WHERE session_id = $1 FOR UPDATE")
                .bind(session_id.as_str())
                .fetch_all(&mut *transaction)
                .await?;
        let payload_hashes = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT mapping.payload_hash
             FROM trace_span_payloads AS mapping
             JOIN trace_spans AS span ON span.id = mapping.span_id
             WHERE span.session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id.as_str())
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::SessionNotFound(session_id.to_string()));
        }
        cleanup_trace_payload_candidates(&mut transaction, payload_hashes).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Remove payload mappings older than the configured retention window.
    ///
    /// Age is measured from the owning Span, not `trace_payloads.created_at`:
    /// payload bodies are deduplicated, so a freshly written Span may point at
    /// a body row first created months earlier.
    pub async fn purge_expired_trace_payloads(
        &self,
        retention_days: u32,
    ) -> Result<usize, StorageError> {
        let retention_days = i32::try_from(retention_days).map_err(|_| {
            StorageError::InvalidInput(
                "trace payload retention must be between 1 and 2147483647 days".to_string(),
            )
        })?;
        if retention_days == 0 {
            return Err(StorageError::InvalidInput(
                "trace payload retention must be between 1 and 2147483647 days".to_string(),
            ));
        }

        let mut transaction = self.pool.begin().await?;
        lock_trace_payload_mutations(&mut transaction).await?;
        let payload_hashes = sqlx::query_scalar::<_, String>(
            "DELETE FROM trace_span_payloads AS mapping
             USING trace_spans AS span
             WHERE span.id = mapping.span_id
               AND span.started_at <
                   (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                   - make_interval(days => $1)
               AND NOT EXISTS (
                   SELECT 1
                   FROM trace_annotations AS annotation
                   WHERE annotation.session_id = span.session_id
                     AND annotation.trace_id = span.trace_id
               )
             RETURNING mapping.payload_hash",
        )
        .bind(retention_days)
        .fetch_all(&mut *transaction)
        .await?;
        let deleted_mapping_count = payload_hashes.len();
        cleanup_trace_payload_candidates(&mut transaction, payload_hashes).await?;
        transaction.commit().await?;
        Ok(deleted_mapping_count)
    }

    pub async fn load_messages(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<Message>, StorageError> {
        Ok(self
            .load_conversation_records(session_id)
            .await?
            .into_iter()
            .map(|record| Message {
                role: record.role,
                content: record.content,
            })
            .collect())
    }

    pub async fn load_conversation_items(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ConversationItem>, StorageError> {
        let compaction = self.load_latest_conversation_compaction(session_id).await?;
        let replaced_through = compaction
            .as_ref()
            .map_or(0, |record| record.replaced_through_message_sequence);
        let rows = self
            .load_message_records_between(session_id, replaced_through, None)
            .await?;
        let mut items = Vec::with_capacity(rows.len() + usize::from(compaction.is_some()) * 3);
        if let Some(compaction) = &compaction {
            let last_user = self.load_compaction_last_user(compaction).await?;
            items.extend(
                compacted_items(compaction, last_user)
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            );
        }
        items.extend(rows.into_iter().map(|record| {
            ConversationItem::persisted(
                record.id,
                record.sequence,
                Message {
                    role: record.role,
                    content: record.content,
                },
            )
        }));
        Ok(items)
    }

    /// Loads the current model-visible Conversation projection. The raw
    /// transcript remains in `messages`; the latest compaction summary replaces
    /// only the prefix through its recorded message sequence.
    pub async fn load_conversation_records(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let compaction = self.load_latest_conversation_compaction(session_id).await?;
        let through_sequence = compaction
            .as_ref()
            .map_or(0, |record| record.replaced_through_message_sequence);
        let rows = self
            .load_message_records_between(session_id, through_sequence, None)
            .await?;
        let mut records = Vec::with_capacity(rows.len() + usize::from(compaction.is_some()) * 3);
        if let Some(compaction) = &compaction {
            let last_user = self.load_compaction_last_user_record(compaction).await?;
            records.push(StoredMessageRecord {
                id: format!("{}:last-user", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: last_user.role,
                content: last_user.content,
                created_at: compaction.created_at.clone(),
            });
            records.push(StoredMessageRecord {
                id: format!("{}:summary", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: compaction_summary_message(&compaction.summary).content,
                created_at: compaction.created_at.clone(),
            });
            records.push(StoredMessageRecord {
                id: format!("{}:reminder", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: Message::text(Role::User, &compaction.runtime_reminder).content,
                created_at: compaction.created_at.clone(),
            });
        }
        records.extend(rows);
        Ok(records)
    }

    pub async fn replay_conversation(
        &self,
        session_id: &SessionId,
        selector: ConversationProjectionSelector,
    ) -> Result<ConversationProjectionRecord, StorageError> {
        match &selector {
            ConversationProjectionSelector::Latest => {
                let checkpoint = self.load_latest_conversation_compaction(session_id).await?;
                let through_message_sequence: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = $1",
                )
                .bind(session_id.as_str())
                .fetch_one(&self.pool)
                .await?;
                Ok(ConversationProjectionRecord {
                    selector: selector.clone(),
                    checkpoint_id: checkpoint.map(|checkpoint| checkpoint.id),
                    through_message_sequence,
                    messages: self.load_conversation_records(session_id).await?,
                })
            }
            ConversationProjectionSelector::Compaction { compaction_id } => {
                let checkpoint = self
                    .load_conversation_compaction(session_id, compaction_id)
                    .await?;
                Ok(ConversationProjectionRecord {
                    selector: selector.clone(),
                    checkpoint_id: Some(checkpoint.id.clone()),
                    through_message_sequence: checkpoint.through_message_sequence,
                    messages: self.compaction_projection_records(&checkpoint).await?,
                })
            }
            ConversationProjectionSelector::ThroughMessage { sequence } => {
                if *sequence < 0 {
                    return Err(StorageError::InvalidInput(
                        "conversation replay sequence must be non-negative".to_string(),
                    ));
                }
                let checkpoint = self
                    .load_latest_compaction_through(session_id, *sequence)
                    .await?;
                let mut messages = match &checkpoint {
                    Some(checkpoint) => self.compaction_projection_records(checkpoint).await?,
                    None => Vec::new(),
                };
                let after_sequence = checkpoint
                    .as_ref()
                    .map_or(0, |checkpoint| checkpoint.replaced_through_message_sequence);
                messages.extend(
                    self.load_message_records_between(session_id, after_sequence, Some(*sequence))
                        .await?,
                );
                Ok(ConversationProjectionRecord {
                    selector: selector.clone(),
                    checkpoint_id: checkpoint.map(|checkpoint| checkpoint.id),
                    through_message_sequence: *sequence,
                    messages,
                })
            }
        }
    }

    pub async fn read_compaction_transcript(
        &self,
        session_id: &SessionId,
        query: ConversationTranscriptQuery,
    ) -> Result<ConversationTranscriptPage, StorageError> {
        let after_sequence = query.after_sequence.unwrap_or(0);
        if after_sequence < 0 {
            return Err(StorageError::InvalidInput(
                "compaction transcript afterSequence must be non-negative".to_string(),
            ));
        }
        let limit = query
            .limit
            .unwrap_or(DEFAULT_COMPACTION_TRANSCRIPT_PAGE_LIMIT);
        if !(1..=MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT).contains(&limit) {
            return Err(StorageError::InvalidInput(format!(
                "compaction transcript limit must be between 1 and {MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT}"
            )));
        }

        let checkpoint = match query.compaction_id.as_deref() {
            Some(compaction_id) => {
                self.load_conversation_compaction(session_id, compaction_id)
                    .await?
            }
            None => self
                .load_latest_conversation_compaction(session_id)
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "session has no conversation compaction to read back".to_string(),
                    )
                })?,
        };
        let fetch_limit = i64::from(limit) + 1;
        let rows: Vec<(String, Option<String>, i64, String, Value, String)> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1
               AND sequence > $2
               AND sequence <= $3
             ORDER BY sequence
             LIMIT $4",
        )
        .bind(session_id.as_str())
        .bind(after_sequence)
        .bind(checkpoint.through_message_sequence)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await?;
        let has_more = rows.len() > limit as usize;
        let messages = rows
            .into_iter()
            .take(limit as usize)
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
            .collect::<Result<Vec<_>, StorageError>>()?;
        let next_after_sequence = has_more.then(|| {
            messages
                .last()
                .expect("a page with more rows has at least one returned message")
                .sequence
        });
        Ok(ConversationTranscriptPage {
            session_id: session_id.to_string(),
            compaction_id: checkpoint.id,
            through_message_sequence: checkpoint.through_message_sequence,
            messages,
            next_after_sequence,
            has_more,
        })
    }

    async fn compaction_projection_records(
        &self,
        compaction: &ConversationCompaction,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let last_user = self.load_compaction_last_user_record(compaction).await?;
        Ok(vec![
            StoredMessageRecord {
                id: format!("{}:last-user", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: last_user.role,
                content: last_user.content,
                created_at: compaction.created_at.clone(),
            },
            StoredMessageRecord {
                id: format!("{}:summary", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: compaction_summary_message(&compaction.summary).content,
                created_at: compaction.created_at.clone(),
            },
            StoredMessageRecord {
                id: format!("{}:reminder", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: Message::text(Role::User, &compaction.runtime_reminder).content,
                created_at: compaction.created_at.clone(),
            },
        ])
    }

    async fn load_compaction_last_user(
        &self,
        compaction: &ConversationCompaction,
    ) -> Result<Message, StorageError> {
        let record = self.load_compaction_last_user_record(compaction).await?;
        Ok(Message {
            role: record.role,
            content: record.content,
        })
    }

    async fn load_compaction_last_user_record(
        &self,
        compaction: &ConversationCompaction,
    ) -> Result<StoredMessageRecord, StorageError> {
        let message_id = compaction.last_user_message_id.as_deref().ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "compaction {} has no last user message",
                compaction.id
            ))
        })?;
        let row: Option<(String, Option<String>, i64, String, Value, String)> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1 AND id = $2 AND role = 'user'",
        )
        .bind(compaction.session_id.as_str())
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        let (id, turn_id, sequence, role, content, created_at) = row.ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "compaction {} last user message was not found",
                compaction.id
            ))
        })?;
        Ok(StoredMessageRecord {
            id,
            turn_id,
            sequence,
            role: parse_role(&role)?,
            content: serde_json::from_value(content)?,
            created_at,
        })
    }

    async fn load_message_records_between(
        &self,
        session_id: &SessionId,
        after_sequence: i64,
        through_sequence: Option<i64>,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let rows: Vec<(String, Option<String>, i64, String, Value, String)> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1
               AND sequence > $2
               AND ($3::BIGINT IS NULL OR sequence <= $3)
             ORDER BY sequence",
        )
        .bind(session_id.as_str())
        .bind(after_sequence)
        .bind(through_sequence)
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

    pub async fn load_message_records(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let rows: Vec<(String, Option<String>, i64, String, Value, String)> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
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

    pub async fn load_latest_conversation_compaction(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ConversationCompaction>, StorageError> {
        let row: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1
             ORDER BY sequence DESC
             LIMIT 1",
        )
        .bind(session_id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(conversation_compaction_from_row).transpose()
    }

    pub async fn list_conversation_compactions(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ConversationCompaction>, StorageError> {
        let rows: Vec<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1
             ORDER BY sequence DESC",
        )
        .bind(session_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(conversation_compaction_from_row)
            .collect()
    }

    async fn load_latest_compaction_through(
        &self,
        session_id: &SessionId,
        through_message_sequence: i64,
    ) -> Result<Option<ConversationCompaction>, StorageError> {
        let row: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1 AND replaced_through_message_sequence <= $2
             ORDER BY replaced_through_message_sequence DESC, sequence DESC
             LIMIT 1",
        )
        .bind(session_id.as_str())
        .bind(through_message_sequence)
        .fetch_optional(&self.pool)
        .await?;
        row.map(conversation_compaction_from_row).transpose()
    }

    pub async fn load_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<ConversationCompaction, StorageError> {
        let row: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1 AND id = $2",
        )
        .bind(session_id.as_str())
        .bind(compaction_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(conversation_compaction_from_row)
            .transpose()?
            .ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "conversation compaction not found: {compaction_id}"
                ))
            })
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
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 error_code = COALESCE(error_code, 'process_restart'),
                 error_message = COALESCE(error_message, 'process restarted before span completed')
             WHERE status = 'running'",
        )
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query(
            "UPDATE turns
             SET status = 'interrupted',
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
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
        // 两路来源：有 Turn 支撑的 Trace，以及无 Turn 的独立 Trace（手动压缩、rewind）。
        // 后者没有 Turn 行可以 JOIN，因此必须单独一路——只从 turns 出发会让它们永远不可见。
        let traces = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT turns.id AS trace_id, turns.id AS turn_id, turns.session_id,
                    turns.sequence AS turn_sequence, turns.status,
                    turns.resolved_model_name, turns.model_call_count,
                    turns.model_submission_count,
                    turns.tool_call_count, COUNT(spans.id)::BIGINT AS span_count,
                    to_char(turns.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(turns.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    turns.started_at AS sort_key
             FROM turns turns
             LEFT JOIN trace_spans spans ON spans.trace_id = turns.id
             WHERE ($1::TEXT IS NULL OR turns.session_id = $1)
             GROUP BY turns.id

             UNION ALL

             SELECT root.trace_id, NULL AS turn_id, root.session_id,
                    NULL AS turn_sequence,
                    -- Span 与 Turn 的状态词汇不同，映射到 Turn 的取值，
                    -- 让列表只有一套状态语言（前端过滤器与徽章依赖这一点）。
                    CASE root.status
                        WHEN 'succeeded' THEN 'completed'
                        WHEN 'outcome_unknown' THEN 'interrupted'
                        ELSE root.status
                    END AS status,
                    COALESCE(root.resolved_model_name, '') AS resolved_model_name,
                    0 AS model_call_count, 0 AS model_submission_count,
                    0 AS tool_call_count,
                    (SELECT COUNT(*)::BIGINT FROM trace_spans member
                     WHERE member.trace_id = root.trace_id) AS span_count,
                    to_char(root.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(root.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    root.started_at AS sort_key
             FROM trace_spans root
             WHERE root.turn_id IS NULL
               AND root.parent_span_id IS NULL
               AND ($1::TEXT IS NULL OR root.session_id = $1)

             ORDER BY sort_key DESC, trace_id
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
            "SELECT turns.id AS trace_id, turns.id AS turn_id, turns.session_id,
                    turns.sequence AS turn_sequence, turns.status,
                    turns.resolved_model_name, turns.model_call_count,
                    turns.model_submission_count,
                    turns.tool_call_count, COUNT(spans.id)::BIGINT AS span_count,
                    to_char(turns.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(turns.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at
             FROM turns turns
             LEFT JOIN trace_spans spans ON spans.trace_id = turns.id
             WHERE turns.id = $1
             GROUP BY turns.id",
        )
        .bind(turn_id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::TurnNotFound(turn_id.to_string()))?;
        let spans = sqlx::query_as::<_, TraceSpanRecord>(
            "SELECT id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
                    model_id, resolved_model_name, provider_request_id,
                    provider_call_id, requested_tool_name, resolved_tool_name,
                    attempt_count, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_tokens, (input_tokens + output_tokens) AS total_tokens,
                    response_message_id, permission_wait_ms,
                    to_char(started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    error_code, error_message, attributes
             FROM trace_spans
             WHERE trace_id = $1
             ORDER BY started_at, id",
        )
        .bind(turn_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        summary.span_count = i64::try_from(spans.len()).unwrap_or(i64::MAX);
        let completeness = derive_trace_completeness(
            &summary.status,
            summary.model_submission_count,
            summary.tool_call_count,
            &spans,
        );
        Ok(TurnTrace {
            summary,
            spans,
            completeness,
        })
    }

    pub async fn get_trace_by_id(&self, trace_id: &str) -> Result<TurnTrace, StorageError> {
        if trace_id.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "trace id must not be blank".to_string(),
            ));
        }
        let turn_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM turns WHERE id = $1
             UNION ALL
             SELECT turn_id FROM trace_spans
             WHERE trace_id = $1 AND turn_id IS NOT NULL
             LIMIT 1",
        )
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(turn_id) = turn_id {
            return self.get_trace(&TurnId::new(turn_id)).await;
        }

        let mut summary = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT root.trace_id, NULL AS turn_id, root.session_id,
                    NULL AS turn_sequence,
                    CASE root.status
                        WHEN 'succeeded' THEN 'completed'
                        WHEN 'outcome_unknown' THEN 'interrupted'
                        ELSE root.status
                    END AS status,
                    COALESCE(root.resolved_model_name, '') AS resolved_model_name,
                    0 AS model_call_count, 0 AS model_submission_count,
                    0 AS tool_call_count,
                    (SELECT COUNT(*)::BIGINT FROM trace_spans member
                     WHERE member.trace_id = root.trace_id) AS span_count,
                    to_char(root.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(root.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at
             FROM trace_spans root
             WHERE root.trace_id = $1
               AND root.turn_id IS NULL
               AND root.parent_span_id IS NULL
             ORDER BY root.started_at, root.id
             LIMIT 1",
        )
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::TraceNotFound(trace_id.to_string()))?;
        let spans = self.load_trace_spans(trace_id).await?;
        summary.span_count = i64::try_from(spans.len()).unwrap_or(i64::MAX);
        let completeness = derive_trace_completeness(
            &summary.status,
            summary.model_submission_count,
            summary.tool_call_count,
            &spans,
        );
        Ok(TurnTrace {
            summary,
            spans,
            completeness,
        })
    }

    pub async fn get_span_payload(
        &self,
        span_id: &str,
        slot: TracePayloadSlot,
    ) -> Result<Option<TraceSpanPayloadRecord>, StorageError> {
        if span_id.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "span id must not be blank".to_string(),
            ));
        }
        Ok(sqlx::query_as::<_, TraceSpanPayloadRecord>(
            "SELECT mapping.span_id, mapping.slot, payload.body, payload.byte_size,
                    mapping.truncated, mapping.original_byte_size,
                    mapping.redacted_count
             FROM trace_span_payloads mapping
             JOIN trace_payloads payload ON payload.hash = mapping.payload_hash
             WHERE mapping.span_id = $1 AND mapping.slot = $2",
        )
        .bind(span_id)
        .bind(slot.as_str())
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn load_trace_spans(&self, trace_id: &str) -> Result<Vec<TraceSpanRecord>, StorageError> {
        Ok(sqlx::query_as::<_, TraceSpanRecord>(
            "SELECT id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
                    model_id, resolved_model_name, provider_request_id,
                    provider_call_id, requested_tool_name, resolved_tool_name,
                    attempt_count, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_tokens, (input_tokens + output_tokens) AS total_tokens,
                    response_message_id, permission_wait_ms,
                    to_char(started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    error_code, error_message, attributes
             FROM trace_spans
             WHERE trace_id = $1
             ORDER BY started_at, id",
        )
        .bind(trace_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Every Compaction Span in a Session, newest first.
    ///
    /// A manual compaction has no Turn, so its Span is unreachable through
    /// [`Self::get_trace`]. This is the read path for the Session scope the
    /// `compaction` kind was introduced with; it also returns the Turn-scoped
    /// threshold/overflow compactions so a Session's compaction history reads
    /// as one list.
    pub async fn list_compaction_spans(
        &self,
        session_id: &SessionId,
        limit: i64,
    ) -> Result<Vec<TraceSpanRecord>, StorageError> {
        let spans = sqlx::query_as::<_, TraceSpanRecord>(
            "SELECT id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
                    model_id, resolved_model_name, provider_request_id,
                    provider_call_id, requested_tool_name, resolved_tool_name,
                    attempt_count, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_tokens, (input_tokens + output_tokens) AS total_tokens,
                    response_message_id, permission_wait_ms,
                    to_char(started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    error_code, error_message, attributes
             FROM trace_spans
             WHERE session_id = $1 AND kind = 'compaction'
             ORDER BY started_at DESC, id
             LIMIT $2",
        )
        .bind(session_id.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(spans)
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
            .filter(|span| span.kind == "model_call" && span.parent_span_id.is_none())
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
            to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at,
            to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS updated_at,
            to_char(last_turn_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS last_turn_at
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
        submission_attempt: u8,
    ) -> Result<(), String> {
        if model_call_index == 0 {
            return Err("model call index must be positive".to_string());
        }
        if submission_attempt == 0 {
            return Err("model submission attempt must be positive".to_string());
        }
        let result = sqlx::query(
            "UPDATE turns
             SET model_call_count = GREATEST(model_call_count, $2),
                 model_submission_count = model_submission_count + 1,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
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
    ) -> Result<String, String> {
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

    async fn save_conversation_compaction(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, String> {
        self.save_conversation_compaction_inner(session_id, input)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load_compaction_source_messages(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<Message>, String> {
        self.load_message_records(session_id)
            .await
            .map(|records| {
                records
                    .into_iter()
                    .map(|record| Message {
                        role: record.role,
                        content: record.content,
                    })
                    .collect()
            })
            .map_err(|error| error.to_string())
    }

    async fn read_compaction_transcript(
        &self,
        session_id: &SessionId,
        query: ConversationTranscriptQuery,
    ) -> Result<ConversationTranscriptPage, String> {
        PostgresStorage::read_compaction_transcript(self, session_id, query)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load_latest_compaction_runtime_state(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<CompactionRuntimeState>, String> {
        self.load_latest_conversation_compaction(session_id)
            .await
            .map(|checkpoint| checkpoint.map(|checkpoint| checkpoint.runtime_state))
            .map_err(|error| error.to_string())
    }

    async fn rewind_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
        runtime_state: CompactionRuntimeState,
        runtime_reminder: String,
    ) -> Result<ConversationCompaction, String> {
        self.rewind_conversation_compaction_inner(
            session_id,
            compaction_id,
            runtime_state,
            runtime_reminder,
        )
        .await
        .map_err(|error| error.to_string())
    }

    async fn load_compaction_last_user_message(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<Message, String> {
        let compaction = self
            .load_conversation_compaction(session_id, compaction_id)
            .await
            .map_err(|error| error.to_string())?;
        self.load_compaction_last_user(&compaction)
            .await
            .map_err(|error| error.to_string())
    }

    async fn delete_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<(), String> {
        self.delete_conversation_compaction_inner(session_id, compaction_id)
            .await
            .map_err(|error| error.to_string())
    }
}

impl PostgresStorage {
    async fn save_conversation_compaction_inner(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, StorageError> {
        if input.source_message_count == 0 {
            return Err(StorageError::InvalidInput(
                "compaction source message count must be positive".to_string(),
            ));
        }
        if input.resolved_model_name.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "compaction model name must not be blank".to_string(),
            ));
        }
        let summary = validate_summary_text(&input.summary)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        validate_runtime_checkpoint_state(&input.runtime_state, &input.runtime_reminder)?;
        if input.kind == ConversationCompactionKind::Rewind {
            return Err(StorageError::InvalidInput(
                "rewind checkpoints must be created through the rewind operation".to_string(),
            ));
        }
        let trigger_turn_id = input.trigger_turn_id.as_ref().map(TurnId::as_str);
        let requires_trigger_turn = matches!(
            input.kind,
            ConversationCompactionKind::Threshold | ConversationCompactionKind::Overflow
        );
        if requires_trigger_turn != trigger_turn_id.is_some() {
            return Err(StorageError::InvalidInput(
                "threshold and overflow compactions require exactly one trigger turn".to_string(),
            ));
        }
        if input.last_user_message_id.is_some() != input.last_user_message_sequence.is_some() {
            return Err(StorageError::InvalidInput(
                "last user message id and sequence must be provided together".to_string(),
            ));
        }
        let source_message_count = i32::try_from(input.source_message_count).map_err(|_| {
            StorageError::InvalidInput("compaction source message count overflow".to_string())
        })?;
        let input_tokens = optional_token(input.input_tokens)?;
        let output_tokens = optional_token(input.output_tokens)?;
        let runtime_state = serde_json::to_value(&input.runtime_state)?;

        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        match input.kind {
            ConversationCompactionKind::Manual => {
                let running_turn: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1 FROM turns WHERE session_id = $1 AND status = 'running'
                     )",
                )
                .bind(session_id.as_str())
                .fetch_one(&mut *transaction)
                .await?;
                if running_turn {
                    return Err(StorageError::InvalidInput(format!(
                        "session has an active turn and cannot be compacted: {session_id}"
                    )));
                }
            }
            ConversationCompactionKind::Threshold | ConversationCompactionKind::Overflow => {
                let active_trigger: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1 FROM turns
                         WHERE id = $1 AND session_id = $2 AND status = 'running'
                     )",
                )
                .bind(trigger_turn_id)
                .bind(session_id.as_str())
                .fetch_one(&mut *transaction)
                .await?;
                if !active_trigger {
                    return Err(StorageError::InvalidInput(
                        "threshold or overflow compaction trigger turn is not active in this session"
                            .to_string(),
                    ));
                }
            }
            ConversationCompactionKind::Rewind => unreachable!("validated above"),
        }
        let through_message_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if through_message_sequence == 0 {
            return Err(StorageError::InvalidInput(
                "conversation is empty and cannot be compacted".to_string(),
            ));
        }
        let (last_user_message_id, last_user_message_sequence) =
            match (input.last_user_message_id, input.last_user_message_sequence) {
                (Some(message_id), Some(message_sequence)) => {
                    let valid: bool = sqlx::query_scalar(
                        "SELECT EXISTS(
                             SELECT 1 FROM messages
                             WHERE id = $1 AND session_id = $2 AND sequence = $3
                               AND role = 'user' AND sequence <= $4
                         )",
                    )
                    .bind(&message_id)
                    .bind(session_id.as_str())
                    .bind(message_sequence)
                    .bind(through_message_sequence)
                    .fetch_one(&mut *transaction)
                    .await?;
                    if !valid {
                        return Err(StorageError::InvalidInput(
                            "compaction last user message does not belong to the source boundary"
                                .to_string(),
                        ));
                    }
                    (message_id, message_sequence)
                }
                (None, None) => sqlx::query_as::<_, (String, i64)>(
                    "SELECT id, sequence FROM messages
                         WHERE session_id = $1 AND role = 'user' AND sequence <= $2
                         ORDER BY sequence DESC LIMIT 1",
                )
                .bind(session_id.as_str())
                .bind(through_message_sequence)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "conversation has no user request to preserve".to_string(),
                    )
                })?,
                _ => unreachable!("validated above"),
            };
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1
             FROM conversation_compactions WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let id = format!("compaction-{}", Uuid::new_v4().simple());
        let row: ConversationCompactionRow = sqlx::query_as(
            "INSERT INTO conversation_compactions (
                 id, session_id, sequence, through_message_sequence,
                 replaced_through_message_sequence, source_message_count,
                 checkpoint_format_version, kind, summary_format_version,
                 last_user_message_id, last_user_message_sequence,
                 resolved_model_name, summary, runtime_state,
                 runtime_reminder_format_version, runtime_reminder,
                 trigger_turn_id, parent_compaction_id, input_tokens, output_tokens
             ) VALUES (
                 $1, $2, $3, $4, $4, $5, 1, $6, 1, $7, $8,
                 $9, $10, $11, 1, $12, $13, NULL, $14, $15
             )
             RETURNING id, session_id, sequence, through_message_sequence,
                       replaced_through_message_sequence, source_message_count,
                       checkpoint_format_version, kind, summary_format_version,
                       last_user_message_id, last_user_message_sequence,
                       resolved_model_name, summary, runtime_state,
                       runtime_reminder_format_version, runtime_reminder,
                       trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                       to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at",
        )
        .bind(&id)
        .bind(session_id.as_str())
        .bind(sequence)
        .bind(through_message_sequence)
        .bind(source_message_count)
        .bind(input.kind.as_str())
        .bind(&last_user_message_id)
        .bind(last_user_message_sequence)
        .bind(&input.resolved_model_name)
        .bind(&summary)
        .bind(runtime_state)
        .bind(&input.runtime_reminder)
        .bind(trigger_turn_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .fetch_one(&mut *transaction)
        .await?;
        if let Some(turn_id) = trigger_turn_id {
            sqlx::query(
                "UPDATE turns SET
                     input_tokens = CASE WHEN $2::BIGINT IS NULL THEN input_tokens
                         ELSE COALESCE(input_tokens, 0) + $2 END,
                     output_tokens = CASE WHEN $3::BIGINT IS NULL THEN output_tokens
                         ELSE COALESCE(output_tokens, 0) + $3 END,
                     updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                 WHERE id = $1",
            )
            .bind(turn_id)
            .bind(input_tokens)
            .bind(output_tokens)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        conversation_compaction_from_row(row)
    }

    async fn rewind_conversation_compaction_inner(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
        runtime_state: CompactionRuntimeState,
        runtime_reminder: String,
    ) -> Result<ConversationCompaction, StorageError> {
        validate_runtime_checkpoint_state(&runtime_state, &runtime_reminder)?;
        let runtime_state = serde_json::to_value(runtime_state)?;
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        let running_turn: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                 SELECT 1 FROM turns WHERE session_id = $1 AND status = 'running'
             )",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if running_turn {
            return Err(StorageError::InvalidInput(format!(
                "session has an active turn and cannot be rewound: {session_id}"
            )));
        }

        let source: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1 AND id = $2
             FOR SHARE",
        )
        .bind(session_id.as_str())
        .bind(compaction_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let source = source.ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "conversation compaction not found: {compaction_id}"
            ))
        })?;
        let source_checkpoint_format =
            stored_format_version(source.checkpoint_format_version, "checkpoint")?;
        if source_checkpoint_format > 1 {
            return Err(StorageError::InvalidInput(format!(
                "unsupported stored checkpoint format version: {source_checkpoint_format}"
            )));
        }
        let source_summary_format =
            stored_format_version(source.summary_format_version, "summary")?;
        match source_summary_format {
            0 => {}
            1 => {
                validate_summary_text(&source.summary)
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            }
            version => {
                return Err(StorageError::InvalidInput(format!(
                    "unsupported stored summary format version: {version}"
                )));
            }
        }
        if source.last_user_message_id.is_none() || source.last_user_message_sequence.is_none() {
            return Err(StorageError::InvalidInput(format!(
                "conversation compaction cannot be restored because it has no last user anchor: {compaction_id}"
            )));
        }

        let replaced_through_message_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1
             FROM conversation_compactions WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let id = format!("compaction-{}", Uuid::new_v4().simple());
        let row: ConversationCompactionRow = sqlx::query_as(
            "INSERT INTO conversation_compactions (
                 id, session_id, sequence, through_message_sequence,
                 replaced_through_message_sequence, source_message_count,
                 checkpoint_format_version, kind, summary_format_version,
                 last_user_message_id, last_user_message_sequence,
                 resolved_model_name, summary, runtime_state,
                 runtime_reminder_format_version, runtime_reminder,
                 trigger_turn_id, parent_compaction_id, input_tokens, output_tokens
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, 'rewind', $8, $9, $10,
                 $11, $12, $13, $14, $15, NULL, $16, $17, $18
             )
             RETURNING id, session_id, sequence, through_message_sequence,
                       replaced_through_message_sequence, source_message_count,
                       checkpoint_format_version, kind, summary_format_version,
                       last_user_message_id, last_user_message_sequence,
                       resolved_model_name, summary, runtime_state,
                       runtime_reminder_format_version, runtime_reminder,
                       trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                       to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at",
        )
        .bind(&id)
        .bind(session_id.as_str())
        .bind(sequence)
        .bind(source.through_message_sequence)
        .bind(replaced_through_message_sequence)
        .bind(source.source_message_count)
        .bind(1_i16)
        .bind(source.summary_format_version)
        .bind(&source.last_user_message_id)
        .bind(source.last_user_message_sequence)
        .bind(&source.resolved_model_name)
        .bind(&source.summary)
        .bind(runtime_state)
        .bind(1_i16)
        .bind(runtime_reminder)
        .bind(&source.id)
        .bind(source.input_tokens)
        .bind(source.output_tokens)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        conversation_compaction_from_row(row)
    }

    async fn delete_conversation_compaction_inner(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<(), StorageError> {
        let result =
            sqlx::query("DELETE FROM conversation_compactions WHERE id = $1 AND session_id = $2")
                .bind(compaction_id)
                .bind(session_id.as_str())
                .execute(&self.pool)
                .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InvalidInput(format!(
                "conversation compaction not found: {compaction_id}"
            )));
        }
        Ok(())
    }

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
                 resolved_provider_kind, resolved_model_name, app_version, status
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'running')",
        )
        .bind(turn_id.as_str())
        .bind(session_id.as_str())
        .bind(client_request_id.as_str())
        .bind(turn_sequence)
        .bind(&model.model_id)
        .bind(&model.provider_kind)
        .bind(&model.model_name)
        .bind(env!("CARGO_PKG_VERSION"))
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
             SET updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 last_turn_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
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
    ) -> Result<String, StorageError> {
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
        let message_id = insert_message(
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
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
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
        Ok(message_id)
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
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
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

async fn lock_trace_payload_mutations(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StorageError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(TRACE_PAYLOAD_ADVISORY_LOCK)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn cleanup_trace_payload_candidates(
    transaction: &mut Transaction<'_, Postgres>,
    mut payload_hashes: Vec<String>,
) -> Result<(), StorageError> {
    payload_hashes.sort_unstable();
    payload_hashes.dedup();
    for payload_hash in payload_hashes {
        // Keep each candidate in a savepoint. RESTRICT is the final defense for
        // an already-committed reference written outside the shared lock path;
        // one such reference must not roll back cleanup of unrelated bodies.
        let mut cleanup = transaction.begin().await?;
        let deletion = sqlx::query(
            "DELETE FROM trace_payloads AS payload
             WHERE payload.hash = $1
               AND NOT EXISTS (
                   SELECT 1 FROM trace_span_payloads AS mapping
                   WHERE mapping.payload_hash = payload.hash
               )",
        )
        .bind(payload_hash)
        .execute(&mut *cleanup)
        .await;
        match deletion {
            Ok(_) => cleanup.commit().await?,
            Err(error) if is_payload_reference_conflict(&error) => cleanup.rollback().await?,
            Err(error) => return Err(error.into()),
        }
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
) -> Result<String, StorageError> {
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
    .bind(&message_id)
    .bind(session_id.as_str())
    .bind(turn_id.map(TurnId::as_str))
    .bind(sequence)
    .bind(role.as_provider_str())
    .bind(content)
    .bind(provider_call_id)
    .bind(tool_name)
    .execute(&mut **transaction)
    .await?;
    Ok(message_id)
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

fn validate_runtime_checkpoint_state(
    runtime_state: &CompactionRuntimeState,
    runtime_reminder: &str,
) -> Result<(), StorageError> {
    if runtime_state.schema_version != 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported compaction runtime state schema version: {}",
            runtime_state.schema_version
        )));
    }
    validate_system_reminder(runtime_reminder)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn stored_token(value: Option<i64>) -> Result<Option<u64>, StorageError> {
    value
        .map(|value| {
            u64::try_from(value).map_err(|_| {
                StorageError::InvalidInput("stored token count is negative".to_string())
            })
        })
        .transpose()
}

fn conversation_compaction_from_row(
    row: ConversationCompactionRow,
) -> Result<ConversationCompaction, StorageError> {
    let kind = match row.kind.as_str() {
        "manual" => ConversationCompactionKind::Manual,
        "threshold" => ConversationCompactionKind::Threshold,
        "overflow" => ConversationCompactionKind::Overflow,
        "rewind" => ConversationCompactionKind::Rewind,
        other => {
            return Err(StorageError::InvalidInput(format!(
                "unknown stored compaction kind: {other}"
            )));
        }
    };
    let checkpoint_format_version =
        stored_format_version(row.checkpoint_format_version, "checkpoint")?;
    if checkpoint_format_version > 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported stored checkpoint format version: {checkpoint_format_version}"
        )));
    }
    let summary_format_version = stored_format_version(row.summary_format_version, "summary")?;
    if summary_format_version > 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported stored summary format version: {summary_format_version}"
        )));
    }
    let summary = if summary_format_version == 1 {
        validate_summary_text(&row.summary)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?
    } else {
        row.summary
    };
    let runtime_reminder_format_version =
        stored_format_version(row.runtime_reminder_format_version, "runtime reminder")?;
    if runtime_reminder_format_version != 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported stored runtime reminder format version: {runtime_reminder_format_version}"
        )));
    }
    let runtime_state: CompactionRuntimeState = serde_json::from_value(row.runtime_state)?;
    validate_runtime_checkpoint_state(&runtime_state, &row.runtime_reminder)?;

    Ok(ConversationCompaction {
        id: row.id,
        session_id: row.session_id,
        sequence: row.sequence,
        through_message_sequence: row.through_message_sequence,
        replaced_through_message_sequence: row.replaced_through_message_sequence,
        source_message_count: u32::try_from(row.source_message_count).map_err(|_| {
            StorageError::InvalidInput(
                "stored compaction source message count is invalid".to_string(),
            )
        })?,
        checkpoint_format_version,
        kind,
        summary_format_version,
        last_user_message_id: row.last_user_message_id,
        last_user_message_sequence: row.last_user_message_sequence,
        resolved_model_name: row.resolved_model_name,
        summary,
        runtime_state,
        runtime_reminder_format_version,
        runtime_reminder: row.runtime_reminder,
        trigger_turn_id: row.trigger_turn_id,
        parent_compaction_id: row.parent_compaction_id,
        input_tokens: stored_token(row.input_tokens)?,
        output_tokens: stored_token(row.output_tokens)?,
        created_at: row.created_at,
    })
}

fn stored_format_version(value: i16, name: &str) -> Result<u16, StorageError> {
    u16::try_from(value).map_err(|_| {
        StorageError::InvalidInput(format!("stored {name} format version is negative"))
    })
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
            trace_id: "turn-1".to_string(),
            session_id: "session-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            parent_span_id: parent_span_id.map(str::to_string),
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
            response_message_id: None,
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
            span(
                "summary-model-1",
                "model_call",
                Some("compaction-1"),
                "succeeded",
            ),
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
