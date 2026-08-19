use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use reqwest::{RequestBuilder, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::model::Agent;

pub const DIRECTORY_HEADER: &str = "x-opencode-directory";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const RESTART_DELAY: Duration = Duration::from_millis(500);

/// 启动自检探测的契约点。
///
/// 这里**不比较版本号**。版本相等不保证行为相同，版本不同也不意味着不兼容——
/// 用等号近似"兼容"两边都会错。真正要守住的是"opencode 改了我们依赖的东西时，
/// 失败发生在启动的一声响里，而不是半夜某个 Agent 静默不回话"。
///
/// 因此这里直接探我们真正依赖的行为；版本号只进诊断信息。
/// 设计见 `docs/collaboration.md` §4.2。
const CONTRACT_PROBES: &[&str] = &[
    "GET /session (v1 路径族，返回数组)",
    "GET /agent",
    "GET /global/event (可建立 SSE)",
];

#[derive(Debug, Clone)]
pub struct OpenCodeClient {
    http: reqwest::Client,
    base_url: String,
    password: Option<String>,
}

impl OpenCodeClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            password: env::var("OPENCODE_SERVER_PASSWORD").ok(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn health(&self) -> Result<Health, OpenCodeError> {
        self.json(self.authenticated(self.http.get(self.url("/global/health"))))
            .await
    }

    /// 启动自检：探测 daemon 真正依赖的端点是否存在、形状是否还对。
    ///
    /// 便宜的部分在这里一次探完（四次 HTTP、零 token）；探不了的部分
    /// （事件载荷形状、审批流）靠严格解析在首次使用时响亮失败，
    /// 而不是 `unwrap_or_default()` 之后静默走错路。
    pub async fn probe_contract(&self) -> Result<(), OpenCodeError> {
        let sessions: serde_json::Value = self
            .json(self.authenticated(self.http.get(self.url("/session?limit=1"))))
            .await
            .map_err(|error| OpenCodeError::ContractUnsatisfied {
                probe: CONTRACT_PROBES[0],
                detail: error.to_string(),
            })?;
        // v1 返回数组；v2 的 /api/session 返回 {data, cursor}。形状不对说明
        // 我们赖以生存的 v1 路径族已经变了——而 prompt_async 只存在于 v1。
        if !sessions.is_array() {
            return Err(OpenCodeError::ContractUnsatisfied {
                probe: CONTRACT_PROBES[0],
                detail: format!("expected a JSON array, got {sessions}"),
            });
        }

        let agents: serde_json::Value = self
            .json(self.authenticated(self.http.get(self.url("/agent"))))
            .await
            .map_err(|error| OpenCodeError::ContractUnsatisfied {
                probe: CONTRACT_PROBES[1],
                detail: error.to_string(),
            })?;
        if !agents.is_array() {
            return Err(OpenCodeError::ContractUnsatisfied {
                probe: CONTRACT_PROBES[1],
                detail: format!("expected a JSON array, got {agents}"),
            });
        }

        // 全局流是跨 instance 汇总待审批的唯一廉价通路（见 docs/collaboration.md §6）。
        self.global_events()
            .await
            .map_err(|error| OpenCodeError::ContractUnsatisfied {
                probe: CONTRACT_PROBES[2],
                detail: error.to_string(),
            })?;

        Ok(())
    }

    pub async fn create_session(
        &self,
        directory: &Path,
        title: &str,
    ) -> Result<Session, OpenCodeError> {
        self.json(
            self.for_directory(self.http.post(self.url("/session")), directory)
                .json(&json!({"title": title})),
        )
        .await
    }

    pub async fn get_session(
        &self,
        directory: &Path,
        session_id: &str,
    ) -> Result<Option<Session>, OpenCodeError> {
        let response = self
            .for_directory(
                self.http.get(self.url(&format!("/session/{session_id}"))),
                directory,
            )
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(parse_json_response(response).await?))
    }

    pub async fn prompt_async(
        &self,
        directory: &Path,
        session_id: &str,
        agent: &Agent,
        text: &str,
    ) -> Result<(), OpenCodeError> {
        let body = json!({
            "agent": agent.id,
            "model": {
                "providerID": agent.provider_id,
                "modelID": agent.model_id,
            },
            "system": agent.system_prompt,
            "parts": [{"type": "text", "text": text}],
        });
        let response = self
            .for_directory(
                self.http
                    .post(self.url(&format!("/session/{session_id}/prompt_async"))),
                directory,
            )
            .json(&body)
            .send()
            .await?;
        if response.status() != StatusCode::NO_CONTENT {
            return Err(response_error(response).await);
        }
        Ok(())
    }

    pub async fn abort(&self, directory: &Path, session_id: &str) -> Result<bool, OpenCodeError> {
        self.json(
            self.for_directory(
                self.http
                    .post(self.url(&format!("/session/{session_id}/abort"))),
                directory,
            ),
        )
        .await
    }

    pub async fn reply_permission(
        &self,
        directory: &Path,
        request_id: &str,
        reply: PermissionReply,
        message: Option<&str>,
    ) -> Result<bool, OpenCodeError> {
        self.json(
            self.for_directory(
                self.http
                    .post(self.url(&format!("/permission/{request_id}/reply"))),
                directory,
            )
            .json(&json!({"reply": reply, "message": message})),
        )
        .await
    }

    pub async fn global_events(&self) -> Result<GlobalEventStream, OpenCodeError> {
        let response = self
            .authenticated(
                self.http
                    .get(self.url("/global/event"))
                    .header(reqwest::header::ACCEPT, "text/event-stream"),
            )
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        Ok(GlobalEventStream {
            response,
            buffer: Vec::new(),
        })
    }

    async fn json<T: for<'de> Deserialize<'de>>(
        &self,
        request: RequestBuilder,
    ) -> Result<T, OpenCodeError> {
        parse_json_response(request.send().await?).await
    }

    fn for_directory(&self, request: RequestBuilder, directory: &Path) -> RequestBuilder {
        self.authenticated(request.header(DIRECTORY_HEADER, directory.to_string_lossy().as_ref()))
    }

    fn authenticated(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.password {
            Some(password) => request.basic_auth("opencode", Some(password)),
            None => request,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub healthy: bool,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    pub id: String,
    pub directory: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    Once,
    Always,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalEvent {
    pub directory: Option<PathBuf>,
    pub project: Option<String>,
    pub payload: Value,
}

impl GlobalEvent {
    pub fn event_type(&self) -> Option<&str> {
        self.payload.get("type").and_then(Value::as_str)
    }

    pub fn session_id(&self) -> Option<&str> {
        self.payload
            .pointer("/properties/sessionID")
            .or_else(|| self.payload.pointer("/properties/info/sessionID"))
            .or_else(|| self.payload.pointer("/properties/part/sessionID"))
            .and_then(Value::as_str)
    }

    pub fn session_status(&self) -> Option<&str> {
        (self.event_type() == Some("session.status"))
            .then(|| {
                self.payload
                    .pointer("/properties/status/type")
                    .and_then(Value::as_str)
            })
            .flatten()
    }
}

pub struct GlobalEventStream {
    response: reqwest::Response,
    buffer: Vec<u8>,
}

impl GlobalEventStream {
    pub async fn next(&mut self) -> Result<GlobalEvent, OpenCodeError> {
        loop {
            if let Some(frame) = take_sse_frame(&mut self.buffer) {
                if let Some(data) = sse_data(&frame)? {
                    let raw: Value = serde_json::from_str(&data)?;
                    return normalize_global_event(raw);
                }
                continue;
            }
            let chunk = self
                .response
                .chunk()
                .await?
                .ok_or(OpenCodeError::EventStreamClosed)?;
            self.buffer.extend_from_slice(&chunk);
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineConnection {
    pub generation: u64,
    pub pid: Option<u32>,
    pub version: Version,
    pub client: OpenCodeClient,
}

pub struct OpenCodeSupervisor {
    connection: watch::Receiver<Option<EngineConnection>>,
    task: JoinHandle<()>,
}

impl OpenCodeSupervisor {
    pub async fn start(cancel: CancellationToken) -> Result<Self, OpenCodeError> {
        let binary = env::var_os("OPENCODE_BIN").unwrap_or_else(|| "opencode".into());
        Self::start_with_binary(binary, cancel).await
    }

    pub async fn start_with_binary(
        binary: impl Into<PathBuf>,
        cancel: CancellationToken,
    ) -> Result<Self, OpenCodeError> {
        let binary = binary.into();
        let (process, connection) = spawn_checked(1, &binary).await?;
        let (connection_tx, connection_rx) = watch::channel(Some(connection));
        let task = tokio::spawn(supervise(process, binary, connection_tx, cancel));
        Ok(Self {
            connection: connection_rx,
            task,
        })
    }

    pub fn current(&self) -> Result<EngineConnection, OpenCodeError> {
        self.connection
            .borrow()
            .clone()
            .ok_or(OpenCodeError::Unavailable)
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<EngineConnection>> {
        self.connection.clone()
    }

    pub async fn shutdown(self) {
        let _ = self.task.await;
    }
}

async fn supervise(
    mut process: OpenCodeProcess,
    binary: PathBuf,
    connection: watch::Sender<Option<EngineConnection>>,
    cancel: CancellationToken,
) {
    let mut generation = 1_u64;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                process.shutdown().await;
                return;
            }
            status = process.wait() => {
                let _ = connection.send(None);
                match status {
                    Ok(status) => eprintln!("opencode serve exited with {status}; restarting"),
                    Err(error) => eprintln!("failed waiting for opencode serve: {error}; restarting"),
                }
            }
        }
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = sleep(RESTART_DELAY) => {}
            }
            generation = generation.saturating_add(1);
            match spawn_checked(generation, &binary).await {
                Ok((next_process, next_connection)) => {
                    process = next_process;
                    let _ = connection.send(Some(next_connection));
                    break;
                }
                Err(error) => eprintln!("failed to restart opencode serve: {error}"),
            }
        }
    }
}

async fn spawn_checked(
    generation: u64,
    binary: &Path,
) -> Result<(OpenCodeProcess, EngineConnection), OpenCodeError> {
    let mut process = OpenCodeProcess::start(binary.as_os_str()).await?;
    let client = OpenCodeClient::new(process.base_url.clone());
    let health = client.health().await?;
    if !health.healthy {
        process.shutdown().await;
        return Err(OpenCodeError::Unhealthy);
    }
    // health 返回非 semver 本身就是契约被破坏，不是"版本不合"。
    let version = match Version::parse(&health.version) {
        Ok(version) => version,
        Err(error) => {
            process.shutdown().await;
            return Err(OpenCodeError::InvalidVersion {
                actual: health.version,
                message: error.to_string(),
            });
        }
    };
    if let Err(error) = client.probe_contract().await {
        process.shutdown().await;
        return Err(error);
    }
    let pid = process.child.id();
    Ok((
        process,
        EngineConnection {
            generation,
            pid,
            version,
            client,
        },
    ))
}

struct OpenCodeProcess {
    child: Child,
    base_url: String,
}

impl OpenCodeProcess {
    async fn start(binary: &OsStr) -> Result<Self, OpenCodeError> {
        let mut child = Command::new(binary)
            .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().ok_or(OpenCodeError::MissingPipe)?;
        let stderr = child.stderr.take().ok_or(OpenCodeError::MissingPipe)?;
        let (line_tx, mut line_rx) = mpsc::unbounded_channel();
        spawn_line_reader("stdout", stdout, line_tx.clone());
        spawn_line_reader("stderr", stderr, line_tx);
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut output = Vec::new();
        let base_url = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(OpenCodeError::StartupTimeout { output });
            }
            let line = timeout(remaining, line_rx.recv())
                .await
                .map_err(|_| OpenCodeError::StartupTimeout {
                    output: output.clone(),
                })?
                .ok_or_else(|| OpenCodeError::ExitedDuringStartup {
                    output: output.clone(),
                })?;
            output.push(format!("{}: {}", line.0, line.1));
            if let Some(url) = listening_url(&line.1) {
                break url;
            }
        };
        Ok(Self { child, base_url })
    }

    async fn wait(&mut self) -> Result<std::process::ExitStatus, std::io::Error> {
        self.child.wait().await
    }

    async fn shutdown(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill().await;
        }
        let _ = self.child.wait().await;
    }
}

fn spawn_line_reader<R>(
    source: &'static str,
    reader: R,
    sender: mpsc::UnboundedSender<(&'static str, String)>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // Keep draining after startup even though the ready-line receiver has gone away.
            // Closing a long-lived child's stdout/stderr pipes can otherwise surface as EPIPE.
            let _ = sender.send((source, line));
        }
    });
}

fn listening_url(line: &str) -> Option<String> {
    const PREFIX: &str = "opencode server listening on ";
    let start = line.find(PREFIX)? + PREFIX.len();
    Some(line[start..].trim().trim_end_matches('/').to_string())
}

async fn parse_json_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, OpenCodeError> {
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    Ok(response.json().await?)
}

async fn response_error(response: reqwest::Response) -> OpenCodeError {
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                value.to_str().unwrap_or("<non-UTF-8>").to_string(),
            )
        })
        .collect();
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("<failed to read body: {error}>"));
    OpenCodeError::Http {
        status,
        headers,
        body,
    }
}

fn normalize_global_event(raw: Value) -> Result<GlobalEvent, OpenCodeError> {
    let payload = raw
        .get("payload")
        .cloned()
        .ok_or_else(|| OpenCodeError::InvalidEvent(raw.clone()))?;
    Ok(GlobalEvent {
        directory: raw
            .get("directory")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        project: raw
            .get("project")
            .and_then(Value::as_str)
            .map(str::to_string),
        payload,
    })
}

fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let end = buffer.windows(2).position(|window| window == b"\n\n")?;
    let frame = buffer.drain(..end).collect();
    buffer.drain(..2);
    Some(frame)
}

fn sse_data(frame: &[u8]) -> Result<Option<String>, std::str::Utf8Error> {
    let text = std::str::from_utf8(frame)?;
    let data = text
        .lines()
        .filter_map(|line| {
            line.strip_suffix('\r')
                .unwrap_or(line)
                .strip_prefix("data:")
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>();
    Ok((!data.is_empty()).then(|| data.join("\n")))
}

#[derive(Debug, Error)]
pub enum OpenCodeError {
    #[error("failed to start or communicate with opencode: {0}")]
    Io(#[from] std::io::Error),
    #[error("OpenCode HTTP transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("OpenCode JSON was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("OpenCode SSE contained invalid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("opencode startup self-check failed at [{probe}]: {detail}")]
    ContractUnsatisfied { probe: &'static str, detail: String },
    #[error("installed opencode version {actual:?} is not valid semver: {message}")]
    InvalidVersion { actual: String, message: String },
    #[error("opencode health endpoint reported unhealthy")]
    Unhealthy,
    #[error("opencode did not announce a listening URL within 30 seconds; output: {output:?}")]
    StartupTimeout { output: Vec<String> },
    #[error("opencode exited before announcing a listening URL; output: {output:?}")]
    ExitedDuringStartup { output: Vec<String> },
    #[error("opencode process output pipe was unavailable")]
    MissingPipe,
    #[error("OpenCode API returned {status}: {body}; headers={headers:?}")]
    Http {
        status: StatusCode,
        headers: BTreeMap<String, String>,
        body: String,
    },
    #[error("OpenCode global event stream closed")]
    EventStreamClosed,
    #[error("OpenCode global event wrapper was invalid: {0}")]
    InvalidEvent(Value),
    #[error("OpenCode is restarting and temporarily unavailable")]
    Unavailable,
    #[error("failed to persist OpenCode session identity: {0}")]
    Persistence(String),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{listening_url, normalize_global_event, sse_data, take_sse_frame};

    #[test]
    fn parses_ready_line_and_split_sse_frames() {
        assert_eq!(
            listening_url("opencode server listening on http://127.0.0.1:4096"),
            Some("http://127.0.0.1:4096".to_string())
        );
        let mut buffer = b"data: {\"type\":\"one\"}\n\ndata: {\"type\":".to_vec();
        let first = take_sse_frame(&mut buffer).unwrap();
        assert_eq!(
            sse_data(&first).unwrap().as_deref(),
            Some("{\"type\":\"one\"}")
        );
        assert!(take_sse_frame(&mut buffer).is_none());
    }

    #[test]
    fn normalizes_global_wrapper_without_losing_directory() {
        let event = normalize_global_event(json!({
            "directory": "/tmp/alice",
            "project": "global",
            "payload": {"type": "permission.asked", "properties": {"sessionID": "ses_1"}}
        }))
        .unwrap();
        assert_eq!(
            event.directory.as_deref().and_then(Path::to_str),
            Some("/tmp/alice")
        );
        assert_eq!(event.event_type(), Some("permission.asked"));
        assert_eq!(event.session_id(), Some("ses_1"));
    }
}
