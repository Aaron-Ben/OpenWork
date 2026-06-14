use async_trait::async_trait;
use serde_json::{Value, json};

use crate::builtin::resolve;
use crate::tool::{Tool, ToolContext, ToolOutput};

#[derive(Default)]
pub struct List;

#[async_trait]
impl Tool for List {
    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List entries in a directory. Directories are suffixed with '/'. Defaults to the working directory."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path; defaults to the working directory." }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let resolved = resolve(&ctx.working_dir, path);
        let mut entries = match tokio::fs::read_dir(&resolved).await {
            Ok(rd) => rd,
            Err(err) => {
                return ToolOutput::error(format!(
                    "failed to list {}: {err}",
                    resolved.display()
                ))
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
            ToolOutput::text(format!("{} is empty", resolved.display()))
        } else {
            ToolOutput::text(names.join("\n"))
        }
    }
}
