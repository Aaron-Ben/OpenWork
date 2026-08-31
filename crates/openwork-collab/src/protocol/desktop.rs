use serde::{Deserialize, Serialize};

use super::{EngineInventoryView, EngineReadinessView, RunnerStatusView};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProcessBootstrap {
    pub runtime_session_id: String,
    pub desktop_secret: String,
    pub computer_secret: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerProcessReady {
    pub protocol_version: u32,
    pub runtime_session_id: String,
    pub base_url: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerProcessBootstrap {
    pub runtime_session_id: String,
    pub base_url: String,
    pub computer_secret: String,
    pub openwork_root: String,
    pub shim_executable: String,
    pub engine_executable: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerProcessReady {
    pub protocol_version: u32,
    pub runtime_session_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCommandRequest {
    pub request_id: Option<String>,
    pub command: DesktopCommand,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DesktopCommand {
    Status,
    ListAgents,
    CreateAgent {
        display_name: String,
        role: Option<String>,
        persona: String,
        engine_id: String,
        main_model_id: String,
        triage_model_id: String,
    },
    SetAgentAgenda {
        agent_id: String,
        enabled: bool,
    },
    UpdateAgent {
        agent_id: String,
        display_name: String,
        role: Option<String>,
        persona: String,
        engine_id: String,
        main_model_id: String,
        triage_model_id: String,
    },
    ArchiveAgent {
        agent_id: String,
    },
    RestoreAgent {
        agent_id: String,
    },
    ListRooms,
    CreateDirectRoom {
        agent_id: String,
    },
    CreateGroupRoom {
        title: String,
        agent_ids: Vec<String>,
    },
    ListRoomMembers {
        room_id: String,
    },
    AddGroupMember {
        room_id: String,
        agent_id: String,
    },
    RemoveGroupMember {
        room_id: String,
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
        title: String,
        description: Option<String>,
    },
    ListRuns {
        limit: u32,
    },
}

impl DesktopCommand {
    pub fn is_mutating(&self) -> bool {
        !matches!(
            self,
            Self::Status
                | Self::ListAgents
                | Self::ListRooms
                | Self::ListRoomMembers { .. }
                | Self::ListMessages { .. }
                | Self::ListBoards
                | Self::ListRuns { .. }
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DesktopCommandResult {
    Status(RuntimeStatusView),
    Agent(AgentView),
    Agents { agents: Vec<AgentView> },
    Room(RoomView),
    Rooms { rooms: Vec<RoomView> },
    Members { members: Vec<ParticipantView> },
    Message(MessageView),
    Messages { messages: Vec<MessageView> },
    Board(BoardView),
    Boards { boards: Vec<BoardView> },
    Runs { runs: Vec<RunSummaryView> },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatusView {
    pub protocol_version: u32,
    pub runtime_session_id: String,
    pub started_at: i64,
    pub last_computer_heartbeat: Option<i64>,
    pub engines: Vec<EngineInventoryView>,
    pub engine_readiness: Vec<EngineReadinessView>,
    pub runners: Vec<RunnerStatusView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: String,
    pub display_name: String,
    pub role: Option<String>,
    pub persona: String,
    pub engine_id: String,
    pub main_model_id: String,
    pub triage_model_id: String,
    pub config_revision: i64,
    pub agenda_enabled: bool,
    pub archived_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantView {
    pub id: String,
    pub kind: String,
    pub display_name: String,
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
    pub title: String,
    pub description: Option<String>,
    pub created_by: String,
    pub columns: Vec<BoardColumnView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardColumnView {
    pub id: String,
    pub title: String,
    pub position: i32,
    pub is_terminal: bool,
    pub cards: Vec<CardView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub id: String,
    pub board_id: String,
    pub column_id: String,
    pub title: String,
    pub description: Option<String>,
    pub position: i32,
    pub assignee_id: Option<String>,
    pub created_by: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryView {
    pub id: String,
    pub agent_id: String,
    pub runtime_session_id: String,
    pub trigger: String,
    pub status: String,
    pub engine_id: String,
    pub main_model_id: String,
    pub outcome: Option<String>,
    pub room_id: Option<String>,
    pub focus_card_id: Option<String>,
    pub trigger_reason: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
}
