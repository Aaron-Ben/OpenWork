use openwork_collab::protocol::InvalidationEvent;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

pub const COLLAB_INVALIDATION_EVENT: &str = "openwork://collaboration-invalidation";

pub fn spawn(app: AppHandle, mut invalidations: broadcast::Receiver<InvalidationEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match invalidations.recv().await {
                Ok(invalidation) => {
                    if let Err(error) = app.emit(COLLAB_INVALIDATION_EVENT, invalidation) {
                        tracing::warn!(%error, "Collaboration invalidation could not reach WebView");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "Collaboration Desktop projection lagged; UI will refetch snapshots"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}
