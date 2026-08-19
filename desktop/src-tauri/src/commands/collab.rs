use openwork_collab::{
    daemon::IpcRequest,
    model::{
        Agent, AgentInput, AgentView, MessagePage, MessagePageAnchor, Room, RoomSummary,
        SendMessageOutcome,
    },
    opencode::PermissionReply,
    permission::PendingPermission,
};
use serde_json::Value;

use crate::{collab_client::CollabDaemonClient, CommandError};

#[tauri::command]
pub async fn collab_status(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Value, CommandError> {
    client
        .call(&IpcRequest::Status)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_agent_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<AgentView>, CommandError> {
    client
        .call(&IpcRequest::ListAgents)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_agent_create(
    client: tauri::State<'_, CollabDaemonClient>,
    agent: AgentInput,
) -> Result<Agent, CommandError> {
    client
        .call(&IpcRequest::CreateAgent { agent })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_agent_update(
    client: tauri::State<'_, CollabDaemonClient>,
    agent: AgentInput,
) -> Result<Agent, CommandError> {
    client
        .call(&IpcRequest::UpdateAgent { agent })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_room_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<RoomSummary>, CommandError> {
    client
        .call(&IpcRequest::ListRooms)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_room_create(
    client: tauri::State<'_, CollabDaemonClient>,
    id: String,
    title: String,
) -> Result<Room, CommandError> {
    client
        .call(&IpcRequest::CreateRoom { id, title })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_room_add_member(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    participant_id: String,
) -> Result<Value, CommandError> {
    client
        .call(&IpcRequest::AddMember {
            room_id,
            participant_id,
        })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_message_send(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    body: String,
) -> Result<SendMessageOutcome, CommandError> {
    client
        .call(&IpcRequest::SendMessage {
            room_id,
            author_id: "user".to_string(),
            body,
        })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_message_page(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    anchor: Option<MessagePageAnchor>,
    limit: u32,
) -> Result<MessagePage, CommandError> {
    client
        .call(&IpcRequest::MessagePage {
            room_id,
            anchor,
            limit,
        })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_room_mark_read(
    client: tauri::State<'_, CollabDaemonClient>,
    room_id: String,
    through_sequence: i64,
) -> Result<Value, CommandError> {
    client
        .call(&IpcRequest::MarkRead {
            room_id,
            through_sequence,
        })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_permission_list(
    client: tauri::State<'_, CollabDaemonClient>,
) -> Result<Vec<PendingPermission>, CommandError> {
    client
        .call(&IpcRequest::Permissions)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_permission_reply(
    client: tauri::State<'_, CollabDaemonClient>,
    id: String,
    reply: PermissionReply,
    message: Option<String>,
) -> Result<Value, CommandError> {
    client
        .call(&IpcRequest::ReplyPermission { id, reply, message })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn collab_permission_abort(
    client: tauri::State<'_, CollabDaemonClient>,
    id: String,
) -> Result<Value, CommandError> {
    client
        .call(&IpcRequest::AbortPermission { id })
        .await
        .map_err(CommandError::from)
}
