use anvil_core::ai::ContentBlock;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tool::{ApprovalDecision, Tool, ToolContext, ToolOutput};

const MAX_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Default)]
pub struct Bash;

#[async_trait]
impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command via `sh -c` in the working directory. Returns combined stdout/stderr and the exit code. Subject to approval."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute." }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(command) = input.get("command").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'command' argument");
        };
        match ctx.approval.check("bash", &input) {
            ApprovalDecision::Deny(reason) => {
                return ToolOutput::error(format!("command denied: {reason}"));
            }
            ApprovalDecision::Allow => {}
        }
        let output = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&ctx.working_dir)
            .output()
            .await
        {
            Ok(o) => o,
            Err(err) => return ToolOutput::error(format!("failed to spawn command: {err}")),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut combined = String::new();
        if !stdout.is_empty() {
            combined.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str("[stderr]\n");
            combined.push_str(&stderr);
        }
        if combined.len() > MAX_OUTPUT_BYTES {
            combined.truncate(MAX_OUTPUT_BYTES);
            combined.push_str("\n...[output truncated]");
        }
        let status = output.status.code().unwrap_or(-1);
        combined.push_str(&format!("\n[exit {status}]"));

        ToolOutput {
            content: vec![ContentBlock::text(combined)],
            is_error: !output.status.success(),
        }
    }
}
