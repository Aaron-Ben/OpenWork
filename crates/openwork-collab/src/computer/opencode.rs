use std::{
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use async_trait::async_trait;
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

use super::engine::{
    ClassifyRequest, ClassifyResult, EngineAdapter, EngineError, EngineProbe, EngineProbeStatus,
    EngineUsage, TurnRequest, TurnResult,
};

const TRIAGE_AGENT: &str = "openwork-triage";
const MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 1024 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;
const ERROR_TAIL_BYTES: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const BEHAVIOR_PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const CLASSIFY_TIMEOUT: Duration = Duration::from_secs(60);
const MAIN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const NO_OUTPUT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

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
    absolute_timeout: Duration,
    require_session: bool,
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr_tail: Vec<u8>,
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

    async fn probe_command(&self, arguments: &[&str]) -> Result<CapturedOutput, EngineError> {
        let mut command = Command::new(&self.executable);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                EngineError::Missing(error.to_string())
            } else {
                EngineError::Io(error)
            }
        })?;
        capture_output(
            &mut child,
            CancellationToken::new(),
            PROBE_TIMEOUT,
            PROBE_TIMEOUT,
        )
        .await
    }

    async fn execute(&self, request: ExecutionRequest) -> Result<TurnResult, EngineError> {
        let resuming = request
            .args
            .windows(2)
            .any(|arguments| arguments[0] == "--session" && !arguments[1].is_empty());
        let mut command = Command::new(&self.executable);
        command
            .args(request.args)
            .current_dir(&request.cwd)
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
        let process_group = child.id().map(|pid| -(pid as i32));
        let write_result = tokio::select! {
            _ = request.cancellation.cancelled() => Err(EngineError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                stdin.write_all(request.prompt.as_bytes()).await?;
                stdin.shutdown().await?;
                Ok::<(), std::io::Error>(())
            }) => match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(error.into()),
                Err(_) => Err(EngineError::Timeout("stdin")),
            }
        };
        if let Err(error) = write_result {
            signal_process_group(process_group, libc::SIGINT);
            if tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .is_err()
            {
                signal_process_group(process_group, libc::SIGKILL);
                let _ = child.wait().await;
            }
            return Err(error);
        }
        drop(stdin);

        let output = capture_output(
            &mut child,
            request.cancellation,
            request.absolute_timeout,
            NO_OUTPUT_TIMEOUT.min(request.absolute_timeout),
        )
        .await?;
        let parsed = parse_output(&output.stdout, request.model).map_err(|error| match error {
            EngineError::Reported(detail) => {
                EngineError::Reported(redact_error_tail(detail.as_bytes(), &request.cwd))
            }
            error => error,
        });
        let stderr = redact_error_tail(&output.stderr_tail, &request.cwd);
        if resuming {
            let detail = match &parsed {
                Err(error) => Some(error.to_string()),
                Ok(_) if !output.status.success() => Some(stderr.clone()),
                Ok(_) => None,
            };
            if let Some(detail) = detail
                && session_invalid_text(&detail)
            {
                return Err(EngineError::SessionInvalid(detail));
            }
        }
        if !output.status.success() {
            if matches!(&parsed, Err(EngineError::Reported(_))) {
                return parsed;
            }
            return Err(EngineError::Process(if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            }));
        }
        let result = parsed?;
        if request.require_session && result.session_id.is_none() {
            return Err(EngineError::Protocol(
                "OpenCode completed a new turn without a session id".to_string(),
            ));
        }
        Ok(result)
    }
}

async fn capture_output(
    child: &mut Child,
    cancellation: CancellationToken,
    absolute_timeout: Duration,
    no_output_timeout: Duration,
) -> Result<CapturedOutput, EngineError> {
    let process_group = child.id().map(|pid| -(pid as i32));
    let mut stdout = child.stdout.take().ok_or_else(|| {
        EngineError::Process("OpenCode process did not expose stdout".to_string())
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        EngineError::Process("OpenCode process did not expose stderr".to_string())
    })?;
    let mut stdout_bytes = Vec::new();
    let mut stderr_tail = Vec::new();
    let mut stdout_total = 0_usize;
    let mut stderr_total = 0_usize;
    let mut line_bytes = 0_usize;
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut status = None;
    let mut terminal_error = None;
    let started = tokio::time::Instant::now();
    let absolute_deadline = started + absolute_timeout;
    let mut no_output_deadline = started + no_output_timeout;
    let mut force_kill_at = None;
    let mut drain_deadline = None;
    let mut stdout_buffer = [0_u8; 8192];
    let mut stderr_buffer = [0_u8; 8192];

    loop {
        if status.is_some() && !stdout_open && !stderr_open && force_kill_at.is_none() {
            break;
        }
        tokio::select! {
            result = child.wait(), if status.is_none() => {
                match result {
                    Ok(exit) => {
                        status = Some(exit);
                        drain_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(2));
                    }
                    Err(error) => {
                        signal_process_group(process_group, libc::SIGKILL);
                        return Err(error.into());
                    }
                }
            }
            result = stdout.read(&mut stdout_buffer), if stdout_open => {
                match result {
                    Ok(0) => stdout_open = false,
                    Ok(read) => {
                        no_output_deadline = tokio::time::Instant::now() + no_output_timeout;
                        stdout_total = stdout_total.saturating_add(read);
                        let mut line_exceeded = false;
                        for byte in &stdout_buffer[..read] {
                            if *byte == b'\n' {
                                line_bytes = 0;
                            } else {
                                line_bytes = line_bytes.saturating_add(1);
                                line_exceeded |= line_bytes > MAX_JSONL_LINE_BYTES;
                            }
                        }
                        if stdout_total > MAX_STDOUT_BYTES || line_exceeded {
                            if terminal_error.is_none() {
                                terminal_error = Some(EngineError::OutputLimit("stdout"));
                                signal_process_group(process_group, libc::SIGKILL);
                            }
                        } else {
                            stdout_bytes.extend_from_slice(&stdout_buffer[..read]);
                        }
                    }
                    Err(error) => {
                        if terminal_error.is_none() {
                            terminal_error = Some(error.into());
                            signal_process_group(process_group, libc::SIGKILL);
                        }
                        stdout_open = false;
                    }
                }
            }
            result = stderr.read(&mut stderr_buffer), if stderr_open => {
                match result {
                    Ok(0) => stderr_open = false,
                    Ok(read) => {
                        no_output_deadline = tokio::time::Instant::now() + no_output_timeout;
                        stderr_total = stderr_total.saturating_add(read);
                        append_tail(&mut stderr_tail, &stderr_buffer[..read], ERROR_TAIL_BYTES);
                        if stderr_total > MAX_STDERR_BYTES && terminal_error.is_none() {
                            terminal_error = Some(EngineError::OutputLimit("stderr"));
                            signal_process_group(process_group, libc::SIGKILL);
                        }
                    }
                    Err(error) => {
                        if terminal_error.is_none() {
                            terminal_error = Some(error.into());
                            signal_process_group(process_group, libc::SIGKILL);
                        }
                        stderr_open = false;
                    }
                }
            }
            _ = cancellation.cancelled(), if terminal_error.is_none() => {
                terminal_error = Some(EngineError::Cancelled);
                signal_process_group(process_group, libc::SIGINT);
                force_kill_at = Some(tokio::time::Instant::now() + Duration::from_secs(2));
            }
            _ = tokio::time::sleep_until(no_output_deadline), if status.is_none() && terminal_error.is_none() => {
                terminal_error = Some(EngineError::Timeout("output"));
                signal_process_group(process_group, libc::SIGINT);
                force_kill_at = Some(tokio::time::Instant::now() + Duration::from_secs(2));
            }
            _ = tokio::time::sleep_until(absolute_deadline), if status.is_none() && terminal_error.is_none() => {
                terminal_error = Some(EngineError::Timeout("turn"));
                signal_process_group(process_group, libc::SIGINT);
                force_kill_at = Some(tokio::time::Instant::now() + Duration::from_secs(2));
            }
            _ = optional_deadline(force_kill_at), if force_kill_at.is_some() => {
                signal_process_group(process_group, libc::SIGKILL);
                force_kill_at = None;
            }
            _ = optional_deadline(drain_deadline), if drain_deadline.is_some() && status.is_some() && (stdout_open || stderr_open) => {
                signal_process_group(process_group, libc::SIGKILL);
                break;
            }
        }
    }

    if let Some(error) = terminal_error {
        return Err(error);
    }
    Ok(CapturedOutput {
        status: status.ok_or_else(|| {
            EngineError::Process("OpenCode process ended without an exit status".to_string())
        })?,
        stdout: stdout_bytes,
        stderr_tail,
    })
}

async fn optional_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn signal_process_group(process_group: Option<i32>, signal: i32) {
    if let Some(process_group) = process_group {
        unsafe { libc::kill(process_group, signal) };
    }
}

fn append_tail(output: &mut Vec<u8>, chunk: &[u8], limit: usize) {
    if chunk.len() >= limit {
        output.clear();
        output.extend_from_slice(&chunk[chunk.len() - limit..]);
        return;
    }
    let overflow = output
        .len()
        .saturating_add(chunk.len())
        .saturating_sub(limit);
    if overflow > 0 {
        output.drain(..overflow);
    }
    output.extend_from_slice(chunk);
}

fn redact_error_tail(stderr: &[u8], cwd: &std::path::Path) -> String {
    let mut text = String::from_utf8_lossy(stderr).trim().to_string();
    let cwd = cwd.to_string_lossy();
    if !cwd.is_empty() {
        text = text.replace(cwd.as_ref(), "<agent-home>");
    }
    for marker in ["Bearer ", "bearer ", "token=", "TOKEN="] {
        let mut offset = 0;
        while let Some(relative_start) = text[offset..].find(marker) {
            let start = offset + relative_start;
            let secret_start = start + marker.len();
            if text[secret_start..].starts_with("<redacted>") {
                offset = secret_start + "<redacted>".len();
                continue;
            }
            let secret_len = text[secret_start..]
                .find(char::is_whitespace)
                .unwrap_or(text.len() - secret_start);
            text.replace_range(secret_start..secret_start + secret_len, "<redacted>");
            offset = secret_start + "<redacted>".len();
        }
    }
    text
}

fn session_invalid_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "session not found",
        "unknown session",
        "invalid session",
        "session does not exist",
        "session is corrupt",
        "corrupt transcript",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn broken_probe(version: Option<String>, detail: String) -> EngineProbe {
    EngineProbe {
        status: EngineProbeStatus::Broken,
        version,
        detail: Some(if detail.trim().is_empty() {
            "OpenCode probe command failed".to_string()
        } else {
            detail
        }),
    }
}

fn authentication_error(error: &EngineError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    [
        "unauthorized",
        "unauthenticated",
        "invalid api key",
        "not logged in",
        "not authenticated",
        "not signed in",
        "please sign in",
        "please log in",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[async_trait]
impl EngineAdapter for OpenCodeAdapter {
    async fn probe(&self) -> Result<EngineProbe, EngineError> {
        let version = match self.probe_command(&["--version"]).await {
            Ok(output) if output.status.success() => output,
            Err(EngineError::Missing(detail)) => {
                return Ok(EngineProbe {
                    status: EngineProbeStatus::Missing,
                    version: None,
                    detail: Some(detail),
                });
            }
            Ok(output) => {
                return Ok(EngineProbe {
                    status: EngineProbeStatus::Broken,
                    version: None,
                    detail: Some(redact_error_tail(
                        &output.stderr_tail,
                        std::path::Path::new(""),
                    )),
                });
            }
            Err(error) => {
                return Ok(EngineProbe {
                    status: EngineProbeStatus::Broken,
                    version: None,
                    detail: Some(error.to_string()),
                });
            }
        };
        let version_text = String::from_utf8_lossy(&version.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string);
        let help = match self.probe_command(&["run", "--help"]).await {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).into_owned()
            }
            Ok(output) => {
                return Ok(broken_probe(
                    version_text,
                    redact_error_tail(&output.stderr_tail, std::path::Path::new("")),
                ));
            }
            Err(error) => return Ok(broken_probe(version_text, error.to_string())),
        };
        for required in ["--pure", "--format", "--auto", "--model", "--session"] {
            if !help.contains(required) {
                return Ok(broken_probe(
                    version_text,
                    format!("OpenCode run --help is missing required flag {required}"),
                ));
            }
        }
        match self.probe_command(&["auth", "list"]).await {
            Ok(output) if output.status.success() => Ok(EngineProbe {
                status: EngineProbeStatus::Ready,
                version: version_text,
                detail: None,
            }),
            Ok(output) => Ok(EngineProbe {
                status: EngineProbeStatus::Unauthenticated,
                version: version_text,
                detail: Some(redact_error_tail(
                    &output.stderr_tail,
                    std::path::Path::new(""),
                )),
            }),
            Err(error) => Ok(broken_probe(version_text, error.to_string())),
        }
    }

    async fn probe_behavior(&self) -> Result<EngineProbe, EngineError> {
        let light = self.probe().await?;
        if light.status != EngineProbeStatus::Ready {
            return Ok(light);
        }
        let neutral = tempfile::Builder::new()
            .prefix("openwork-opencode-probe-")
            .tempdir()?;
        let result = self
            .execute(ExecutionRequest {
                cwd: neutral.path().to_path_buf(),
                prompt: "Connectivity check. Reply with exactly: OK".to_string(),
                args: vec![
                    "run".to_string(),
                    "--pure".to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--agent".to_string(),
                    TRIAGE_AGENT.to_string(),
                ],
                model: None,
                config_content: Some(triage_config_content()),
                environment: std::collections::BTreeMap::from([
                    (
                        "XDG_CONFIG_HOME".to_string(),
                        neutral.path().to_string_lossy().into_owned(),
                    ),
                    (
                        "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
                        "1".to_string(),
                    ),
                ]),
                cancellation: CancellationToken::new(),
                absolute_timeout: BEHAVIOR_PROBE_TIMEOUT,
                require_session: true,
            })
            .await;
        match result {
            Ok(result) if result.text.trim() == "OK" => Ok(light),
            Ok(result) => Ok(broken_probe(
                light.version,
                format!(
                    "OpenCode behavioral probe returned unexpected text: {}",
                    result.text.trim()
                ),
            )),
            Err(error) if authentication_error(&error) => Ok(EngineProbe {
                status: EngineProbeStatus::Unauthenticated,
                version: light.version,
                detail: Some(error.to_string()),
            }),
            Err(error) => Ok(broken_probe(light.version, error.to_string())),
        }
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
                environment: request.environment,
                cancellation: request.cancellation,
                absolute_timeout: CLASSIFY_TIMEOUT,
                require_session: false,
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
            absolute_timeout: MAIN_TIMEOUT,
            require_session: request.resume_session_id.is_none(),
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
