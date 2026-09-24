use serde::{Deserialize, Serialize};

use super::{
    EngineInventoryView, EngineReadinessView, RoomSnapshotView, RoomSummaryView, RunnerStatusView,
};

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
        quoted_message_id: Option<String>,
    },
    /// 房间快照（collaboration-desktop.md §4.2）。
    OpenRoom {
        room_id: String,
    },
    /// 置顶或取消置顶（collaboration-desktop.md §4.5）。
    PinRoom {
        room_id: String,
        pinned: bool,
    },
    /// 用户在 Desktop 中看到了 `room_id` 到 `up_to_seq` 为止的消息（collaboration.md §8.3）。
    RoomViewed {
        room_id: String,
        up_to_seq: i64,
    },
    ListBoards,
    CreateBoard {
        title: String,
        description: Option<String>,
    },
    UpdateBoard {
        board_id: String,
        title: String,
        description: Option<String>,
    },
    DeleteBoard {
        board_id: String,
    },
    CreateBoardColumn {
        board_id: String,
        title: String,
        kind: Option<ColumnKind>,
    },
    UpdateBoardColumn {
        column_id: String,
        title: String,
        kind: Option<ColumnKind>,
    },
    MoveBoardColumn {
        column_id: String,
        before_column_id: Option<String>,
    },
    DeleteBoardColumn {
        column_id: String,
    },
    AssignCard {
        card_id: String,
        assignee_id: Option<String>,
    },
    DeleteCard {
        card_id: String,
    },
    ListRuns {
        agent_id: Option<String>,
        status: Option<String>,
        limit: u32,
    },
    GetRunTrace {
        run_id: String,
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
                | Self::OpenRoom { .. }
                // 只增不减的写入，天然幂等；Desktop 每次看到新消息都上报，不进幂等账本。
                | Self::RoomViewed { .. }
                | Self::ListBoards
                | Self::ListRuns { .. }
                | Self::GetRunTrace { .. }
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
    Agents {
        agents: Vec<AgentView>,
    },
    Room(RoomView),
    Rooms {
        rooms: Vec<RoomSummaryView>,
    },
    RoomSnapshot(Box<RoomSnapshotView>),
    RoomPinned {
        room_id: String,
        pinned: bool,
    },
    Members {
        members: Vec<ParticipantView>,
    },
    Message(MessageView),
    RoomViewed {
        room_id: String,
        user_viewed_seq: i64,
    },
    Board(BoardView),
    Boards {
        boards: Vec<BoardView>,
    },
    Card(CardView),
    Deleted {
        entity_id: String,
    },
    Runs {
        runs: Vec<RunSummaryView>,
    },
    RunTrace(Box<RunTraceView>),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatusView {
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
    pub activity: AgentActivity,
}

/// Agent 现在在做什么（collaboration-desktop.md §4.1）。时间都是带 `+08:00` 的 RFC 3339。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentActivity {
    Working {
        room_id: Option<String>,
        room_title: Option<String>,
        card_id: Option<String>,
        card_title: Option<String>,
        started_at: String,
    },
    Queued {
        card_count: i64,
        first_card_title: String,
    },
    Error {
        message: String,
    },
    Idle {
        room_id: Option<String>,
        room_title: Option<String>,
        last_spoke_at: Option<String>,
    },
    Archived,
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
    /// 这条消息引用的同一房间内的消息（collaboration.md §9.3）。
    pub quoted: Option<QuotedMessageView>,
}

/// 被引用消息的摘要；`body` 最多 180 个字符。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuotedMessageView {
    pub id: String,
    pub author_id: String,
    pub author_name: String,
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
    pub kind: Option<ColumnKind>,
    pub cards: Vec<CardView>,
}

/// Column 的语义（collaboration.md §11.1）；列名可以随意改，领取与 Agenda 只看它。
/// `None` 表示未分类：领取不会把卡片移出未分类列。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Todo,
    Doing,
    Done,
}

impl ColumnKind {
    /// 数据库 `collab_board_columns.kind` 中的取值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
        }
    }

    /// 解析数据库取值；迁移的 CHECK 约束保证只会出现这三种。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "todo" => Some(Self::Todo),
            "doing" => Some(Self::Doing),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
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
    /// 负责人对这张卡片的当前状态，只在 Desktop 读取看板时计算；Agent 命令的输出不带它
    /// （collaboration-desktop.md §4.3）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<CardAgentState>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CardAgentState {
    Working,
    Queued,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryView {
    pub id: String,
    pub agent_id: String,
    pub runtime_session_id: String,
    pub trigger: String,
    pub status: String,
    pub engine_id: String,
    pub main_model_id: String,
    pub observed_model_id: Option<String>,
    pub outcome: Option<String>,
    pub room_id: Option<String>,
    pub focus_card_id: Option<String>,
    pub trigger_reason: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub stage: String,
    pub started_at: String,
    pub heartbeat_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: i64,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub rate_limit_percent: Option<f64>,
    pub tool_calls: i64,
    pub event_count: i64,
    pub inbox_message_count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunTraceView {
    pub run: RunSummaryView,
    pub events: Vec<RunEventView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunEventView {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub level: String,
    pub data: serde_json::Value,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// collaboration-desktop.md §4.1：`activity` 以 `kind` 区分，枚举值 snake_case、字段 camelCase；
    /// 与 `desktop/src/bridge/collab.ts` 的 `CollabAgentActivity` 同形。
    #[test]
    fn agent_activity_serializes_as_a_kind_tagged_camel_case_object() {
        let working = AgentActivity::Working {
            room_id: Some("room-1".to_string()),
            room_title: Some("Release".to_string()),
            card_id: None,
            card_title: None,
            started_at: "2026-09-25T10:07:14+08:00".to_string(),
        };
        assert_eq!(
            serde_json::to_value(working).unwrap(),
            json!({
                "kind": "working",
                "roomId": "room-1",
                "roomTitle": "Release",
                "cardId": null,
                "cardTitle": null,
                "startedAt": "2026-09-25T10:07:14+08:00",
            })
        );
        let queued = AgentActivity::Queued {
            card_count: 2,
            first_card_title: "Fix login".to_string(),
        };
        assert_eq!(
            serde_json::to_value(queued).unwrap(),
            json!({ "kind": "queued", "cardCount": 2, "firstCardTitle": "Fix login" })
        );
        let idle = AgentActivity::Idle {
            room_id: None,
            room_title: None,
            last_spoke_at: None,
        };
        assert_eq!(
            serde_json::to_value(idle).unwrap(),
            json!({ "kind": "idle", "roomId": null, "roomTitle": null, "lastSpokeAt": null })
        );
        assert_eq!(
            serde_json::to_value(AgentActivity::Error {
                message: "Engine missing".to_string()
            })
            .unwrap(),
            json!({ "kind": "error", "message": "Engine missing" })
        );
        assert_eq!(
            serde_json::to_value(AgentActivity::Archived).unwrap(),
            json!({ "kind": "archived" })
        );
    }

    /// collaboration-desktop.md §4.3：没有状态的卡片不带 `agentState` 字段，有状态时为 snake_case。
    #[test]
    fn card_agent_state_is_omitted_unless_the_desktop_computed_it() {
        let card = CardView {
            id: "card-1".to_string(),
            board_id: "board-1".to_string(),
            column_id: "column-1".to_string(),
            title: "Fix login".to_string(),
            description: None,
            position: 0,
            assignee_id: Some("bo".to_string()),
            created_by: "local-user".to_string(),
            agent_state: None,
        };
        let plain = serde_json::to_value(&card).unwrap();
        assert!(plain.get("agentState").is_none(), "{plain}");
        let queued = serde_json::to_value(CardView {
            agent_state: Some(CardAgentState::Queued),
            ..card
        })
        .unwrap();
        assert_eq!(queued["agentState"], json!("queued"));
    }
}
