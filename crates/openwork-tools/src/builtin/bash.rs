use async_trait::async_trait;
use openwork_protocol::ai::ContentBlock;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::{
    AccessKind, NetworkMode,
    tool::{Tool, ToolContext, ToolOutput},
};

const MAX_OUTPUT_BYTES: usize = 32 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;

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
                "command": { "type": "string", "description": "Shell command to execute." },
                "timeoutMs": { "type": "number", "description": "Optional timeout in milliseconds. Defaults to 30000 and is capped at 120000." }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(command) = input.get("command").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'command' argument");
        };
        if let Err(message) = ctx.check_path(&ctx.working_dir, AccessKind::Read) {
            return ToolOutput::error(message);
        }
        let timeout_ms = input
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1, MAX_TIMEOUT_MS);
        // 审批由编排层(agent loop)统一处理:能进入到这里即已获批准。
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(&ctx.working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env_clear();
        for key in ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        if ctx.permissions.network == NetworkMode::Restricted {
            cmd.env("OPENWORK_NETWORK_RESTRICTED", "1");
        }

        let started = Instant::now();
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => return ToolOutput::error(format!("failed to spawn command: {err}")),
        };
        let wait_task = tokio::spawn(async move { child.wait_with_output().await });
        let abort_wait = wait_task.abort_handle();
        let output = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => {
                abort_wait.abort();
                return ToolOutput::error("command cancelled");
            }
            _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
                abort_wait.abort();
                return ToolOutput::error(format!("command timed out after {timeout_ms} ms"));
            }
            result = wait_task => match result {
                Ok(Ok(output)) => output,
                Ok(Err(err)) => return ToolOutput::error(format!("failed to wait for command: {err}")),
                Err(err) => return ToolOutput::error(format!("command task failed: {err}")),
            }
        };
        let elapsed_ms = started.elapsed().as_millis();

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
        let mut combined = crate::builtin::truncate_output(combined, MAX_OUTPUT_BYTES);
        let status = output.status.code().unwrap_or(-1);
        combined.push_str(&format!("\n[exit {status}; duration {elapsed_ms} ms]"));

        ToolOutput {
            content: vec![ContentBlock::text(combined)],
            is_error: !output.status.success(),
        }
    }
}
