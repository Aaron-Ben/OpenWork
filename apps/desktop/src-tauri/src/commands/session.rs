use openwork_app::{OpenWorkApplication, Session, SessionInput, SessionLoadResult, SessionSummary};

use crate::CommandError;

#[tauri::command]
pub async fn session_list(
    application: tauri::State<'_, OpenWorkApplication>,
) -> Result<Vec<SessionSummary>, CommandError> {
    application
        .sessions()
        .list()
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn session_create(
    application: tauri::State<'_, OpenWorkApplication>,
    input: SessionInput,
) -> Result<Session, CommandError> {
    application
        .sessions()
        .create(input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn session_load(
    application: tauri::State<'_, OpenWorkApplication>,
    id: String,
) -> Result<SessionLoadResult, CommandError> {
    application
        .sessions()
        .load(&id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn session_delete(
    application: tauri::State<'_, OpenWorkApplication>,
    id: String,
) -> Result<(), CommandError> {
    application
        .sessions()
        .delete(&id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn session_rename(
    application: tauri::State<'_, OpenWorkApplication>,
    id: String,
    title: String,
) -> Result<Session, CommandError> {
    application
        .sessions()
        .rename(&id, &title)
        .await
        .map_err(CommandError::from)
}
