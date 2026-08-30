use openwork_collab::protocol::{
    AgentView, BoardView, ComputerView, ControlRequest, ControlResponse, MessageView,
    ParticipantView, RoomView, RunSummaryView,
};

use crate::{collab_client::CollabDaemonClient, CommandError};

#[tauri::command]
pub async fn collab_status(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<ComputerView, CommandError> {
    match client.call(&ControlRequest::EnsureLocalComputer).await? {
        ControlResponse::LocalComputer(registration) => Ok(registration.computer),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_proactivity_set(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: String,
    enabled: bool,
) -> Result<AgentView, CommandError> {
    match client
        .call(&ControlRequest::SetAgentProactivity { agent_id, enabled })
        .await?
    {
        ControlResponse::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<AgentView>, CommandError> {
    match client.call(&ControlRequest::ListAgents).await? {
        ControlResponse::Agents { agents } => Ok(agents),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_agent_create(
    client: tauri::State<'_, CollabDaemonClient>,
    id: String,
    display_name: String,
    system_prompt: String,
    model: String,
) -> Result<AgentView, CommandError> {
    match client
        .call(&ControlRequest::CreateAgent {
            id,
            display_name,
            system_prompt,
            model,
        })
        .await?
    {
        ControlResponse::Agent(agent) => Ok(agent),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_room_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<RoomView>, CommandError> {
    match client.call(&ControlRequest::ListRooms).await? {
        ControlResponse::Rooms { rooms } => Ok(rooms),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_direct_room_create(
    client: tauri::State<'_, CollabDaemonClient>,
    agent_id: String,
) -> Result<RoomView, CommandError> {
    match client
        .call(&ControlRequest::CreateDirectRoom { agent_id })
        .await?
    {
        ControlResponse::Room(room) => Ok(room),
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
        .call(&ControlRequest::CreateGroupRoom { title, agent_ids })
        .await?
    {
        ControlResponse::Room(room) => Ok(room),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_room_member_list(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
) -> Result<Vec<ParticipantView>, CommandError> {
    match client
        .call(&ControlRequest::ListRoomMembers { room_id })
        .await?
    {
        ControlResponse::Members { members } => Ok(members),
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
        .call(&ControlRequest::AddGroupMember { room_id, agent_id })
        .await?
    {
        ControlResponse::Members { members } => Ok(members),
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
        .call(&ControlRequest::RemoveGroupMember { room_id, agent_id })
        .await?
    {
        ControlResponse::Members { members } => Ok(members),
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
        .call(&ControlRequest::SendMessage { room_id, body })
        .await?
    {
        ControlResponse::Message(message) => Ok(message),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_message_list(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
) -> Result<Vec<MessageView>, CommandError> {
    match client
        .call(&ControlRequest::ListMessages { room_id })
        .await?
    {
        ControlResponse::Messages { messages } => Ok(messages),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_board_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<BoardView>, CommandError> {
    match client.call(&ControlRequest::ListBoards).await? {
        ControlResponse::Boards { boards } => Ok(boards),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_board_create(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    title: String,
) -> Result<BoardView, CommandError> {
    match client
        .call(&ControlRequest::CreateBoard { room_id, title })
        .await?
    {
        ControlResponse::Board(board) => Ok(board),
        response => Err(unexpected(response)),
    }
}

#[tauri::command]
pub async fn collab_run_list(
    client: tauri::State<'_, CollabDaemonClient>,
    limit: u32,
) -> Result<Vec<RunSummaryView>, CommandError> {
    match client.call(&ControlRequest::ListRuns { limit }).await? {
        ControlResponse::Runs { runs } => Ok(runs),
        response => Err(unexpected(response)),
    }
}

fn unexpected(response: ControlResponse) -> CommandError {
    let message = match response {
        ControlResponse::Error { message } => message,
        _ => "collaboration Server returned an unexpected response".to_string(),
    };
    CommandError::new(crate::CommandErrorCode::CollaborationUnavailable, message)
}
