use async_trait::async_trait;
use serde_json::{Value, json};

use crate::builtin::resolve;
use crate::tool::{Tool, ToolContext, ToolOutput};

const MAX_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct Read;

#[async_trait]
impl Tool for Read {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file from the filesystem. Returns content prefixed with line numbers. Refuses files larger than 1 MB."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path, or a path relative to the working directory."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'path' argument");
        };
        let resolved = resolve(&ctx.working_dir, path);
        match tokio::fs::read_to_string(&resolved).await {
            Ok(content) => {
                if content.len() > MAX_BYTES {
                    return ToolOutput::error(format!(
                        "file too large: {} bytes (max {})",
                        content.len(),
                        MAX_BYTES
                    ));
                }
                let numbered = content
                    .lines()
                    .enumerate()
                    .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutput::text(numbered)
            }
            Err(err) => ToolOutput::error(format!(
                "failed to read {}: {err}",
                resolved.display()
            )),
        }
    }
}
