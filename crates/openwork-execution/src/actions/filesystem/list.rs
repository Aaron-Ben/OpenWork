use crate::policy::AccessKind;
use async_trait::async_trait;
use openwork_protocol::capability::{Observation, ObservationErrorCode};
use serde_json::Value;

use crate::ExecutionContext;
use crate::handler::ActionHandler;

use super::resolve;

#[derive(Default)]
pub struct List;

#[async_trait]
impl ActionHandler for List {
    fn name(&self) -> &'static str {
        "list"
    }

    async fn invoke(&self, input: Value, ctx: &ExecutionContext) -> Observation {
        let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let resolved = resolve(&ctx.working_dir, path);
        if let Err(message) = ctx.check_path(&resolved, AccessKind::Read) {
            return Observation::denied(message);
        }
        let mut entries = match tokio::fs::read_dir(&resolved).await {
            Ok(rd) => rd,
            Err(err) => {
                return Observation::failed(
                    ObservationErrorCode::ExecutionFailed,
                    format!("failed to list {}: {err}", resolved.display()),
                    false,
                );
            }
        };
        let mut names = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let suffix = entry
                .file_type()
                .await
                .ok()
                .map(|t| if t.is_dir() { "/" } else { "" })
                .unwrap_or("");
            names.push(format!("{name}{suffix}"));
        }
        names.sort();
        if names.is_empty() {
            Observation::succeeded(format!("{} is empty", resolved.display()))
        } else {
            Observation::succeeded(names.join("\n"))
        }
    }
}
