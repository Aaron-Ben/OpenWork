use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::opencode::GlobalEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermission {
    pub id: String,
    pub session_id: String,
    pub directory: Option<String>,
    pub permission: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct PermissionTracker {
    pending: Arc<RwLock<BTreeMap<String, PendingPermission>>>,
}

impl PermissionTracker {
    pub async fn observe(&self, event: &GlobalEvent) {
        match event.event_type() {
            Some("openwork.engine.restarted") => {
                self.pending.write().await.clear();
            }
            Some("permission.asked") => {
                let Some(id) = event
                    .payload
                    .pointer("/properties/id")
                    .and_then(serde_json::Value::as_str)
                else {
                    return;
                };
                let Some(session_id) = event.session_id() else {
                    return;
                };
                let permission = event
                    .payload
                    .pointer("/properties/permission")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                self.pending.write().await.insert(
                    id.to_string(),
                    PendingPermission {
                        id: id.to_string(),
                        session_id: session_id.to_string(),
                        directory: event
                            .directory
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned()),
                        permission: permission.to_string(),
                        payload: event.payload.clone(),
                    },
                );
            }
            Some("permission.replied") => {
                if let Some(id) = event
                    .payload
                    .pointer("/properties/requestID")
                    .and_then(serde_json::Value::as_str)
                {
                    self.pending.write().await.remove(id);
                }
            }
            _ => {}
        }
    }

    pub async fn snapshot(&self) -> Vec<PendingPermission> {
        self.pending.read().await.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::PermissionTracker;
    use crate::opencode::GlobalEvent;

    #[tokio::test]
    async fn tracks_asked_and_replied_from_global_wrappers() {
        let tracker = PermissionTracker::default();
        tracker
            .observe(&GlobalEvent {
                directory: Some(PathBuf::from("/tmp/alice")),
                project: Some("global".to_string()),
                payload: json!({
                    "type": "permission.asked",
                    "properties": {
                        "id": "per_1",
                        "sessionID": "ses_1",
                        "permission": "external_directory"
                    }
                }),
            })
            .await;
        assert_eq!(tracker.snapshot().await.len(), 1);
        tracker
            .observe(&GlobalEvent {
                directory: Some(PathBuf::from("/tmp/alice")),
                project: Some("global".to_string()),
                payload: json!({
                    "type": "permission.replied",
                    "properties": {"requestID": "per_1", "sessionID": "ses_1"}
                }),
            })
            .await;
        assert!(tracker.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn engine_restart_drops_permissions_owned_by_the_dead_process() {
        let tracker = PermissionTracker::default();
        tracker
            .observe(&GlobalEvent {
                directory: Some(PathBuf::from("/tmp/alice")),
                project: Some("global".to_string()),
                payload: json!({
                    "type": "permission.asked",
                    "properties": {
                        "id": "per_stale",
                        "sessionID": "ses_1",
                        "permission": "external_directory"
                    }
                }),
            })
            .await;
        tracker
            .observe(&GlobalEvent {
                directory: None,
                project: None,
                payload: json!({"type": "openwork.engine.restarted", "properties": {}}),
            })
            .await;
        assert!(tracker.snapshot().await.is_empty());
    }
}
