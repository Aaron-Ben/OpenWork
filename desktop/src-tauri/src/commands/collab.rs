use openwork_collab::protocol::{
    AgentView, BoardView, CardChangeView, ColumnKind, DesktopCommand, DesktopCommandResult,
    MessageView, ParticipantView, RoomSnapshotView, RoomSummaryView, RoomView, RunSummaryView,
    RunTraceView, RuntimeStatusView,
};
use serde::Deserialize;

use crate::{collab_client::CollabDaemonClient, CommandError};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabCardCreateInput {
    board_id: String,
    column_id: String,
    title: String,
    description: Option<String>,
    assignee_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabAgentUpdateInput {
    agent_id: String,
    display_name: String,
    role: Option<String>,
    persona: String,
    engine_id: String,
    main_model_id: String,
    triage_model_id: String,
}

#[tauri::command]
pub async fn collab_status(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<RuntimeStatusView, CommandError> {
    match client.call(DesktopCommand::Status).await? {
        DesktopCommandResult::Status(status) => Ok(status),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_agenda_set(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: String,
    enabled: bool,
) -> Result<AgentView, CommandError> {
    match client
        .call(DesktopCommand::SetAgentAgenda { agent_id, enabled })
        .await?
    {
        DesktopCommandResult::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<AgentView>, CommandError> {
    match client.call(DesktopCommand::ListAgents).await? {
        DesktopCommandResult::Agents { agents } => Ok(agents),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_create(
    client: tauri::State<'_, CollabDaemonClient>,
    display_name: String,
    role: Option<String>,
    persona: String,
    engine_id: String,
    main_model_id: String,
    triage_model_id: String,
) -> Result<AgentView, CommandError> {
    match client
        .call(DesktopCommand::CreateAgent {
            display_name,
            role,
            persona,
            engine_id,
            main_model_id,
            triage_model_id,
        })
        .await?
    {
        DesktopCommandResult::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_update(
    client: tauri::State<'_, CollabDaemonClient>,
    input: CollabAgentUpdateInput,
) -> Result<AgentView, CommandError> {
    let CollabAgentUpdateInput {
        agent_id,
        display_name,
        role,
        persona,
        engine_id,
        main_model_id,
        triage_model_id,
    } = input;
    match client
        .call(DesktopCommand::UpdateAgent {
            agent_id,
            display_name,
            role,
            persona,
            engine_id,
            main_model_id,
            triage_model_id,
        })
        .await?
    {
        DesktopCommandResult::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_archive(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: String,
) -> Result<AgentView, CommandError> {
    match client
        .call(DesktopCommand::ArchiveAgent { agent_id })
        .await?
    {
        DesktopCommandResult::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_restore(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: String,
) -> Result<AgentView, CommandError> {
    match client
        .call(DesktopCommand::RestoreAgent { agent_id })
        .await?
    {
        DesktopCommandResult::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_room_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<RoomSummaryView>, CommandError> {
    match client.call(DesktopCommand::ListRooms).await? {
        DesktopCommandResult::Rooms { rooms } => Ok(rooms),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_direct_room_create(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: String,
) -> Result<RoomView, CommandError> {
    match client
        .call(DesktopCommand::CreateDirectRoom { agent_id })
        .await?
    {
        DesktopCommandResult::Room(room) => Ok(room),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_group_room_create(
    client: tauri::State<'_, CollabDaemonClient>,
    title: String,
    agent_ids: Vec<String>,
) -> Result<RoomView, CommandError> {
    match client
        .call(DesktopCommand::CreateGroupRoom { title, agent_ids })
        .await?
    {
        DesktopCommandResult::Room(room) => Ok(room),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_room_member_list(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
) -> Result<Vec<ParticipantView>, CommandError> {
    match client
        .call(DesktopCommand::ListRoomMembers { room_id })
        .await?
    {
        DesktopCommandResult::Members { members } => Ok(members),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_group_member_add(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    agent_id: String,
) -> Result<Vec<ParticipantView>, CommandError> {
    match client
        .call(DesktopCommand::AddGroupMember { room_id, agent_id })
        .await?
    {
        DesktopCommandResult::Members { members } => Ok(members),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_group_member_remove(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    agent_id: String,
) -> Result<Vec<ParticipantView>, CommandError> {
    match client
        .call(DesktopCommand::RemoveGroupMember { room_id, agent_id })
        .await?
    {
        DesktopCommandResult::Members { members } => Ok(members),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_message_send(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    body: String,
    quoted_message_id: Option<String>,
) -> Result<MessageView, CommandError> {
    match client
        .call(DesktopCommand::SendMessage {
            room_id,
            body,
            quoted_message_id,
        })
        .await?
    {
        DesktopCommandResult::Message(message) => Ok(message),
        response => Err(unexpected(response)),
    }
}

/// 用户看到了 `room_id` 到 `up_to_seq` 为止的消息；返回 Server 记录后的位置（只增不减）。
#[tauri::command]
pub async fn collab_room_viewed(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    up_to_seq: i64,
) -> Result<i64, CommandError> {
    match client
        .call(DesktopCommand::RoomViewed { room_id, up_to_seq })
        .await?
    {
        DesktopCommandResult::RoomViewed {
            user_viewed_seq, ..
        } => Ok(user_viewed_seq),
        response => Err(unexpected(response)),
    }
}

/// 房间快照：消息、Agent 成员及其当前状态、说明行（collaboration-desktop.md §4.2）。
#[tauri::command]
pub async fn collab_room_open(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
) -> Result<RoomSnapshotView, CommandError> {
    match client.call(DesktopCommand::OpenRoom { room_id }).await? {
        DesktopCommandResult::RoomSnapshot(snapshot) => Ok(*snapshot),
        response => Err(unexpected(response)),
    }
}

/// 置顶或取消置顶一个房间（collaboration-desktop.md §4.2）。
#[tauri::command]
pub async fn collab_room_pin(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    pinned: bool,
) -> Result<(), CommandError> {
    match client
        .call(DesktopCommand::PinRoom { room_id, pinned })
        .await?
    {
        DesktopCommandResult::RoomPinned { .. } => Ok(()),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_board_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<BoardView>, CommandError> {
    match client.call(DesktopCommand::ListBoards).await? {
        DesktopCommandResult::Boards { boards } => Ok(boards),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_board_create(
    client: tauri::State<'_, CollabDaemonClient>,
    title: String,
    description: Option<String>,
) -> Result<BoardView, CommandError> {
    match client
        .call(DesktopCommand::CreateBoard { title, description })
        .await?
    {
        DesktopCommandResult::Board(board) => Ok(board),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_board_update(
    client: tauri::State<'_, CollabDaemonClient>,
    board_id: String,
    title: String,
    description: Option<String>,
) -> Result<BoardView, CommandError> {
    match client
        .call(DesktopCommand::UpdateBoard {
            board_id,
            title,
            description,
        })
        .await?
    {
        DesktopCommandResult::Board(board) => Ok(board),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_board_delete(
    client: tauri::State<'_, CollabDaemonClient>,
    board_id: String,
) -> Result<String, CommandError> {
    deleted(
        client
            .call(DesktopCommand::DeleteBoard { board_id })
            .await?,
    )
}

#[tauri::command]
pub async fn collab_board_column_create(
    client: tauri::State<'_, CollabDaemonClient>,
    board_id: String,
    title: String,
    kind: Option<ColumnKind>,
) -> Result<BoardView, CommandError> {
    board_result(
        client
            .call(DesktopCommand::CreateBoardColumn {
                board_id,
                title,
                kind,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn collab_board_column_update(
    client: tauri::State<'_, CollabDaemonClient>,
    column_id: String,
    title: String,
    kind: Option<ColumnKind>,
) -> Result<BoardView, CommandError> {
    board_result(
        client
            .call(DesktopCommand::UpdateBoardColumn {
                column_id,
                title,
                kind,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn collab_board_column_move(
    client: tauri::State<'_, CollabDaemonClient>,
    column_id: String,
    before_column_id: Option<String>,
) -> Result<BoardView, CommandError> {
    board_result(
        client
            .call(DesktopCommand::MoveBoardColumn {
                column_id,
                before_column_id,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn collab_board_column_delete(
    client: tauri::State<'_, CollabDaemonClient>,
    column_id: String,
) -> Result<BoardView, CommandError> {
    board_result(
        client
            .call(DesktopCommand::DeleteBoardColumn { column_id })
            .await?,
    )
}

#[tauri::command]
pub async fn collab_card_assign(
    client: tauri::State<'_, CollabDaemonClient>,
    card_id: String,
    assignee_id: Option<String>,
) -> Result<CardChangeView, CommandError> {
    card_change(
        &client,
        DesktopCommand::AssignCard {
            card_id,
            assignee_id,
        },
    )
    .await
}

/// Desktop 用户建卡（collaboration.md §11.2）；指定负责人与描述里的 `@` 会叫醒对应的 Agent。
#[tauri::command]
pub async fn collab_card_create(
    client: tauri::State<'_, CollabDaemonClient>,
    input: CollabCardCreateInput,
) -> Result<CardChangeView, CommandError> {
    let CollabCardCreateInput {
        board_id,
        column_id,
        title,
        description,
        assignee_id,
    } = input;
    card_change(
        &client,
        DesktopCommand::CreateCard {
            board_id,
            column_id,
            title,
            description,
            assignee_id,
        },
    )
    .await
}

/// 标题与描述都可选、至少给一个；描述写空字符串即清空。
#[tauri::command]
pub async fn collab_card_update(
    client: tauri::State<'_, CollabDaemonClient>,
    card_id: String,
    title: Option<String>,
    description: Option<String>,
) -> Result<CardChangeView, CommandError> {
    card_change(
        &client,
        DesktopCommand::UpdateCard {
            card_id,
            title,
            description,
        },
    )
    .await
}

/// 移到 `column_id`，放在 `before_card_id` 之前；不给时放到末尾。
#[tauri::command]
pub async fn collab_card_move(
    client: tauri::State<'_, CollabDaemonClient>,
    card_id: String,
    column_id: String,
    before_card_id: Option<String>,
) -> Result<CardChangeView, CommandError> {
    card_change(
        &client,
        DesktopCommand::MoveCard {
            card_id,
            column_id,
            before_card_id,
        },
    )
    .await
}

async fn card_change(
    client: &CollabDaemonClient,
    command: DesktopCommand,
) -> Result<CardChangeView, CommandError> {
    match client.call(command).await? {
        DesktopCommandResult::Card(change) => Ok(change),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_card_delete(
    client: tauri::State<'_, CollabDaemonClient>,
    card_id: String,
) -> Result<String, CommandError> {
    deleted(client.call(DesktopCommand::DeleteCard { card_id }).await?)
}

#[tauri::command]
pub async fn collab_run_list(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: Option<String>,
    status: Option<String>,
    limit: u32,
) -> Result<Vec<RunSummaryView>, CommandError> {
    match client
        .call(DesktopCommand::ListRuns {
            agent_id,
            status,
            limit,
        })
        .await?
    {
        DesktopCommandResult::Runs { runs } => Ok(runs),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_run_trace(
    client: tauri::State<'_, CollabDaemonClient>,
    run_id: String,
) -> Result<RunTraceView, CommandError> {
    match client.call(DesktopCommand::GetRunTrace { run_id }).await? {
        DesktopCommandResult::RunTrace(trace) => Ok(*trace),
        response => Err(unexpected(response)),
    }
}

fn unexpected(_response: DesktopCommandResult) -> CommandError {
    CommandError::new(
        crate::CommandErrorCode::CollaborationUnavailable,
        "collaboration Server returned an unexpected response",
    )
}

fn board_result(response: DesktopCommandResult) -> Result<BoardView, CommandError> {
    match response {
        DesktopCommandResult::Board(board) => Ok(board),
        response => Err(unexpected(response)),
    }
}

fn deleted(response: DesktopCommandResult) -> Result<String, CommandError> {
    match response {
        DesktopCommandResult::Deleted { entity_id } => Ok(entity_id),
        response => Err(unexpected(response)),
    }
}
