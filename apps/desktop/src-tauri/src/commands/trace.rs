use openwork_app::{OpenWorkApplication, TurnTrace, TurnTraceSummary};

use crate::CommandError;

/// Returns one compact diagnostic summary per Turn in a Session.
#[tauri::command]
pub async fn trace_session(
    application: tauri::State<'_, OpenWorkApplication>,
    session_id: String,
) -> Result<Vec<TurnTraceSummary>, CommandError> {
    application
        .traces()
        .list_session(&session_id)
        .await
        .map_err(CommandError::from)
}

/// Loads the complete V1 span tree for one Turn on demand.
#[tauri::command]
pub async fn trace_turn(
    application: tauri::State<'_, OpenWorkApplication>,
    turn_id: String,
) -> Result<TurnTrace, CommandError> {
    application
        .traces()
        .load_turn(&turn_id)
        .await
        .map_err(CommandError::from)
}
