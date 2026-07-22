use openwork_core::{
    session::TurnId, ClientRequestId, ContextWindowInspection, LoadedSession, OpenWorkCore,
    PermissionDecision, ReapplyFileChangesResult, SessionId, SessionInput, SessionRecord,
    SessionSnapshot, SessionUpdateEnvelope, ToolCallId, TraceTurnSummary, TurnAccepted, TurnTrace,
    UndoFileChangesResult,
};
use openwork_models::model::ContentBlock;

use crate::CommandError;

#[tauri::command]
pub async fn runtime_session_list(
    core: tauri::State<'_, OpenWorkCore>,
) -> Result<Vec<SessionRecord>, CommandError> {
    core.list_sessions().await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_create(
    core: tauri::State<'_, OpenWorkCore>,
    input: SessionInput,
) -> Result<SessionRecord, CommandError> {
    core.create_session(&input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_load(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<LoadedSession, CommandError> {
    core.load_session(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_context_window_inspect(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<ContextWindowInspection, CommandError> {
    core.inspect_context_window(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_rename(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    title: String,
) -> Result<SessionRecord, CommandError> {
    core.rename_session(&SessionId::new(session_id), &title)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_delete(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<(), CommandError> {
    core.delete_session(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_turn_start(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    client_request_id: String,
    text: String,
) -> Result<TurnAccepted, CommandError> {
    let session_id = SessionId::new(session_id);
    core.start_turn(
        &session_id,
        ClientRequestId::new(client_request_id),
        vec![ContentBlock::text(text)],
    )
    .await
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_turn_cancel(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    turn_id: String,
) -> Result<bool, CommandError> {
    core.cancel_turn(&SessionId::new(session_id), TurnId::new(turn_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_file_changes_undo(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    change_ids: Vec<String>,
) -> Result<UndoFileChangesResult, CommandError> {
    core.undo_file_changes(&SessionId::new(session_id), change_ids)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_file_changes_reapply(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    change_ids: Vec<String>,
) -> Result<ReapplyFileChangesResult, CommandError> {
    core.reapply_file_changes(&SessionId::new(session_id), change_ids)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_permission_resolve(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    turn_id: String,
    tool_call_id: String,
    allow: bool,
) -> Result<(), CommandError> {
    core.resolve_permission(
        &SessionId::new(session_id),
        TurnId::new(turn_id),
        ToolCallId::new(tool_call_id),
        if allow {
            PermissionDecision::Allow
        } else {
            PermissionDecision::Deny
        },
    )
    .await
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_snapshot(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<SessionSnapshot, CommandError> {
    core.get_session_snapshot(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_update_replay(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    after_sequence: u64,
) -> Result<Vec<SessionUpdateEnvelope>, CommandError> {
    core.replay_updates(&SessionId::new(session_id), after_sequence)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_list(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: Option<String>,
    limit: i64,
) -> Result<Vec<TraceTurnSummary>, CommandError> {
    let session_id = session_id.map(SessionId::new);
    core.list_traces(session_id.as_ref(), limit)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_get(
    core: tauri::State<'_, OpenWorkCore>,
    turn_id: String,
) -> Result<TurnTrace, CommandError> {
    core.get_trace(&TurnId::new(turn_id))
        .await
        .map_err(CommandError::from)
}
