use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tool::{Tool, ToolContext, ToolOutput};
use crate::{AccessKind, builtin::resolve};

#[derive(Default)]
pub struct Write;

#[async_trait]
impl Tool for Write {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write text content to a file. Creates the file (and parent directories) if missing; overwrites if it exists."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or working-dir-relative path." },
                "content": { "type": "string", "description": "Full file content to write." }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'path' argument");
        };
        let Some(content) = input.get("content").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'content' argument");
        };
        let resolved = resolve(&ctx.working_dir, path);
        if let Err(message) = ctx.check_path(&resolved, AccessKind::Write) {
            return ToolOutput::error(message);
        }
        if let Some(parent) = resolved.parent()
            && let Err(err) = tokio::fs::create_dir_all(parent).await
        {
            return ToolOutput::error(format!("failed to create parent dirs: {err}"));
        }
        match tokio::fs::write(&resolved, content).await {
            Ok(_) => ToolOutput::text(format!(
                "wrote {} bytes to {}",
                content.len(),
                resolved.display()
            )),
            Err(err) => ToolOutput::error(format!("failed to write {}: {err}", resolved.display())),
        }
    }
}
