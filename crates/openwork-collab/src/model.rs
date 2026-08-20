use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ObservationInput<'a> {
    pub run_id: Option<&'a str>,
    pub agent_id: Option<&'a str>,
    pub room_id: Option<&'a str>,
    pub kind: &'a str,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationRecord {
    pub id: String,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub room_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabLogEntry {
    pub source: String,
    pub id: String,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub room_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub display_name: String,
    pub role: Option<String>,
    pub bio: Option<String>,
    pub system_prompt: String,
    pub provider_id: String,
    pub model_id: String,
    pub opencode_session_id: Option<String>,
    pub enabled: bool,
    pub scanner_enabled: bool,
}

/// Create/update payload. `id` is `None` on create when the daemon should
/// derive it from `display_name`; it must be `Some` for updates, which never
/// re-derive an existing Agent's id (docs/collaboration.md §3.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInput {
    pub id: Option<String>,
    pub display_name: String,
    pub role: Option<String>,
    pub bio: Option<String>,
    pub system_prompt: String,
    pub provider_id: String,
    pub model_id: String,
    pub enabled: bool,
    pub scanner_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentActivity {
    Idle,
    Busy,
    Replying,
    Compacting,
    Executing { detail: String },
    Unresponsive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    #[serde(flatten)]
    pub agent: Agent,
    pub activity: AgentActivity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
    pub next_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMember {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub enabled: bool,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSummary {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
    pub next_sequence: i64,
    pub last_read_sequence: i64,
    pub unread_count: u64,
    pub muted: bool,
    pub members: Vec<RoomMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub room_id: String,
    pub sequence: i64,
    pub author_id: String,
    pub kind: String,
    pub body: String,
    pub system_payload: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "sequence", rename_all = "snake_case")]
pub enum MessagePageAnchor {
    Around(i64),
    Before(i64),
    After(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePageQuery {
    pub anchor: MessagePageAnchor,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageOutcome {
    pub message: Message,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldReply {
    pub peer_sequence: i64,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum AgentReplyOutcome {
    Published(SendMessageOutcome),
    Held(HeldReply),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inbox {
    pub messages: Vec<Message>,
    pub unread_count: u64,
    pub omitted_count: u64,
    pub omission_notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Acted,
    Silent,
    Unpublished,
}

impl RunOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Acted => "acted",
            Self::Silent => "silent",
            Self::Unpublished => "unpublished",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomGlance {
    pub room_id: String,
    pub highest_sequence: i64,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub message_id: String,
    pub actor_id: String,
    pub emoji: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: String,
    pub room_id: String,
    pub title: String,
    pub columns: Vec<BoardColumn>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardColumn {
    pub id: String,
    pub board_id: String,
    pub title: String,
    pub position: i32,
    pub is_done: bool,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub board_id: String,
    pub column_id: String,
    pub title: String,
    pub description: Option<String>,
    pub position: i32,
    pub assignee_id: Option<String>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardInput {
    pub board_id: String,
    pub column_id: String,
    pub title: String,
    pub description: Option<String>,
    pub position: i32,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardMutation {
    pub card: Card,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "result",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CardClaimOutcome {
    Claimed(Box<CardMutation>),
    AlreadyClaimed { card_id: String, claimed_by: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimReleaseCandidate {
    pub card_id: String,
    pub room_id: String,
    pub claimed_by: String,
    pub opencode_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasedClaims {
    pub count: u64,
    pub room_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageSettings {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone)]
pub struct TriageRecordInput<'a> {
    pub agent_id: &'a str,
    pub room_id: &'a str,
    pub up_to_sequence: i64,
    pub actionable: bool,
    pub response_mode: Option<&'a str>,
    pub source: &'a str,
    pub reason: Option<&'a str>,
    pub prompt_note: Option<&'a str>,
    pub provider_id: Option<&'a str>,
    pub model_id: Option<&'a str>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageRecord {
    pub id: String,
    pub agent_id: String,
    pub room_id: String,
    pub up_to_sequence: i64,
    pub actionable: bool,
    pub response_mode: Option<String>,
    pub source: String,
    pub reason: Option<String>,
    pub prompt_note: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaCard {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaCandidate {
    pub room_id: String,
    pub highest_sequence: i64,
    pub cards: Vec<AgendaCard>,
    pub stalled: bool,
    pub recent_messages: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannerRoomSnapshot {
    pub room_id: String,
    pub highest_sequence: i64,
    pub recent_messages: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannerSnapshot {
    pub agent_id: String,
    pub rooms: Vec<ScannerRoomSnapshot>,
}
