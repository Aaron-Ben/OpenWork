use openwork_protocol::model::{ContentBlock, Role};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model: String,
    pub updated_at: i64,
}

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

#[derive(Debug, Clone)]
pub struct NewMessage {
    pub role: Role,
    pub parts: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    pub session_id: String,
    pub role: Role,
    pub parts: Vec<ContentBlock>,
    pub seq: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadResult {
    pub session: Session,
    pub messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed,
    Cancelled,
    DoomLoop { repeated: String },
    Failed { message: String },
}
