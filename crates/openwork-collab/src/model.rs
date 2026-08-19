use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInput {
    pub id: String,
    pub display_name: String,
    pub role: Option<String>,
    pub bio: Option<String>,
    pub system_prompt: String,
    pub provider_id: String,
    pub model_id: String,
    pub enabled: bool,
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
