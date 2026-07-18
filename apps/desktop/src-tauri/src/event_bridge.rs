use openwork_core::SessionUpdateEnvelope;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

pub const SESSION_UPDATE_EVENT: &str = "openwork://session-update";

pub fn spawn_session_update_bridge(
    app: AppHandle,
    mut updates: broadcast::Receiver<SessionUpdateEnvelope>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match updates.recv().await {
                Ok(payload) => {
                    let _ = app.emit(SESSION_UPDATE_EVENT, payload);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
