use std::{path::PathBuf, process::Stdio};

use async_trait::async_trait;
use serde_json::Value;
use tokio::{io::AsyncWriteExt, process::Command};
use tokio_util::sync::CancellationToken;

use super::engine::{
    ClassifyRequest, ClassifyResult, EngineAdapter, EngineError, EngineProbe, EngineProbeStatus,
    EngineUsage, TurnRequest, TurnResult,
};

const TRIAGE_AGENT: &str = "openwork-triage";

#[derive(Clone, Debug)]
pub struct OpenCodeAdapter {
    executable: PathBuf,
}

struct ExecutionRequest {
    cwd: PathBuf,
    prompt: String,
    args: Vec<String>,
    model: Option<String>,
    config_content: Option<String>,
    environment: std::collections::BTreeMap<String, String>,
    cancellation: CancellationToken,
}

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self::with_executable("opencode")
    }
}

impl OpenCodeAdapter {
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    async fn execute(&self, request: ExecutionRequest) -> Result<TurnResult, EngineError> {
        let mut command = Command::new(&self.executable);
        command
            .args(request.args)
            .current_dir(request.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .envs(request.environment);
        command.process_group(0);
        if let Some(config_content) = request.config_content {
            command.env("OPENCODE_CONFIG_CONTENT", config_content);
        }
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                EngineError::Missing(error.to_string())
            } else {
                EngineError::Io(error)
            }
        })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            EngineError::Process("OpenCode process did not expose stdin".to_string())
        })?;
        stdin.write_all(request.prompt.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);

        let process_group = child.id().map(|pid| -(pid as i32));
        let mut wait = Box::pin(child.wait_with_output());
        let (output, cancelled) = tokio::select! {
            result = &mut wait => (result?, false),
            _ = request.cancellation.cancelled() => {
                if let Some(process_group) = process_group {
                    unsafe { libc::kill(process_group, libc::SIGINT) };
                }
                let output = tokio::select! {
                    result = &mut wait => result?,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                        if let Some(process_group) = process_group {
                            unsafe { libc::kill(process_group, libc::SIGKILL) };
                        }
                        wait.await?
                    }
                };
                (output, true)
            }
        };
        if cancelled {
            return Err(EngineError::Cancelled);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            return Err(EngineError::Process(if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            }));
        }

        parse_output(&output.stdout, request.model)
    }
}

#[async_trait]
impl EngineAdapter for OpenCodeAdapter {
    async fn probe(&self) -> Result<EngineProbe, EngineError> {
        let version = Command::new(&self.executable)
            .arg("--version")
            .output()
            .await
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    EngineError::Missing(error.to_string())
                } else {
                    EngineError::Io(error)
                }
            })?;
        if !version.status.success() {
            return Ok(EngineProbe {
                status: EngineProbeStatus::Broken,
                version: None,
                detail: Some(String::from_utf8_lossy(&version.stderr).trim().to_string()),
            });
        }
        let version_text = String::from_utf8_lossy(&version.stdout).trim().to_string();
        let help = Command::new(&self.executable)
            .args(["run", "--help"])
            .output()
            .await?;
        let help_text = String::from_utf8_lossy(&help.stdout);
        let missing_flags: Vec<&str> = ["--pure", "--format", "--auto", "--model", "--session"]
            .into_iter()
            .filter(|flag| !help_text.contains(flag))
            .collect();
        if !help.status.success() || !missing_flags.is_empty() {
            return Ok(EngineProbe {
                status: EngineProbeStatus::Broken,
                version: Some(version_text),
                detail: Some(if missing_flags.is_empty() {
                    "opencode run --help failed".to_string()
                } else {
                    format!(
                        "opencode run is missing required flags: {}",
                        missing_flags.join(", ")
                    )
                }),
            });
        }
        let auth = Command::new(&self.executable)
            .args(["auth", "list"])
            .output()
            .await?;
        let auth_text = String::from_utf8_lossy(&auth.stdout).to_ascii_lowercase();
        if !auth.status.success() || auth_text.contains("0 credentials") {
            return Ok(EngineProbe {
                status: EngineProbeStatus::Unauthenticated,
                version: Some(version_text),
                detail: Some("OpenCode has no usable local credentials".to_string()),
            });
        }
        Ok(EngineProbe {
            status: EngineProbeStatus::Ready,
            version: Some(version_text),
            detail: None,
        })
    }

    async fn classify(&self, request: ClassifyRequest) -> Result<ClassifyResult, EngineError> {
        let mut args = vec![
            "run".to_string(),
            "--pure".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--agent".to_string(),
            TRIAGE_AGENT.to_string(),
        ];
        if let Some(model) = &request.model {
            args.extend(["--model".to_string(), model.clone()]);
        }
        let result = self
            .execute(ExecutionRequest {
                cwd: request.cwd,
                prompt: request.prompt,
                args,
                model: request.model,
                config_content: Some(triage_config_content()),
                environment: Default::default(),
                cancellation: request.cancellation,
            })
            .await?;
        Ok(ClassifyResult {
            text: result.text,
            model: result.model,
            usage: result.usage,
        })
    }

    async fn run_turn(&self, request: TurnRequest) -> Result<TurnResult, EngineError> {
        let mut args = vec![
            "run".to_string(),
            "--pure".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--auto".to_string(),
        ];
        if let Some(session_id) = &request.resume_session_id {
            args.extend(["--session".to_string(), session_id.clone()]);
        }
        if let Some(model) = &request.model {
            args.extend(["--model".to_string(), model.clone()]);
        }
        self.execute(ExecutionRequest {
            cwd: request.home,
            prompt: request.prompt,
            args,
            model: request.model,
            config_content: None,
            environment: request.environment,
            cancellation: request.cancellation,
        })
        .await
    }
}

fn triage_config_content() -> String {
    serde_json::json!({
        "agent": {
            TRIAGE_AGENT: {
                "description": "OpenWork local classifier (tool-free)",
                "mode": "primary",
                "prompt": "Return only the requested JSON decision. Do not call tools.",
                "permission": {"*": "deny"}
            }
        }
    })
    .to_string()
}

fn parse_output(stdout: &[u8], model: Option<String>) -> Result<TurnResult, EngineError> {
    let stdout = std::str::from_utf8(stdout)
        .map_err(|error| EngineError::Protocol(format!("stdout is not UTF-8: {error}")))?;
    let mut result = TurnResult {
        model,
        ..TurnResult::default()
    };
    let mut observed = false;

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line)
            .map_err(|error| EngineError::Protocol(format!("invalid JSONL: {error}")))?;
        observed = true;
        if let Some(session_id) = event.get("sessionID").and_then(Value::as_str)
            && !session_id.is_empty()
        {
            result.session_id = Some(session_id.to_string());
        }
        match event.get("type").and_then(Value::as_str) {
            Some("error") => return Err(EngineError::Reported(error_text(event.get("error")))),
            Some("text") => {
                if let Some(text) = event.pointer("/part/text").and_then(Value::as_str) {
                    result.text.push_str(text);
                }
            }
            Some("step_finish") => add_usage(&mut result.usage, event.pointer("/part/tokens")),
            _ => {}
        }
    }

    if !observed {
        return Err(EngineError::Protocol(
            "OpenCode exited without JSONL events".to_string(),
        ));
    }
    Ok(result)
}

fn error_text(error: Option<&Value>) -> String {
    error
        .and_then(|value| {
            value
                .pointer("/data/message")
                .or_else(|| value.get("message"))
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
        .or_else(|| error.map(Value::to_string))
        .unwrap_or_else(|| "unknown OpenCode error".to_string())
}

fn add_usage(total: &mut EngineUsage, tokens: Option<&Value>) {
    let number = |pointer: &str| {
        tokens
            .and_then(|value| value.pointer(pointer))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    total.input_tokens = total.input_tokens.saturating_add(number("/input"));
    total.output_tokens = total
        .output_tokens
        .saturating_add(number("/output").saturating_add(number("/reasoning")));
    total.cached_input_tokens = total
        .cached_input_tokens
        .saturating_add(number("/cache/read"));
    total.cache_creation_input_tokens = total
        .cache_creation_input_tokens
        .saturating_add(number("/cache/write"));
}
