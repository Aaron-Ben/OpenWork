use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::opencode::GlobalEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermission {
    pub id: String,
    pub session_id: String,
    pub agent_id: Option<String>,
    #[serde(skip_serializing)]
    pub directory: Option<String>,
    pub permission: String,
    pub patterns: Vec<String>,
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
                        agent_id: event
                            .directory
                            .as_deref()
                            .and_then(std::path::Path::file_name)
                            .and_then(std::ffi::OsStr::to_str)
                            .map(str::to_string),
                        directory: event
                            .directory
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned()),
                        permission: permission.to_string(),
                        patterns: event
                            .payload
                            .pointer("/properties/patterns")
                            .and_then(serde_json::Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string)
                            .collect(),
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

    pub async fn get(&self, id: &str) -> Option<PendingPermission> {
        self.pending.read().await.get(id).cloned()
    }

    pub async fn remove(&self, id: &str) -> Option<PendingPermission> {
        self.pending.write().await.remove(id)
    }

    pub async fn remove_session(&self, session_id: &str) -> usize {
        let mut pending = self.pending.write().await;
        let before = pending.len();
        pending.retain(|_, permission| permission.session_id != session_id);
        before - pending.len()
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
