use openwork_database::{DbDateTime, Migration};
use openwork_db_macros::PgEntity;

#[derive(Debug, Clone, PgEntity)]
#[table_name = "sessions"]
#[allow(dead_code)]
pub struct SessionRecord {
    #[primary_key]
    pub id: String,
    pub title: String,
    #[indexed]
    pub provider_id: String,
    pub model: String,
    pub working_dir: Option<String>,
    pub created_at: DbDateTime,
    #[indexed]
    pub updated_at: DbDateTime,
}

#[derive(Debug, Clone, PgEntity)]
#[table_name = "messages"]
#[allow(dead_code)]
pub struct MessageRecord {
    #[primary_key]
    pub id: String,
    #[indexed]
    pub session_id: String,
    pub role: String,
    pub parts_json: serde_json::Value,
    pub seq: i64,
    pub created_at: DbDateTime,
}

#[derive(Debug, Clone, PgEntity)]
#[table_name = "llm_events"]
#[allow(dead_code)]
pub struct LlmEventRecord {
    #[primary_key]
    pub id: String,
    #[indexed]
    pub session_id: String,
    #[indexed]
    pub request_id: String,
    pub event: String,
    pub payload_json: serde_json::Value,
    pub seq: i64,
    pub created_at: DbDateTime,
}

#[derive(Debug, Clone, PgEntity)]
#[table_name = "tool_runs"]
#[allow(dead_code)]
pub struct ToolRunRecord {
    #[primary_key]
    pub id: String,
    #[indexed]
    pub session_id: String,
    pub request_id: String,
    pub tool_call_id: String,
    pub name: String,
    pub input_json: serde_json::Value,
    pub approval_status: Option<String>,
    pub started_at: DbDateTime,
    pub finished_at: Option<DbDateTime>,
    pub duration_ms: Option<i64>,
    pub output_json: Option<serde_json::Value>,
    pub is_error: bool,
}

pub const SESSION_MIGRATIONS: &[Migration] = &[Migration {
    version: 202606250101,
    name: "create_sessions",
    statements: &[
        r#"CREATE TABLE IF NOT EXISTS sessions (
           id TEXT PRIMARY KEY,
           title TEXT NOT NULL,
           provider_id TEXT NOT NULL,
           model TEXT NOT NULL,
           working_dir TEXT,
           created_at TIMESTAMPTZ NOT NULL,
           updated_at TIMESTAMPTZ NOT NULL
         )"#,
        "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC)",
        r#"CREATE TABLE IF NOT EXISTS messages (
           id TEXT PRIMARY KEY,
           session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
           role TEXT NOT NULL,
           parts_json JSONB NOT NULL,
           seq BIGINT NOT NULL,
           created_at TIMESTAMPTZ NOT NULL
         )"#,
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_session_seq ON messages(session_id, seq)",
        r#"CREATE TABLE IF NOT EXISTS llm_events (
           id TEXT PRIMARY KEY,
           session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
           request_id TEXT NOT NULL,
           event TEXT NOT NULL,
           payload_json JSONB NOT NULL,
           seq BIGINT NOT NULL,
           created_at TIMESTAMPTZ NOT NULL
         )"#,
        "CREATE INDEX IF NOT EXISTS idx_llm_events_session_seq ON llm_events(session_id, seq)",
        "CREATE INDEX IF NOT EXISTS idx_llm_events_request_seq ON llm_events(request_id, seq)",
        r#"CREATE TABLE IF NOT EXISTS tool_runs (
           id TEXT PRIMARY KEY,
           session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
           request_id TEXT NOT NULL,
           tool_call_id TEXT NOT NULL,
           name TEXT NOT NULL,
           input_json JSONB NOT NULL,
           approval_status TEXT,
           started_at TIMESTAMPTZ NOT NULL,
           finished_at TIMESTAMPTZ,
           duration_ms BIGINT,
           output_json JSONB,
           is_error BOOLEAN NOT NULL DEFAULT false
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_tool_runs_session_request ON tool_runs(session_id, request_id)",
    ],
}];
