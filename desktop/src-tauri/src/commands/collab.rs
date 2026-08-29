use openwork_collab::protocol::{
    AgentView, ComputerView, ControlRequest, ControlResponse, MessageView, RoomView,
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
) -> Result<AgentView, CommandError> {
    match client
        .call(&ControlRequest::CreateAgent {
            id,
            display_name,
            system_prompt,
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

fn unexpected(response: ControlResponse) -> CommandError {
    let message = match response {
        ControlResponse::Error { message } => message,
        _ => "collaboration Server returned an unexpected response".to_string(),
    };
    CommandError::new(crate::CommandErrorCode::CollaborationUnavailable, message)
}
