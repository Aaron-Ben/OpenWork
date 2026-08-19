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
pub struct Inbox {
    pub messages: Vec<Message>,
    pub unread_count: u64,
}
