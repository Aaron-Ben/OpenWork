use openwork_protocol::model::{ContentBlock, Role};
use serde::{Deserialize, Serialize};

use super::lifecycle::TurnLifecycleSnapshot;

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
    pub working_dir: Option<String>,
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
    /// Aggregate id of the Turn that recorded this message.
    pub turn_id: String,
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
    pub turns: Vec<TurnLifecycleSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed,
    Cancelled,
    DoomLoop { repeated: String },
    Failed { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_summary_exposes_the_bound_working_directory() {
        let summary = SessionSummary {
            id: "session-1".to_string(),
            title: "Conversation".to_string(),
            provider_id: "provider-1".to_string(),
            model: "model-1".to_string(),
            working_dir: Some("/Volumes/Code/OpenWork".to_string()),
            updated_at: 1,
        };

        let value = serde_json::to_value(summary).unwrap();
        assert_eq!(value["workingDir"], "/Volumes/Code/OpenWork");
    }
}
