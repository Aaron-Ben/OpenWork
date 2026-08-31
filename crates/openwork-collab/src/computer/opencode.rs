use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::engine::{
    AgentEngineRuntime, ClassifyRequest, ClassifyResult, EngineAdapter, EngineAvailability,
    EngineError, EngineId, EngineInventory, EngineRuntimeConfig, EngineUsage, TurnRequest,
    TurnResult,
};

const TRIAGE_AGENT: &str = "openwork-triage";
const MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 1024 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;
const ERROR_TAIL_BYTES: usize = 16 * 1024;
const INVENTORY_TIMEOUT: Duration = Duration::from_secs(3);
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

struct ExecutionResult {
    turn: TurnResult,
    session_id: Option<String>,
}

struct OpenCodeRuntime {
    adapter: OpenCodeAdapter,
    config: EngineRuntimeConfig,
    session_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct SessionMetadata {
    engine_id: String,
    model: String,
    persona_hash: String,
    session_id: String,
    updated_at: String,
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

    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult, EngineError> {
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
                EngineError::Missing {
                    detail: error.to_string(),
                }
            } else {
                EngineError::Io(error)
            }
        })?;

        let mut stdin = child.stdin.take().ok_or_else(|| EngineError::Process {
            detail: "process did not expose stdin".to_string(),
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
                Err(_) => Err(EngineError::Timeout { operation: "stdin" }),
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

        let output = match capture_output(
            &mut child,
            request.cancellation,
            request.absolute_timeout,
            NO_OUTPUT_TIMEOUT.min(request.absolute_timeout),
        )
        .await
        {
            Ok(output) => output,
            Err(EngineError::Reported { detail }) => {
                let detail = redact_error_tail(detail.as_bytes(), &request.cwd);
                if resuming && session_invalid_text(&detail) {
                    return Err(EngineError::SessionInvalid { detail });
                }
                return Err(normalize_reported_error(detail));
            }
            Err(error) => return Err(error),
        };
        let parsed = parse_output(&output.stdout, request.model).map_err(|error| match error {
            EngineError::Reported { detail } => EngineError::Reported {
                detail: redact_error_tail(detail.as_bytes(), &request.cwd),
            },
            EngineError::Process { detail } => EngineError::Process {
                detail: redact_error_tail(detail.as_bytes(), &request.cwd),
            },
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
                return Err(EngineError::SessionInvalid { detail });
            }
        }
        if !output.status.success() {
            if let Err(EngineError::Reported { detail }) = &parsed {
                return Err(normalize_reported_error(detail.clone()));
            }
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            return Err(normalize_process_error(detail));
        }
        let result = parsed.map_err(|error| match error {
            EngineError::Reported { detail } => normalize_reported_error(detail),
            error => error,
        })?;
        if request.require_session && result.session_id.is_none() {
            return Err(EngineError::Protocol {
                detail: "completed a new turn without a session id".to_string(),
            });
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
    let mut stdout = child.stdout.take().ok_or_else(|| EngineError::Process {
        detail: "process did not expose stdout".to_string(),
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| EngineError::Process {
        detail: "process did not expose stderr".to_string(),
    })?;
    let mut stdout_bytes = Vec::new();
    let mut stderr_tail = Vec::new();
    let mut stdout_total = 0_usize;
    let mut stderr_total = 0_usize;
    let mut line_bytes = 0_usize;
    let mut inspected_stdout_bytes = 0_usize;
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
                                terminal_error = Some(EngineError::OutputLimit {
                                    stream: "stdout",
                                    limit: if stdout_total > MAX_STDOUT_BYTES {
                                        MAX_STDOUT_BYTES
                                    } else {
                                        MAX_JSONL_LINE_BYTES
                                    },
                                });
                                signal_process_group(process_group, libc::SIGKILL);
                            }
                        } else {
                            stdout_bytes.extend_from_slice(&stdout_buffer[..read]);
                            while let Some(relative_end) = stdout_bytes[inspected_stdout_bytes..]
                                .iter()
                                .position(|byte| *byte == b'\n')
                            {
                                let line_end = inspected_stdout_bytes + relative_end;
                                if terminal_error.is_none()
                                    && let Some(error) = reported_jsonl_error(
                                        &stdout_bytes[inspected_stdout_bytes..line_end],
                                    )
                                {
                                    terminal_error = Some(error);
                                    signal_process_group(process_group, libc::SIGINT);
                                    force_kill_at = Some(
                                        tokio::time::Instant::now() + Duration::from_secs(2),
                                    );
                                }
                                inspected_stdout_bytes = line_end + 1;
                            }
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
                            terminal_error = Some(EngineError::OutputLimit {
                                stream: "stderr",
                                limit: MAX_STDERR_BYTES,
                            });
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
                terminal_error = Some(EngineError::Timeout { operation: "output" });
                signal_process_group(process_group, libc::SIGINT);
                force_kill_at = Some(tokio::time::Instant::now() + Duration::from_secs(2));
            }
            _ = tokio::time::sleep_until(absolute_deadline), if status.is_none() && terminal_error.is_none() => {
                terminal_error = Some(EngineError::Timeout { operation: "turn" });
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
        status: status.ok_or_else(|| EngineError::Process {
            detail: "process ended without an exit status".to_string(),
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

fn reported_jsonl_error(line: &[u8]) -> Option<EngineError> {
    let event: Value = serde_json::from_slice(line).ok()?;
    (event.get("type").and_then(Value::as_str) == Some("error")).then(|| EngineError::Reported {
        detail: error_text(event.get("error")),
    })
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

#[async_trait]
impl EngineAdapter for OpenCodeAdapter {
    fn id(&self) -> EngineId {
        EngineId::opencode()
    }

    async fn probe(&self) -> Result<EngineInventory, EngineError> {
        let mut command = Command::new("/usr/bin/which");
        command
            .arg(&self.executable)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let status = tokio::time::timeout(INVENTORY_TIMEOUT, command.status())
            .await
            .map_err(|_| EngineError::Timeout {
                operation: "inventory scan",
            })??;
        let availability = match status.code() {
            Some(0) => EngineAvailability::Available,
            Some(1) => EngineAvailability::Missing,
            _ => {
                return Err(EngineError::Process {
                    detail: format!("/usr/bin/which failed with {status}"),
                });
            }
        };
        Ok(EngineInventory { availability })
    }

    async fn classify(&self, request: ClassifyRequest) -> Result<ClassifyResult, EngineError> {
        let environment = prepare_environment(&request.config_root, request.environment).await?;
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
                environment,
                cancellation: request.cancellation,
                absolute_timeout: CLASSIFY_TIMEOUT,
                require_session: false,
            })
            .await?;
        Ok(ClassifyResult {
            text: result.turn.text,
            model: result.turn.model,
            usage: result.turn.usage,
        })
    }

    async fn create_agent_runtime(
        &self,
        mut config: EngineRuntimeConfig,
    ) -> Result<Box<dyn AgentEngineRuntime>, EngineError> {
        config.environment = prepare_environment(&config.config_root, config.environment).await?;
        let session_id = load_session(&config).await?;
        Ok(Box::new(OpenCodeRuntime {
            adapter: self.clone(),
            config,
            session_id,
        }))
    }
}

impl OpenCodeRuntime {
    async fn execute_turn(
        &self,
        prompt: String,
        cancellation: CancellationToken,
    ) -> Result<ExecutionResult, EngineError> {
        let mut args = vec![
            "run".to_string(),
            "--pure".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--auto".to_string(),
        ];
        if let Some(session_id) = &self.session_id {
            args.extend(["--session".to_string(), session_id.clone()]);
        }
        args.extend(["--model".to_string(), self.config.model.clone()]);
        self.adapter
            .execute(ExecutionRequest {
                cwd: self.config.home.clone(),
                prompt,
                args,
                model: Some(self.config.model.clone()),
                config_content: None,
                environment: self.config.environment.clone(),
                cancellation,
                absolute_timeout: MAIN_TIMEOUT,
                require_session: self.session_id.is_none(),
            })
            .await
    }
}

#[async_trait]
impl AgentEngineRuntime for OpenCodeRuntime {
    async fn run_turn(&mut self, request: TurnRequest) -> Result<TurnResult, EngineError> {
        let resumed = self.session_id.is_some();
        let result = self
            .execute_turn(request.prompt.clone(), request.cancellation.clone())
            .await;
        let result = if resumed && matches!(result, Err(EngineError::SessionInvalid { .. })) {
            clear_session(&self.config.state_file).await?;
            self.session_id = None;
            self.execute_turn(request.prompt, request.cancellation)
                .await
        } else {
            result
        }?;
        if let Some(session_id) = result.session_id {
            save_session(&self.config, &session_id).await?;
            self.session_id = Some(session_id);
        }
        Ok(result.turn)
    }

    async fn shutdown(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

async fn prepare_environment(
    config_root: &std::path::Path,
    mut environment: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, EngineError> {
    let opencode_config = config_root.join("opencode");
    secure_directory(&opencode_config).await?;
    atomic_write(
        &opencode_config.join("opencode.json"),
        br#"{"permission":{"*":"allow"}}"#,
    )
    .await?;
    environment.insert(
        "XDG_CONFIG_HOME".to_string(),
        config_root.to_string_lossy().into_owned(),
    );
    environment.insert(
        "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
        "1".to_string(),
    );
    Ok(environment)
}

async fn load_session(config: &EngineRuntimeConfig) -> Result<Option<String>, EngineError> {
    match tokio::fs::read(&config.state_file).await {
        Ok(bytes) => {
            let Ok(metadata) = serde_json::from_slice::<SessionMetadata>(&bytes) else {
                return Ok(None);
            };
            if metadata.engine_id == EngineId::opencode().as_str()
                && metadata.model == config.model
                && metadata.persona_hash == config.config_fingerprint
                && !metadata.session_id.trim().is_empty()
            {
                Ok(Some(metadata.session_id))
            } else {
                Ok(None)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn save_session(config: &EngineRuntimeConfig, session_id: &str) -> Result<(), EngineError> {
    let metadata = SessionMetadata {
        engine_id: EngineId::opencode().to_string(),
        model: config.model.clone(),
        persona_hash: config.config_fingerprint.clone(),
        session_id: session_id.to_string(),
        updated_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("current timestamp formats as RFC 3339"),
    };
    atomic_write(
        &config.state_file,
        &serde_json::to_vec(&metadata).map_err(|error| EngineError::Protocol {
            detail: format!("session metadata is not serializable: {error}"),
        })?,
    )
    .await
}

async fn clear_session(state_file: &std::path::Path) -> Result<(), EngineError> {
    match tokio::fs::remove_file(state_file).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn secure_directory(path: &std::path::Path) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

async fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), EngineError> {
    let parent = path.parent().expect("managed Engine file has parent");
    secure_directory(parent).await?;
    let temporary = parent.join(format!(".openwork-{}.tmp", Uuid::new_v4().simple()));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .await?;
    }
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(temporary, path).await?;
    Ok(())
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

fn parse_output(stdout: &[u8], model: Option<String>) -> Result<ExecutionResult, EngineError> {
    let stdout = std::str::from_utf8(stdout).map_err(|error| EngineError::Protocol {
        detail: format!("stdout is not UTF-8: {error}"),
    })?;
    let mut turn = TurnResult {
        model,
        ..TurnResult::default()
    };
    let mut session_id = None;
    let mut observed = false;

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line).map_err(|error| EngineError::Protocol {
            detail: format!("invalid JSONL: {error}"),
        })?;
        observed = true;
        if let Some(observed_session_id) = event.get("sessionID").and_then(Value::as_str)
            && !observed_session_id.is_empty()
        {
            session_id = Some(observed_session_id.to_string());
        }
        match event.get("type").and_then(Value::as_str) {
            Some("error") => {
                return Err(EngineError::Reported {
                    detail: error_text(event.get("error")),
                });
            }
            Some("text") => {
                if let Some(text) = event.pointer("/part/text").and_then(Value::as_str) {
                    turn.text.push_str(text);
                }
            }
            Some("step_finish") => add_usage(&mut turn.usage, event.pointer("/part/tokens")),
            _ => {}
        }
    }

    if !observed {
        return Err(EngineError::Protocol {
            detail: "exited without JSONL events".to_string(),
        });
    }
    Ok(ExecutionResult { turn, session_id })
}

fn normalize_reported_error(detail: String) -> EngineError {
    normalize_provider_error(detail, |detail| EngineError::Reported { detail })
}

fn normalize_process_error(detail: String) -> EngineError {
    normalize_provider_error(detail, |detail| EngineError::Process { detail })
}

fn normalize_provider_error(
    detail: String,
    fallback: impl FnOnce(String) -> EngineError,
) -> EngineError {
    let normalized = detail.to_ascii_lowercase();
    if [
        "rate limit",
        "rate_limit",
        "too many requests",
        "429",
        "quota",
        "overload",
        "503",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        return EngineError::RateLimited {
            retry_after: None,
            detail,
        };
    }
    if [
        "not authenticated",
        "authentication required",
        "unauthorized",
        "not logged in",
        "401",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        return EngineError::Unauthenticated { detail };
    }
    fallback(detail)
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
