//! One cross-instance OpenCode event subscription for permission state.

use std::time::Duration;

use serde_json::json;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    event::{CollabEventKind, CollabEventPublisher},
    opencode::{EngineConnection, GlobalEvent},
    permission::PermissionTracker,
};

pub fn start(
    connections: watch::Receiver<Option<EngineConnection>>,
    events: mpsc::UnboundedSender<GlobalEvent>,
    permissions: PermissionTracker,
    published_events: CollabEventPublisher,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(forward(
        connections,
        events,
        permissions,
        published_events,
        cancel,
    ))
}

async fn forward(
    mut connections: watch::Receiver<Option<EngineConnection>>,
    events: mpsc::UnboundedSender<GlobalEvent>,
    permissions: PermissionTracker,
    published_events: CollabEventPublisher,
    cancel: CancellationToken,
) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let Some(connection) = connections.borrow().clone() else {
            tokio::select! {
                _ = cancel.cancelled() => return,
                changed = connections.changed() => {
                    if changed.is_err() { return; }
                    continue;
                }
            }
        };
        let generation = connection.generation;
        let mut stream = match connection.client.global_events().await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("failed to subscribe /global/event: {error}");
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => continue,
                }
            }
        };
        if generation > 1 {
            let restarted = GlobalEvent {
                directory: None,
                project: None,
                payload: json!({"type": "openwork.engine.restarted", "properties": {}}),
            };
            permissions.observe(&restarted).await;
            published_events
                .publish(CollabEventKind::EngineChanged)
                .await;
            published_events
                .publish(CollabEventKind::PermissionsChanged)
                .await;
            if events.send(restarted).is_err() {
                return;
            }
        }
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                changed = connections.changed() => {
                    if changed.is_err() { return; }
                    if connections.borrow().as_ref().is_none_or(|current| current.generation != generation) {
                        break;
                    }
                }
                event = stream.next() => match event {
                    Ok(event) => {
                        let permissions_changed = matches!(
                            event.event_type(),
                            Some("permission.asked" | "permission.replied")
                        );
                        permissions.observe(&event).await;
                        if permissions_changed {
                            published_events
                                .publish(CollabEventKind::PermissionsChanged)
                                .await;
                        }
                    }
                    Err(error) => {
                        eprintln!("/global/event disconnected: {error}");
                        break;
                    }
                }
            }
        }
    }
}
