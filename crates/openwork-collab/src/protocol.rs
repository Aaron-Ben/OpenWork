use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    EnsureLocalComputer,
    ListAgents,
    ListRooms,
    CreateAgent {
        id: String,
        display_name: String,
        system_prompt: String,
        model: String,
    },
    SetAgentProactivity {
        agent_id: String,
        enabled: bool,
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
    ListBoards,
    CreateBoard {
        room_id: String,
        title: String,
    },
    ListRuns {
        limit: u32,
    },
    ShutdownServer,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlResponse {
    Status { protocol_version: u32 },
    LocalComputer(LocalComputerRegistration),
    Agent(AgentView),
    Agents { agents: Vec<AgentView> },
    Room(RoomView),
    Rooms { rooms: Vec<RoomView> },
    Message(MessageView),
    Messages { messages: Vec<MessageView> },
    Board(BoardView),
    Boards { boards: Vec<BoardView> },
    Runs { runs: Vec<RunSummaryView> },
    Acknowledged,
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
    pub scanner_enabled: bool,
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
pub struct BoardView {
    pub id: String,
    pub room_id: String,
    pub title: String,
    pub columns: Vec<BoardColumnView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardColumnView {
    pub id: String,
    pub title: String,
    pub position: i32,
    pub is_done: bool,
    pub cards: Vec<CardView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub position: i32,
    pub assignee_id: Option<String>,
    pub claimed_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryView {
    pub id: String,
    pub agent_id: String,
    pub trigger: String,
    pub status: String,
    pub outcome: Option<String>,
    pub room_id: Option<String>,
    pub focus_card_id: Option<String>,
    pub trigger_reason: Option<String>,
    pub started_at: String,
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
    pub scanner_enabled: bool,
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
    pub agenda_focus: Option<AgendaFocus>,
    pub carried_over: bool,
    pub issued_at: i64,
    pub expires_at: i64,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaFocus {
    pub room_id: String,
    pub card_id: Option<String>,
    pub room_sequence: i64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgendaCandidate {
    AssignedCard {
        candidate_id: String,
        card_id: String,
        room_id: String,
        title: String,
        column: String,
        assignment: String,
        updated_at: i64,
        room_sequence: i64,
        recent_context: Vec<MessageView>,
    },
    StalledRoom {
        candidate_id: String,
        room_id: String,
        last_sequence: i64,
        last_activity_at: i64,
        open_cards: Vec<String>,
        recent_context: Vec<MessageView>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaCandidateSet {
    pub id: String,
    pub agent_id: String,
    pub computer_generation: i64,
    pub candidates: Vec<AgendaCandidate>,
    pub issued_at: i64,
    pub expires_at: i64,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaPayload {
    pub candidate_set: AgendaCandidateSet,
    pub classify_prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "decision",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgendaDecision {
    Act {
        candidate_id: String,
        reason: String,
    },
    Decline {
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaDecisionRequest {
    pub candidate_set: AgendaCandidateSet,
    pub decision: AgendaDecision,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaDecisionResponse {
    pub trigger: Option<TriggerEnvelope>,
    pub focused_brief: Option<String>,
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
    CardCreated {
        board_id: String,
        card_id: String,
    },
    CardClaimed {
        card_id: String,
        claimed_by: String,
    },
    CardMoved {
        card_id: String,
        column_id: String,
        position: i32,
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
pub const COLLAB_PROTOCOL_VERSION: u32 = 6;
