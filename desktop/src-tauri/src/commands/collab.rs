use openwork_collab::protocol::{
    AgentView, BoardView, DesktopCommand, DesktopCommandResult, MessageView, ParticipantView,
    RoomView, RunSummaryView, RuntimeStatusView,
};
use serde::Deserialize;

use crate::{collab_client::CollabDaemonClient, CommandError};

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
) -> Result<Vec<RoomView>, CommandError> {
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
) -> Result<MessageView, CommandError> {
    match client
        .call(DesktopCommand::SendMessage { room_id, body })
        .await?
    {
        DesktopCommandResult::Message(message) => Ok(message),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_message_list(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
) -> Result<Vec<MessageView>, CommandError> {
    match client
        .call(DesktopCommand::ListMessages { room_id })
        .await?
    {
        DesktopCommandResult::Messages { messages } => Ok(messages),
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
pub async fn collab_run_list(
    client: tauri::State<'_, CollabDaemonClient>,
    limit: u32,
) -> Result<Vec<RunSummaryView>, CommandError> {
    match client.call(DesktopCommand::ListRuns { limit }).await? {
        DesktopCommandResult::Runs { runs } => Ok(runs),
        response => Err(unexpected(response)),
    }
}

fn unexpected(_response: DesktopCommandResult) -> CommandError {
    CommandError::new(
        crate::CommandErrorCode::CollaborationUnavailable,
        "collaboration Server returned an unexpected response",
    )
}
