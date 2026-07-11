use openwork_persistence::{
    Session, SessionInput, SessionLoadResult, SessionStore, SessionSummary,
};

#[tauri::command]
pub async fn session_list(
    store: tauri::State<'_, SessionStore>,
) -> Result<Vec<SessionSummary>, String> {
    store
        .list_sessions()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn session_create(
    store: tauri::State<'_, SessionStore>,
    input: SessionInput,
) -> Result<Session, String> {
    store
        .create_session(input)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn session_load(
    store: tauri::State<'_, SessionStore>,
    id: String,
) -> Result<SessionLoadResult, String> {
    let session = store
        .load_session(&id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("session not found: {id}"))?;
    let messages = store
        .load_messages(&id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(SessionLoadResult { session, messages })
}

#[tauri::command]
pub async fn session_delete(
    store: tauri::State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    store
        .delete_session(&id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn session_rename(
    store: tauri::State<'_, SessionStore>,
    id: String,
    title: String,
) -> Result<Session, String> {
    store
        .rename_session(&id, &title)
        .await
        .map_err(|error| error.to_string())
}
