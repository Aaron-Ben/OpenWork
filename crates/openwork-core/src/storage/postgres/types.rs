use super::*;

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

/// Creation input for a sub-agent Session.
///
/// Deliberately a separate type from [`SessionInput`] rather than four optional
/// fields on it: the two paths have different callers (a user versus a spawning
/// agent), different required fields, and different validation. Optional fields
/// would let a caller build a half-filled sub-agent that only the database
/// rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentSessionInput {
    pub id: SessionId,
    pub parent_session_id: SessionId,
    /// Unique within the parent Session. This is what the model addresses.
    pub task_name: String,
    pub agent_role: String,
    /// Inherited from the parent; never widened or narrowed.
    pub working_directory: String,
    pub default_model_id: Option<String>,
    /// Tool Call Span that spawned it. `None` when the Trace write was dropped.
    pub spawn_span_id: Option<String>,
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
    /// All four are `None` for a root Session and `Some` for a sub-agent,
    /// except `spawn_span_id` which may be `None` either way.
    pub parent_session_id: Option<String>,
    pub task_name: Option<String>,
    pub agent_role: Option<String>,
    pub spawn_span_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeliveredSubAgentResult {
    pub child_session_id: SessionId,
    pub child_turn_id: TurnId,
    pub task_name: String,
    pub status: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub final_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct DeletedOrphanSubAgent {
    pub session_id: String,
    pub task_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubAgentReconciliation {
    pub undelivered: Vec<UndeliveredSubAgentResult>,
    pub deleted_orphans: Vec<DeletedOrphanSubAgent>,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct UndeliveredSubAgentResultRow {
    pub(super) child_session_id: String,
    pub(super) child_turn_id: String,
    pub(super) task_name: String,
    pub(super) status: String,
    pub(super) error_code: Option<String>,
    pub(super) error_message: Option<String>,
    pub(super) assistant_content: Option<Value>,
}

impl SessionRecord {
    pub fn is_sub_agent(&self) -> bool {
        self.parent_session_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessageRecord {
    pub id: String,
    pub turn_id: Option<String>,
    pub sequence: i64,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub message_kind: MessageKind,
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
pub(super) struct ConversationCompactionRow {
    pub(super) id: String,
    pub(super) session_id: String,
    pub(super) sequence: i64,
    pub(super) through_message_sequence: i64,
    pub(super) replaced_through_message_sequence: i64,
    pub(super) source_message_count: i32,
    pub(super) checkpoint_format_version: i16,
    pub(super) kind: String,
    pub(super) summary_format_version: i16,
    pub(super) last_user_message_id: Option<String>,
    pub(super) last_user_message_sequence: Option<i64>,
    pub(super) resolved_model_name: String,
    pub(super) summary: String,
    pub(super) runtime_state: Value,
    pub(super) runtime_reminder_format_version: i16,
    pub(super) runtime_reminder: String,
    pub(super) trigger_turn_id: Option<String>,
    pub(super) parent_compaction_id: Option<String>,
    pub(super) input_tokens: Option<i64>,
    pub(super) output_tokens: Option<i64>,
    pub(super) created_at: String,
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
    /// 整条 Trace 的 token 合计：各 span 的 `input_tokens + output_tokens` 求和，
    /// 与 Span 级 `total_tokens` 同口径；未记录 token 的 span 按 0 计。
    /// 注意该值跨 provider 不可直接比较（cached 是否计入 input 各家不同）。
    pub total_tokens: i64,
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
