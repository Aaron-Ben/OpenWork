use openwork_app::{ChatGenerateRequest, ChatGenerateResponse, OpenWorkApplication};
use tauri::Emitter;

use crate::CommandError;

#[tauri::command]
pub async fn chat_generate_stream(
    app: tauri::AppHandle,
    application: tauri::State<'_, OpenWorkApplication>,
    request: ChatGenerateRequest,
) -> Result<ChatGenerateResponse, CommandError> {
    application
        .turns()
        .generate_stream(request, move |payload| {
            let _ = app.emit("chat-stream-event", payload);
        })
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn resolve_approval(
    application: tauri::State<'_, OpenWorkApplication>,
    turn_id: String,
    approval_id: String,
    allow: bool,
) -> Result<(), CommandError> {
    application
        .turns()
        .resolve_approval(turn_id, approval_id, allow)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn chat_abort(
    application: tauri::State<'_, OpenWorkApplication>,
    request_id: String,
) -> Result<bool, CommandError> {
    Ok(application.turns().cancel(&request_id))
}
