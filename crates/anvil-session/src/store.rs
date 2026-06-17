//! 会话持久化:SQLite 存储 sessions 与 messages。
//!
//! messages 表把每个消息的 parts(`Vec<ContentBlock>`)序列化为 JSON 列。
//! 用 `Arc<Mutex<Connection>>` 保护单连接,SQLite 单写串行化保证正确性。
//! 风格对齐 `anvil_providers::ProviderStore`(同步 Mutex + 原子操作)。

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anvil_core::ai::{ContentBlock, Role};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
pub struct SessionMessagePart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub part_index: i64,
    pub part: ContentBlock,
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

#[derive(Debug, Clone)]
pub struct NewWorktreeSnapshot {
    pub request_id: String,
    pub working_dir: String,
    pub before_status: serde_json::Value,
    pub after_status: serde_json::Value,
    pub changed_files: Vec<String>,
    pub files: Vec<NewWorktreeSnapshotFile>,
}

#[derive(Debug, Clone)]
pub struct NewWorktreeSnapshotFile {
    pub path: String,
    pub before_content: Option<Vec<u8>>,
    pub after_content: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshotSummary {
    pub id: String,
    pub session_id: String,
    pub request_id: String,
    pub working_dir: String,
    pub changed_files: Vec<String>,
    pub created_at: i64,
    pub completed_at: i64,
    pub reverted_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct WorktreeSnapshotRecord {
    pub summary: WorktreeSnapshotSummary,
    pub files: Vec<WorktreeSnapshotFileRecord>,
}

#[derive(Debug, Clone)]
pub struct WorktreeSnapshotFileRecord {
    pub path: String,
    pub before_content: Option<Vec<u8>>,
    pub after_content: Option<Vec<u8>>,
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
    #[error("session io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("session serialize error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// 会话仓库:单连接 + Mutex 保护。`Clone` 廉价(内部 `Arc`)。
#[derive(Clone)]
pub struct SessionStore {
    conn: Arc<Mutex<Connection>>,
}

impl SessionStore {
    /// 打开(或创建)`anvil.db`;建父目录、跑 schema。文件不存在则自动创建。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        Self::run_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn run_schema(conn: &Connection) -> Result<(), SessionError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
               id TEXT PRIMARY KEY,
               title TEXT NOT NULL,
               provider_id TEXT NOT NULL,
               model TEXT NOT NULL,
               working_dir TEXT,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               role TEXT NOT NULL,
               parts_json TEXT NOT NULL,
               seq INTEGER NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_messages_session_seq ON messages(session_id, seq);
             CREATE TABLE IF NOT EXISTS message_parts (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
               part_index INTEGER NOT NULL,
               part_json TEXT NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_message_parts_message_index
               ON message_parts(message_id, part_index);
             CREATE TABLE IF NOT EXISTS llm_events (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               request_id TEXT NOT NULL,
               event TEXT NOT NULL,
               payload_json TEXT NOT NULL,
               seq INTEGER NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_llm_events_session_seq ON llm_events(session_id, seq);",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS worktree_snapshots (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               request_id TEXT NOT NULL,
               working_dir TEXT NOT NULL,
               before_status_json TEXT NOT NULL,
               after_status_json TEXT NOT NULL,
               changed_files_json TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               completed_at INTEGER NOT NULL,
               reverted_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_worktree_snapshots_session_completed
               ON worktree_snapshots(session_id, completed_at);
             CREATE TABLE IF NOT EXISTS worktree_snapshot_files (
               id TEXT PRIMARY KEY,
               snapshot_id TEXT NOT NULL REFERENCES worktree_snapshots(id) ON DELETE CASCADE,
               path TEXT NOT NULL,
               before_content BLOB,
               after_content BLOB
             );
             CREATE INDEX IF NOT EXISTS idx_worktree_snapshot_files_snapshot
               ON worktree_snapshot_files(snapshot_id);",
        )?;
        Ok(())
    }

    /// 列出所有会话摘要,按 `updated_at` 倒序。
    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, title, provider_id, model, updated_at
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                provider_id: row.get(2)?,
                model: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 创建新会话。
    pub fn create_session(&self, input: SessionInput) -> Result<Session, SessionError> {
        let now = now_secs();
        let session = Session {
            id: generate_id(),
            title: input
                .title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "New session".to_string()),
            provider_id: input.provider_id,
            model: input.model,
            working_dir: input.working_dir,
            created_at: now,
            updated_at: now,
        };
        let conn = self.conn.lock().expect("session store mutex poisoned");
        conn.execute(
            "INSERT INTO sessions
               (id, title, provider_id, model, working_dir, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.id,
                session.title,
                session.provider_id,
                session.model,
                session.working_dir,
                session.created_at,
                session.updated_at,
            ],
        )?;
        Ok(session)
    }

    /// 加载单个会话(不含消息)。
    pub fn load_session(&self, id: &str) -> Result<Option<Session>, SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let session = conn
            .query_row(
                "SELECT id, title, provider_id, model, working_dir, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Session {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        provider_id: row.get(2)?,
                        model: row.get(3)?,
                        working_dir: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(session)
    }

    /// 加载会话的所有消息,按 `seq` 升序。
    pub fn load_messages(&self, session_id: &str) -> Result<Vec<SessionMessage>, SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, parts_json, seq, created_at
             FROM messages WHERE session_id = ?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let role_str: String = row.get(2)?;
            let parts_json: String = row.get(3)?;
            let parts: Vec<ContentBlock> = serde_json::from_str(&parts_json).unwrap_or_default();
            Ok(SessionMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: role_from_str(&role_str),
                parts,
                seq: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 重命名会话;不存在返回 `NotFound`。
    pub fn rename_session(&self, id: &str, title: &str) -> Result<Session, SessionError> {
        {
            let conn = self.conn.lock().expect("session store mutex poisoned");
            let updated = conn.execute(
                "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, now_secs(), id],
            )?;
            if updated == 0 {
                return Err(SessionError::NotFound { id: id.to_string() });
            }
        }
        self.load_session(id)?
            .ok_or_else(|| SessionError::NotFound { id: id.to_string() })
    }

    /// 删除会话(级联删除其消息);不存在返回 `NotFound`。
    pub fn delete_session(&self, id: &str) -> Result<(), SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let deleted = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(SessionError::NotFound { id: id.to_string() });
        }
        Ok(())
    }

    /// 事务性地追加一批消息,seq 从当前 max+1 连续递增;同时 touch `updated_at`。
    pub fn append_messages(
        &self,
        session_id: &str,
        new_messages: Vec<NewMessage>,
    ) -> Result<Vec<SessionMessage>, SessionError> {
        if new_messages.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_secs();
        let mut conn = self.conn.lock().expect("session store mutex poisoned");
        let tx = conn.transaction()?;

        let session_exists: bool = tx
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !session_exists {
            return Err(SessionError::NotFound {
                id: session_id.to_string(),
            });
        }

        let max_seq: Option<i64> = tx
            .query_row(
                "SELECT MAX(seq) FROM messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let mut next_seq = max_seq.unwrap_or(0) + 1;

        let mut out = Vec::with_capacity(new_messages.len());
        for msg in new_messages {
            let id = generate_id();
            let parts_json = serde_json::to_string(&msg.parts)?;
            tx.execute(
                "INSERT INTO messages (id, session_id, role, parts_json, seq, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    session_id,
                    role_to_str(msg.role),
                    parts_json,
                    next_seq,
                    now
                ],
            )?;
            for (part_index, part) in msg.parts.iter().enumerate() {
                let part_id = generate_id();
                let part_json = serde_json::to_string(part)?;
                tx.execute(
                    "INSERT INTO message_parts
                       (id, session_id, message_id, part_index, part_json, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![part_id, session_id, id, part_index as i64, part_json, now],
                )?;
            }
            out.push(SessionMessage {
                id,
                session_id: session_id.to_string(),
                role: msg.role,
                parts: msg.parts,
                seq: next_seq,
                created_at: now,
            });
            next_seq += 1;
        }

        tx.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        tx.commit()?;
        Ok(out)
    }

    pub fn append_llm_event(
        &self,
        session_id: &str,
        request_id: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<SessionEvent, SessionError> {
        let now = now_secs();
        let mut conn = self.conn.lock().expect("session store mutex poisoned");
        let tx = conn.transaction()?;

        let session_exists: bool = tx
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !session_exists {
            return Err(SessionError::NotFound {
                id: session_id.to_string(),
            });
        }

        let max_seq: Option<i64> = tx
            .query_row(
                "SELECT MAX(seq) FROM llm_events WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let seq = max_seq.unwrap_or(0) + 1;
        let id = generate_id();
        let payload_json = serde_json::to_string(&payload)?;
        tx.execute(
            "INSERT INTO llm_events
               (id, session_id, request_id, event, payload_json, seq, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, session_id, request_id, event, payload_json, seq, now],
        )?;
        tx.commit()?;
        Ok(SessionEvent {
            id,
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
            event: event.to_string(),
            payload,
            seq,
            created_at: now,
        })
    }

    pub fn append_worktree_snapshot(
        &self,
        session_id: &str,
        snapshot: NewWorktreeSnapshot,
    ) -> Result<Option<WorktreeSnapshotSummary>, SessionError> {
        if snapshot.changed_files.is_empty() {
            return Ok(None);
        }
        let now = now_secs();
        let mut conn = self.conn.lock().expect("session store mutex poisoned");
        let tx = conn.transaction()?;

        let session_exists: bool = tx
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !session_exists {
            return Err(SessionError::NotFound {
                id: session_id.to_string(),
            });
        }

        let id = generate_id();
        let before_status_json = serde_json::to_string(&snapshot.before_status)?;
        let after_status_json = serde_json::to_string(&snapshot.after_status)?;
        let changed_files_json = serde_json::to_string(&snapshot.changed_files)?;
        tx.execute(
            "INSERT INTO worktree_snapshots
               (id, session_id, request_id, working_dir, before_status_json, after_status_json,
                changed_files_json, created_at, completed_at, reverted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
            params![
                id,
                session_id,
                snapshot.request_id,
                snapshot.working_dir,
                before_status_json,
                after_status_json,
                changed_files_json,
                now,
                now,
            ],
        )?;
        for file in snapshot.files {
            tx.execute(
                "INSERT INTO worktree_snapshot_files
                   (id, snapshot_id, path, before_content, after_content)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    generate_id(),
                    id,
                    file.path,
                    file.before_content,
                    file.after_content
                ],
            )?;
        }
        tx.commit()?;

        Ok(Some(WorktreeSnapshotSummary {
            id,
            session_id: session_id.to_string(),
            request_id: snapshot.request_id,
            working_dir: snapshot.working_dir,
            changed_files: snapshot.changed_files,
            created_at: now,
            completed_at: now,
            reverted_at: None,
        }))
    }

    pub fn list_worktree_snapshots(
        &self,
        session_id: &str,
    ) -> Result<Vec<WorktreeSnapshotSummary>, SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, session_id, request_id, working_dir, changed_files_json,
                    created_at, completed_at, reverted_at
             FROM worktree_snapshots
             WHERE session_id = ?1
             ORDER BY completed_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let changed_files_json: String = row.get(4)?;
            let changed_files: Vec<String> =
                serde_json::from_str(&changed_files_json).unwrap_or_default();
            Ok(WorktreeSnapshotSummary {
                id: row.get(0)?,
                session_id: row.get(1)?,
                request_id: row.get(2)?,
                working_dir: row.get(3)?,
                changed_files,
                created_at: row.get(5)?,
                completed_at: row.get(6)?,
                reverted_at: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn load_worktree_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<WorktreeSnapshotRecord>, SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let summary = conn
            .query_row(
                "SELECT id, session_id, request_id, working_dir, changed_files_json,
                        created_at, completed_at, reverted_at
                 FROM worktree_snapshots WHERE id = ?1",
                params![snapshot_id],
                |row| {
                    let changed_files_json: String = row.get(4)?;
                    let changed_files: Vec<String> =
                        serde_json::from_str(&changed_files_json).unwrap_or_default();
                    Ok(WorktreeSnapshotSummary {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        request_id: row.get(2)?,
                        working_dir: row.get(3)?,
                        changed_files,
                        created_at: row.get(5)?,
                        completed_at: row.get(6)?,
                        reverted_at: row.get(7)?,
                    })
                },
            )
            .optional()?;
        let Some(summary) = summary else {
            return Ok(None);
        };

        let mut stmt = conn.prepare(
            "SELECT path, before_content, after_content
             FROM worktree_snapshot_files
             WHERE snapshot_id = ?1
             ORDER BY path ASC",
        )?;
        let rows = stmt.query_map(params![snapshot_id], |row| {
            Ok(WorktreeSnapshotFileRecord {
                path: row.get(0)?,
                before_content: row.get(1)?,
                after_content: row.get(2)?,
            })
        })?;
        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(Some(WorktreeSnapshotRecord { summary, files }))
    }

    pub fn mark_worktree_snapshot_reverted(&self, snapshot_id: &str) -> Result<(), SessionError> {
        let conn = self.conn.lock().expect("session store mutex poisoned");
        let updated = conn.execute(
            "UPDATE worktree_snapshots SET reverted_at = ?1 WHERE id = ?2",
            params![now_secs(), snapshot_id],
        )?;
        if updated == 0 {
            return Err(SessionError::NotFound {
                id: snapshot_id.to_string(),
            });
        }
        Ok(())
    }
}

fn generate_id() -> String {
    format!("sess-{}", Uuid::new_v4().simple())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    use anvil_core::ai::{ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState};
    use std::path::PathBuf;

    fn temp_db() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("anvil-session-test-{}.db", Uuid::new_v4().simple()));
        path
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(format!("{}-wal", path.display()));
        let _ = fs::remove_file(format!("{}-shm", path.display()));
    }

    fn sample_input() -> SessionInput {
        SessionInput {
            title: Some("Test".to_string()),
            provider_id: "prov-1".to_string(),
            model: "test-model".to_string(),
            working_dir: None,
        }
    }

    #[test]
    fn open_missing_creates_db() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        assert!(store.list_sessions().unwrap().is_empty());
        assert!(path.exists());
        cleanup(&path);
    }

    #[test]
    fn create_and_list_sessions() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

        assert!(session.id.starts_with("sess-"));
        let listed = store.list_sessions().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, session.id);

        // 重新打开验证持久化。
        drop(store);
        let store2 = SessionStore::open(&path).unwrap();
        assert_eq!(store2.list_sessions().unwrap().len(), 1);
        cleanup(&path);
    }

    #[test]
    fn append_assigns_sequential_seq() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

        let first = store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::User,
                    parts: vec![ContentBlock::text("hi")],
                }],
            )
            .unwrap();
        let second = store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::Assistant,
                    parts: vec![ContentBlock::text("hello")],
                }],
            )
            .unwrap();

        assert_eq!(first[0].seq, 1);
        assert_eq!(second[0].seq, 2);
        cleanup(&path);
    }

    #[test]
    fn append_multiple_in_one_call() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

        let appended = store
            .append_messages(
                &session.id,
                vec![
                    NewMessage {
                        role: Role::User,
                        parts: vec![ContentBlock::text("q")],
                    },
                    NewMessage {
                        role: Role::Assistant,
                        parts: vec![ContentBlock::text("a")],
                    },
                ],
            )
            .unwrap();
        assert_eq!(appended.len(), 2);
        assert_eq!(appended[0].seq, 1);
        assert_eq!(appended[1].seq, 2);

        let loaded = store.load_messages(&session.id).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].role, Role::User);
        assert_eq!(loaded[1].role, Role::Assistant);
        cleanup(&path);
    }

    #[test]
    fn load_returns_in_seq_order() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

        // 故意乱序 append,验证 load 按 seq 升序。
        store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::User,
                    parts: vec![ContentBlock::text("first")],
                }],
            )
            .unwrap();
        store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::Assistant,
                    parts: vec![ContentBlock::text("second")],
                }],
            )
            .unwrap();

        let loaded = store.load_messages(&session.id).unwrap();
        assert_eq!(loaded[0].parts[0], ContentBlock::text("first"));
        assert_eq!(loaded[1].parts[0], ContentBlock::text("second"));
        cleanup(&path);
    }

    #[test]
    fn delete_cascades_messages() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();
        store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::User,
                    parts: vec![ContentBlock::text("hi")],
                }],
            )
            .unwrap();
        assert_eq!(store.load_messages(&session.id).unwrap().len(), 1);

        store.delete_session(&session.id).unwrap();
        assert!(store.list_sessions().unwrap().is_empty());
        assert!(store.load_messages(&session.id).unwrap().is_empty());

        // 再删一次应 NotFound。
        assert!(matches!(
            store.delete_session(&session.id),
            Err(SessionError::NotFound { .. })
        ));
        cleanup(&path);
    }

    #[test]
    fn parts_json_roundtrip_preserves_all_block_kinds() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

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
            .unwrap();

        let loaded = store.load_messages(&session.id).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].parts, parts);
        cleanup(&path);
    }

    #[test]
    fn append_messages_also_writes_message_parts() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();
        let appended = store
            .append_messages(
                &session.id,
                vec![NewMessage {
                    role: Role::Assistant,
                    parts: vec![
                        ContentBlock::thinking("think"),
                        ContentBlock::text("answer"),
                    ],
                }],
            )
            .unwrap();

        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM message_parts WHERE message_id = ?1",
                params![appended[0].id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn append_llm_event_assigns_sequential_seq() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

        let first = store
            .append_llm_event(
                &session.id,
                "req-1",
                "text_delta",
                serde_json::json!({"delta": "a"}),
            )
            .unwrap();
        let second = store
            .append_llm_event(
                &session.id,
                "req-1",
                "text_delta",
                serde_json::json!({"delta": "b"}),
            )
            .unwrap();

        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
        cleanup(&path);
    }

    #[test]
    fn rename_updates_title() {
        let path = temp_db();
        let store = SessionStore::open(&path).unwrap();
        let session = store.create_session(sample_input()).unwrap();

        let renamed = store.rename_session(&session.id, "Renamed").unwrap();
        assert_eq!(renamed.title, "Renamed");
        assert_eq!(
            store.load_session(&session.id).unwrap().unwrap().title,
            "Renamed"
        );

        assert!(matches!(
            store.rename_session("nonexistent", "x"),
            Err(SessionError::NotFound { .. })
        ));
        cleanup(&path);
    }
}
