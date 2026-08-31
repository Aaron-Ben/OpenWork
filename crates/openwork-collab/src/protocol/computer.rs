use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineStatus {
    Unknown,
    Ready,
    Missing,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineInventoryView {
    pub engine_id: String,
    pub status: EngineStatus,
    pub version: Option<String>,
    pub checked_at: i64,
    pub last_error: Option<String>,
    pub observed_session_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineObservation {
    pub engine_id: String,
    pub status: EngineStatus,
    pub version: Option<String>,
    pub checked_at: i64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineInventoryReport {
    pub engines: Vec<EngineObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssignment {
    pub id: String,
    pub display_name: String,
    pub role: Option<String>,
    pub persona: String,
    pub engine_id: String,
    pub main_model_id: String,
    pub triage_model_id: String,
    pub config_revision: i64,
    pub agenda_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesiredAgents {
    pub runtime_session_id: String,
    pub agents: Vec<AgentAssignment>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTokenResponse {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerState {
    Running,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunnerStatusView {
    pub agent_id: String,
    pub config_revision: i64,
    pub state: RunnerState,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineReadinessView {
    pub engine_id: String,
    pub status: EngineStatus,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerHeartbeatRequest {
    pub engine_readiness: Vec<EngineReadinessView>,
    pub runners: Vec<RunnerStatusView>,
}
