use crate::policy::{AccessKind, NetworkMode};
use async_trait::async_trait;
use openwork_protocol::capability::{Observation, ObservationErrorCode};
use serde_json::Value;
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::ExecutionContext;
use crate::handler::ActionHandler;

const MAX_OUTPUT_BYTES: usize = 32 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;

#[derive(Default)]
pub struct Bash;

#[async_trait]
impl ActionHandler for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    async fn invoke(&self, input: Value, ctx: &ExecutionContext) -> Observation {
        let Some(command) = input.get("command").and_then(Value::as_str) else {
            return Observation::failed(
                ObservationErrorCode::InvalidArguments,
                "missing or invalid 'command' argument",
                false,
            );
        };
        if let Err(message) = ctx.check_path(&ctx.working_dir, AccessKind::Read) {
            return Observation::denied(message);
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
            Err(err) => {
                return Observation::failed(
                    ObservationErrorCode::ExecutionFailed,
                    format!("failed to spawn command: {err}"),
                    false,
                );
            }
        };
        let wait_task = tokio::spawn(async move { child.wait_with_output().await });
        let abort_wait = wait_task.abort_handle();
        let output = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => {
                abort_wait.abort();
                return Observation::cancelled("command cancelled");
            }
            _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
                abort_wait.abort();
                return Observation::failed(
                    ObservationErrorCode::Timeout,
                    format!("command timed out after {timeout_ms} ms"),
                    false,
                );
            }
            result = wait_task => match result {
                Ok(Ok(output)) => output,
                Ok(Err(err)) => return Observation::failed(
                    ObservationErrorCode::ExecutionFailed,
                    format!("failed to wait for command: {err}"),
                    false,
                ),
                Err(err) => return Observation::failed(
                    ObservationErrorCode::ExecutionFailed,
                    format!("command task failed: {err}"),
                    false,
                ),
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
        let mut combined = crate::actions::truncate_output(combined, MAX_OUTPUT_BYTES);
        let status = output.status.code().unwrap_or(-1);
        combined.push_str(&format!("\n[exit {status}; duration {elapsed_ms} ms]"));

        if output.status.success() {
            Observation::succeeded(combined)
        } else {
            Observation::failed(ObservationErrorCode::ExecutionFailed, combined, false)
        }
    }
}
