use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvalidationEvent {
    pub id: String,
    pub kind: InvalidationKind,
    pub subject_id: Option<String>,
    pub revision: Option<i64>,
    pub published_at: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationKind {
    RuntimeReady,
    AgentConfig,
    Message,
    EngineInventory,
    RunnerStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WakeEvent {
    pub id: String,
    pub agent_id: String,
    pub message_id: String,
    pub room_id: String,
    pub reason: String,
    pub published_at: i64,
}
