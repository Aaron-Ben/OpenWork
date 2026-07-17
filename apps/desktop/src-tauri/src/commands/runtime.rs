use openwork_app::{
    OpenWorkApplication, RuntimeLoadedSession, RuntimeModelInput, RuntimeSessionInput,
    RuntimeSessionRecord, RuntimeSessionSnapshot, RuntimeSessionUpdate,
    RuntimeSessionUpdateEnvelope, RuntimeTraceSpan, RuntimeTraceSummary, RuntimeTurnAccepted,
};
use tauri::Emitter;

use crate::CommandError;

#[tauri::command]
pub async fn runtime_model_upsert(
    application: tauri::State<'_, OpenWorkApplication>,
    input: RuntimeModelInput,
) -> Result<(), CommandError> {
    application
        .runtime()
        .register_model(&input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_list(
    application: tauri::State<'_, OpenWorkApplication>,
) -> Result<Vec<RuntimeSessionRecord>, CommandError> {
    application
        .runtime()
        .list_sessions()
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_create(
    application: tauri::State<'_, OpenWorkApplication>,
    input: RuntimeSessionInput,
) -> Result<RuntimeSessionRecord, CommandError> {
    application
        .runtime()
        .create_session(&input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_load(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
) -> Result<RuntimeLoadedSession, CommandError> {
    application
        .runtime()
        .load_session(&session_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_rename(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
    title: String,
) -> Result<RuntimeSessionRecord, CommandError> {
    application
        .runtime()
        .rename_session(&session_id, &title)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_delete(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
) -> Result<(), CommandError> {
    application
        .runtime()
        .delete_session(&session_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_turn_start(
    app: tauri::AppHandle,
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
    client_request_id: String,
    text: String,
) -> Result<RuntimeTurnAccepted, CommandError> {
    let mut updates = application
        .runtime()
        .subscribe_updates(&session_id)
        .await
        .map_err(CommandError::from)?;
    let accepted = application
        .runtime()
        .start_turn(&session_id, &client_request_id, &text)
        .await
        .map_err(CommandError::from)?;
    let turn_id = accepted.turn_id.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match updates.recv().await {
                Ok(payload) if payload.turn_id == turn_id => {
                    let terminal =
                        matches!(&payload.update, RuntimeSessionUpdate::TurnFinished { .. });
                    let _ = app.emit("session-update", payload);
                    if terminal {
                        break;
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(accepted)
}

#[tauri::command]
pub async fn runtime_turn_cancel(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
    turn_id: String,
) -> Result<bool, CommandError> {
    application
        .runtime()
        .cancel_turn(&session_id, &turn_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_permission_resolve(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
    turn_id: String,
    tool_call_id: String,
    allow: bool,
) -> Result<(), CommandError> {
    application
        .runtime()
        .resolve_permission(&session_id, &turn_id, &tool_call_id, allow)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_snapshot(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
) -> Result<RuntimeSessionSnapshot, CommandError> {
    application
        .runtime()
        .snapshot(&session_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_update_replay(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
    after_sequence: u64,
) -> Result<Vec<RuntimeSessionUpdateEnvelope>, CommandError> {
    application
        .runtime()
        .replay_updates(&session_id, after_sequence)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_list(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: Option<String>,
    limit: i64,
) -> Result<Vec<RuntimeTraceSummary>, CommandError> {
    application
        .runtime()
        .list_traces(session_id.as_deref(), limit)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_get(
    application: tauri::State<'_, OpenWorkApplication>,
    turn_id: String,
) -> Result<Vec<RuntimeTraceSpan>, CommandError> {
    application
        .runtime()
        .get_trace(&turn_id)
        .await
        .map_err(CommandError::from)
}
