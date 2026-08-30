use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlRequest {
    EnsureLocalComputer,
    ListAgents,
    ListRooms,
    CreateAgent {
        id: String,
        display_name: String,
        system_prompt: String,
        model: String,
    },
    CreateDirectRoom {
        agent_id: String,
    },
    SendMessage {
        room_id: String,
        body: String,
    },
    ListMessages {
        room_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlResponse {
    LocalComputer(LocalComputerRegistration),
    Agent(AgentView),
    Agents { agents: Vec<AgentView> },
    Room(RoomView),
    Rooms { rooms: Vec<RoomView> },
    Message(MessageView),
    Messages { messages: Vec<MessageView> },
    Error { message: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalComputerRegistration {
    pub computer: ComputerView,
    pub runtime_base_url: String,
    pub device_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ComputerView {
    pub id: String,
    pub name: String,
    pub status: ComputerStatus,
    pub engine_id: String,
    pub engine_status: EngineStatus,
    pub generation: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerStatus {
    Online,
    Offline,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineStatus {
    Unknown,
    Ready,
    Missing,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: String,
    pub display_name: String,
    pub system_prompt: String,
    pub engine_id: String,
    pub model: String,
    pub config_version: i64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoomView {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: String,
    pub room_id: String,
    pub sequence: i64,
    pub author_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStartResponse {
    pub generation: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRequest {
    pub protocol_version: u32,
    pub generation: i64,
    pub daemon_version: String,
    pub supervised: bool,
    pub status: ComputerStatus,
    pub engine: EngineInventoryView,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineInventoryView {
    pub engine_id: String,
    pub status: EngineStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssignment {
    pub id: String,
    pub display_name: String,
    pub role: Option<String>,
    pub bio: Option<String>,
    pub system_prompt: String,
    pub engine_id: String,
    pub model: String,
    pub fast_model: String,
    pub config_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentRoster {
    pub agents: Vec<AgentAssignment>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTokenResponse {
    pub token: String,
    pub expires_at: i64,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriagePayload {
    pub verdict: Option<TriageVerdict>,
    pub instructions: Option<String>,
    pub input: Option<String>,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriageVerdict {
    pub actionable: bool,
    pub reason: String,
    pub prompt_note: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriageReportRequest {
    pub run_id: String,
    pub verdict: TriageVerdict,
    pub model: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxResponse {
    pub trigger: Option<TriggerEnvelope>,
    pub messages: Vec<MessageView>,
    pub carried_over: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEnvelope {
    pub protocol_version: u32,
    pub dispatch_id: String,
    pub agent_id: String,
    pub computer_id: String,
    pub computer_generation: i64,
    pub trigger: String,
    pub deliveries: Vec<DeliveryRange>,
    pub carried_over: bool,
    pub issued_at: i64,
    pub expires_at: i64,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRange {
    pub room_id: String,
    pub from_seq: i64,
    pub up_to_seq: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRunRequest {
    pub trigger: TriggerEnvelope,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub status: String,
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliRequest {
    pub request_id: String,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliResult {
    pub text: String,
    pub exit_code: i32,
    pub side_effects: Vec<CliSideEffect>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliSideEffect {
    MessagePublished {
        room_id: String,
        message_id: String,
        sequence: i64,
    },
    InboxAcknowledged {
        room_id: String,
        up_to_seq: i64,
    },
    ReactionChanged {
        message_id: String,
        emoji: String,
        active: bool,
    },
    DirectRoomOpened {
        room_id: String,
        participant_id: String,
    },
    GroupRoomCreated {
        room_id: String,
    },
    MembershipChanged {
        room_id: String,
        participant_id: String,
        change: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FinishRunRequest {
    pub status: String,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub assistant_text: Option<String>,
}

pub const CLI_MESSAGE_BODY_MAX_BYTES: usize = 1024 * 1024;
pub const COLLAB_PROTOCOL_VERSION: u32 = 3;
