//! 会话持久化:PostgreSQL 存储 sessions、messages 与 llm_events。
//!
//! 当前实现保持前端可见 JSON 形状不变:id 仍为 String,时间仍为 epoch seconds。
//! 底层 PostgreSQL schema 使用 TIMESTAMPTZ,连接会设置为 Asia/Shanghai 显示时区。

use openwork_database::{
    Database, DatabaseConfig, DatabaseError, OrderDirection, PgCrud, PgFilterQuery, QueryCriteria,
    epoch_seconds, now_beijing,
};
use openwork_protocol::model::{ContentBlock, Role};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::records::{LlmEventRecord, MessageRecord, SESSION_MIGRATIONS, SessionRecord};

/// 一个会话的完整记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model: String,
    pub working_dir: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 会话列表项(不含消息)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model: String,
    pub updated_at: i64,
}

/// 创建会话的入参。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    #[serde(default)]
    pub title: Option<String>,
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub working_dir: Option<String>,
}

/// 待写入的消息(role + parts);id/seq/created_at 由 store 分配。
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub role: Role,
    pub parts: Vec<ContentBlock>,
}

/// 已持久化的消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    pub session_id: String,
    pub role: Role,
    pub parts: Vec<ContentBlock>,
    pub seq: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub id: String,
    pub session_id: String,
    pub request_id: String,
    pub event: String,
    pub payload: serde_json::Value,
    pub seq: i64,
    pub created_at: i64,
}

/// `session_load` 的返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadResult {
    pub session: Session,
    pub messages: Vec<SessionMessage>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session not found: {id}")]
    NotFound { id: String },
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("postgres error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("session serialize error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// PostgreSQL-backed 会话仓库。`Clone` 廉价,内部是 `PgPool` clone。
#[derive(Clone)]
pub struct SessionStore {
    db: Database,
}

impl SessionStore {
    pub async fn connect_from_env_or_local() -> Result<Self, SessionError> {
        let db = Database::connect(DatabaseConfig::from_env_or_local()).await?;
        let store = Self { db };
        store.db.migrate(SESSION_MIGRATIONS).await?;
        Ok(store)
    }

    pub async fn connect(config: DatabaseConfig) -> Result<Self, SessionError> {
        let db = Database::connect(config).await?;
        let store = Self { db };
        store.db.migrate(SESSION_MIGRATIONS).await?;
        Ok(store)
    }

    pub fn pool(&self) -> &PgPool {
        self.db.pool()
    }

    /// 列出所有会话摘要,按 `updated_at` 倒序。
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let records = SessionRecord::find_by_criteria(
            QueryCriteria::new().order_by("updated_at", OrderDirection::Desc),
            self.pool(),
        )
        .await?;

        Ok(records
            .into_iter()
            .map(|record| SessionSummary {
                id: record.id,
                title: record.title,
                provider_id: record.provider_id,
                model: record.model,
                updated_at: epoch_seconds(record.updated_at),
            })
            .collect())
    }

    /// 创建新会话。
    pub async fn create_session(&self, input: SessionInput) -> Result<Session, SessionError> {
        let now = now_beijing();
        let now_epoch = epoch_seconds(now);
        let session = Session {
            id: generate_id("sess"),
            title: input
                .title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "New session".to_string()),
            provider_id: input.provider_id,
            model: input.model,
            working_dir: input.working_dir,
            created_at: now_epoch,
            updated_at: now_epoch,
        };

        session_to_record(session.clone())
            .create(self.pool())
            .await?;

        Ok(session)
    }

    /// 加载单个会话(不含消息)。
    pub async fn load_session(&self, id: &str) -> Result<Option<Session>, SessionError> {
        Ok(SessionRecord::get_by_id(id.to_string(), self.pool())
            .await?
            .map(record_to_session))
    }

    /// 加载会话的所有消息,按 `seq` 升序。
    pub async fn load_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessage>, SessionError> {
        let records = MessageRecord::find_by_criteria(
            QueryCriteria::new()
                .eq("session_id", session_id.to_string())
                .order_by("seq", OrderDirection::Asc),
            self.pool(),
        )
        .await?;

        Ok(records.into_iter().map(record_to_message).collect())
    }

    /// 重命名会话;不存在返回 `NotFound`。
    pub async fn rename_session(&self, id: &str, title: &str) -> Result<Session, SessionError> {
        let Some(mut record) = SessionRecord::get_by_id(id.to_string(), self.pool()).await? else {
            return Err(SessionError::NotFound { id: id.to_string() });
        };
        record.title = title.to_string();
        record.updated_at = now_beijing();
        record
            .update(self.pool())
            .await
            .map(record_to_session)
            .map_err(Into::into)
    }

    /// 删除会话(级联删除其消息);不存在返回 `NotFound`。
    pub async fn delete_session(&self, id: &str) -> Result<(), SessionError> {
        let Some(record) = SessionRecord::get_by_id(id.to_string(), self.pool()).await? else {
            return Err(SessionError::NotFound { id: id.to_string() });
        };
        record.delete(self.pool()).await?;
        Ok(())
    }

    /// 事务性地追加一批消息,seq 从当前 max+1 连续递增;同时 touch `updated_at`。
    pub async fn append_messages(
        &self,
        session_id: &str,
        new_messages: Vec<NewMessage>,
    ) -> Result<Vec<SessionMessage>, SessionError> {
        if new_messages.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_beijing();
        let now_epoch = epoch_seconds(now);
        let mut tx = self.pool().begin().await?;

        ensure_session_exists(&mut tx, session_id).await?;

        let max_seq: Option<i64> =
            sqlx::query_scalar("SELECT MAX(seq) FROM messages WHERE session_id = $1")
                .bind(session_id)
                .fetch_one(&mut *tx)
                .await?;
        let mut next_seq = max_seq.unwrap_or(0) + 1;

        let mut out = Vec::with_capacity(new_messages.len());
        for msg in new_messages {
            let id = generate_id("msg");
            let parts_json = serde_json::to_value(&msg.parts)?;
            MessageRecord {
                id: id.clone(),
                session_id: session_id.to_string(),
                role: role_to_str(msg.role).to_string(),
                parts_json: parts_json.clone(),
                seq: next_seq,
                created_at: now,
            }
            .create(&mut *tx)
            .await?;

            out.push(SessionMessage {
                id,
                session_id: session_id.to_string(),
                role: msg.role,
                parts: msg.parts,
                seq: next_seq,
                created_at: now_epoch,
            });
            next_seq += 1;
        }

        sqlx::query("UPDATE sessions SET updated_at = $1 WHERE id = $2")
            .bind(now)
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(out)
    }

    pub async fn append_llm_event(
        &self,
        session_id: &str,
        request_id: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<SessionEvent, SessionError> {
        let now = now_beijing();
        let now_epoch = epoch_seconds(now);
        let mut tx = self.pool().begin().await?;

        ensure_session_exists(&mut tx, session_id).await?;

        let max_seq: Option<i64> =
            sqlx::query_scalar("SELECT MAX(seq) FROM llm_events WHERE session_id = $1")
                .bind(session_id)
                .fetch_one(&mut *tx)
                .await?;
        let seq = max_seq.unwrap_or(0) + 1;
        let id = generate_id("evt");
        LlmEventRecord {
            id: id.clone(),
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
            event: event.to_string(),
            payload_json: payload.clone(),
            seq,
            created_at: now,
        }
        .create(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(SessionEvent {
            id,
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
            event: event.to_string(),
            payload,
            seq,
            created_at: now_epoch,
        })
    }
}

fn session_to_record(session: Session) -> SessionRecord {
    SessionRecord {
        id: session.id,
        title: session.title,
        provider_id: session.provider_id,
        model: session.model,
        working_dir: session.working_dir,
        created_at: db_time_from_epoch(session.created_at),
        updated_at: db_time_from_epoch(session.updated_at),
    }
}

fn record_to_session(record: SessionRecord) -> Session {
    Session {
        id: record.id,
        title: record.title,
        provider_id: record.provider_id,
        model: record.model,
        working_dir: record.working_dir,
        created_at: epoch_seconds(record.created_at),
        updated_at: epoch_seconds(record.updated_at),
    }
}

fn record_to_message(record: MessageRecord) -> SessionMessage {
    SessionMessage {
        id: record.id,
        session_id: record.session_id,
        role: role_from_str(&record.role),
        parts: serde_json::from_value(record.parts_json).unwrap_or_default(),
        seq: record.seq,
        created_at: epoch_seconds(record.created_at),
    }
}

async fn ensure_session_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: &str,
) -> Result<(), SessionError> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1::BIGINT FROM sessions WHERE id = $1")
        .bind(session_id)
        .fetch_optional(&mut **tx)
        .await?;
    if exists.is_none() {
        return Err(SessionError::NotFound {
            id: session_id.to_string(),
        });
    }
    Ok(())
}

fn generate_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn db_time_from_epoch(epoch_seconds: i64) -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(epoch_seconds).unwrap_or_else(|_| now_beijing())
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn role_from_str(s: &str) -> Role {
    match s {
        "system" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::model::{
        ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
    };

    async fn test_store() -> Option<SessionStore> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let store = SessionStore::connect(DatabaseConfig {
            url,
            max_connections: 2,
            acquire_timeout: std::time::Duration::from_secs(5),
        })
        .await
        .ok()?;
        let _ = sqlx::query("TRUNCATE TABLE sessions CASCADE")
            .execute(store.pool())
            .await;
        Some(store)
    }

    fn sample_input() -> SessionInput {
        SessionInput {
            title: Some("Test".to_string()),
            provider_id: "prov-1".to_string(),
            model: "test-model".to_string(),
            working_dir: None,
        }
    }

    #[tokio::test]
    async fn create_and_list_sessions() {
        let Some(store) = test_store().await else {
            return;
        };
        let session = store.create_session(sample_input()).await.unwrap();

        assert!(session.id.starts_with("sess-"));
        let listed = store.list_sessions().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, session.id);
    }

    #[tokio::test]
    async fn append_assigns_sequential_seq() {
        let Some(store) = test_store().await else {
            return;
        };
        let session = store.create_session(sample_input()).await.unwrap();

        let first = store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::User,
                    parts: vec![ContentBlock::text("hi")],
                }],
            )
            .await
            .unwrap();
        let second = store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::Assistant,
                    parts: vec![ContentBlock::text("hello")],
                }],
            )
            .await
            .unwrap();

        assert_eq!(first[0].seq, 1);
        assert_eq!(second[0].seq, 2);
    }

    #[tokio::test]
    async fn parts_json_roundtrip_preserves_all_block_kinds() {
        let Some(store) = test_store().await else {
            return;
        };
        let session = store.create_session(sample_input()).await.unwrap();

        let parts = vec![
            ContentBlock::thinking("let me think"),
            ContentBlock::text("hello"),
            ContentBlock::ToolCall(ToolCallBlock {
                id: "call-1".to_string(),
                name: "bash".to_string(),
                input: "{\"command\":\"ls\"}".to_string(),
                state: ToolCallState::Submitted,
            }),
            ContentBlock::ToolResult(ToolResultBlock {
                id: "call-1".to_string(),
                name: "bash".to_string(),
                output: vec![ContentBlock::text("file_a\nfile_b")],
                state: ToolResultState::Success,
            }),
        ];
        store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::Assistant,
                    parts: parts.clone(),
                }],
            )
            .await
            .unwrap();

        let loaded = store.load_messages(&session.id).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].parts, parts);
    }

    #[tokio::test]
    async fn append_llm_event_assigns_sequential_seq() {
        let Some(store) = test_store().await else {
            return;
        };
        let session = store.create_session(sample_input()).await.unwrap();

        let first = store
            .append_llm_event(
                &session.id,
                "req-1",
                "text_delta",
                serde_json::json!({"delta": "a"}),
            )
            .await
            .unwrap();
        let second = store
            .append_llm_event(
                &session.id,
                "req-1",
                "text_delta",
                serde_json::json!({"delta": "b"}),
            )
            .await
            .unwrap();

        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
    }
}
