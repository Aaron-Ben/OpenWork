use std::{
    collections::BTreeMap,
    env,
    error::Error,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use reqwest::{RequestBuilder, StatusCode};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
    time::{Instant, timeout},
};

pub type SpikeError = Box<dyn Error + Send + Sync>;
pub type SpikeResult<T> = Result<T, SpikeError>;

pub const DIRECTORY_HEADER: &str = "x-opencode-directory";

pub fn message_error(message: impl Into<String>) -> SpikeError {
    io::Error::other(message.into()).into()
}

pub struct OpenCodeServer {
    child: Child,
    base_url: String,
    startup_lines: Vec<String>,
}

impl OpenCodeServer {
    pub async fn start() -> SpikeResult<Self> {
        let binary = env::var_os("OPENCODE_BIN").unwrap_or_else(|| "opencode".into());
        let mut child = Command::new(binary)
            .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| message_error("opencode stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| message_error("opencode stderr was not piped"))?;
        let (line_tx, mut line_rx) = mpsc::unbounded_channel();
        spawn_line_reader("stdout", stdout, line_tx.clone());
        spawn_line_reader("stderr", stderr, line_tx);

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut startup_lines = Vec::new();
        let base_url = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(message_error(format!(
                    "timed out waiting for opencode startup; output: {startup_lines:?}"
                )));
            }
            let received = timeout(remaining, line_rx.recv())
                .await
                .map_err(|_| message_error("timed out waiting for opencode startup output"))?;
            let Some((source, line)) = received else {
                let status = child.try_wait()?;
                return Err(message_error(format!(
                    "opencode exited before announcing its URL ({status:?}); output: {startup_lines:?}"
                )));
            };
            println!("[opencode {source}] {line}");
            startup_lines.push(format!("{source}: {line}"));
            if let Some(url) = listening_url(&line) {
                break url;
            }
        };

        Ok(Self {
            child,
            base_url,
            startup_lines,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn startup_lines(&self) -> &[String] {
        &self.startup_lines
    }

    pub async fn shutdown(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill().await;
        }
        let _ = self.child.wait().await;
    }
}

impl Drop for OpenCodeServer {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn spawn_line_reader<R>(
    source: &'static str,
    reader: R,
    line_tx: mpsc::UnboundedSender<(&'static str, String)>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line_tx.send((source, line)).is_err() {
                break;
            }
        }
    });
}

fn listening_url(line: &str) -> Option<String> {
    const PREFIX: &str = "opencode server listening on ";
    let start = line.find(PREFIX)? + PREFIX.len();
    Some(line[start..].trim().trim_end_matches('/').to_string())
}

#[derive(Clone)]
pub struct OpenCodeClient {
    http: reqwest::Client,
    base_url: String,
    directory: PathBuf,
    password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpEvidence {
    pub status: StatusCode,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl HttpEvidence {
    pub fn print(&self, label: &str) {
        println!("{label}.status={}", self.status);
        println!("{label}.headers={:?}", self.headers);
        println!("{label}.body={}", self.body);
    }
}

impl OpenCodeClient {
    pub fn new(base_url: impl Into<String>, directory: impl Into<PathBuf>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            directory: directory.into(),
            password: env::var("OPENCODE_SERVER_PASSWORD").ok(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub async fn health(&self) -> SpikeResult<Value> {
        let path = "/global/health";
        println!("GET {path}");
        let response = self.request(self.http.get(self.url(path))).send().await?;
        ensure_success(response).await
    }

    pub async fn create_session(&self, title: &str) -> SpikeResult<Value> {
        let path = "/session";
        let body = json!({"title": title});
        println!("POST {path}");
        println!(
            "request.header.{DIRECTORY_HEADER}={}",
            self.directory.display()
        );
        println!("request.body={body}");
        let response = self
            .request(self.http.post(self.url(path)))
            .json(&body)
            .send()
            .await?;
        ensure_success(response).await
    }

    pub async fn get_session(&self, session_id: &str) -> SpikeResult<Value> {
        let path = format!("/session/{session_id}");
        println!("GET {path}");
        let response = self.request(self.http.get(self.url(&path))).send().await?;
        ensure_success(response).await
    }

    pub async fn get_messages(&self, session_id: &str) -> SpikeResult<Value> {
        let path = format!("/session/{session_id}/message");
        println!("GET {path}");
        let response = self.request(self.http.get(self.url(&path))).send().await?;
        ensure_success(response).await
    }

    pub async fn prompt_async(&self, session_id: &str, text: &str) -> SpikeResult<HttpEvidence> {
        let path = format!("/session/{session_id}/prompt_async");
        let body = prompt_body(text);
        println!("POST {path}");
        println!(
            "request.header.{DIRECTORY_HEADER}={}",
            self.directory.display()
        );
        println!("request.body={body}");
        let response = self
            .request(self.http.post(self.url(&path)))
            .json(&body)
            .send()
            .await?;
        response_evidence(response).await
    }

    pub async fn reply_permission(
        &self,
        permission_id: &str,
        reply: &str,
        message: Option<&str>,
    ) -> SpikeResult<HttpEvidence> {
        let path = format!("/permission/{permission_id}/reply");
        let mut body = json!({"reply": reply});
        if let Some(message) = message {
            body["message"] = Value::String(message.to_string());
        }
        println!("POST {path}");
        println!("request.body={body}");
        let response = self
            .request(self.http.post(self.url(&path)))
            .json(&body)
            .send()
            .await?;
        response_evidence(response).await
    }

    pub async fn event_stream(&self) -> SpikeResult<SseStream> {
        let path = "/event";
        println!("GET {path}");
        println!(
            "request.header.{DIRECTORY_HEADER}={}",
            self.directory.display()
        );
        let response = self
            .request(self.http.get(self.url(path)))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        if !response.status().is_success() {
            let evidence = response_evidence(response).await?;
            return Err(message_error(format!(
                "event subscription failed: {evidence:?}"
            )));
        }
        Ok(SseStream {
            response,
            buffer: Vec::new(),
        })
    }

    pub async fn probe_path(&self, path: &str) -> SpikeResult<HttpEvidence> {
        let response = self.request(self.http.get(self.url(path))).send().await?;
        response_evidence(response).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request(&self, builder: RequestBuilder) -> RequestBuilder {
        let builder = builder.header(DIRECTORY_HEADER, self.directory.to_string_lossy().as_ref());
        match &self.password {
            Some(password) => builder.basic_auth("opencode", Some(password)),
            None => builder,
        }
    }
}

pub fn prompt_body(text: &str) -> Value {
    json!({"parts": [{"type": "text", "text": text}]})
}

async fn ensure_success(response: reqwest::Response) -> SpikeResult<Value> {
    let evidence = response_evidence(response).await?;
    if !evidence.status.is_success() {
        return Err(message_error(format!(
            "OpenCode API returned {}: {}",
            evidence.status, evidence.body
        )));
    }
    Ok(serde_json::from_str(&evidence.body)?)
}

async fn response_evidence(response: reqwest::Response) -> SpikeResult<HttpEvidence> {
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
    let body = response.text().await?;
    Ok(HttpEvidence {
        status,
        headers,
        body,
    })
}

pub struct SseStream {
    response: reqwest::Response,
    buffer: Vec<u8>,
}

impl SseStream {
    pub async fn next_json(&mut self, wait: Duration) -> SpikeResult<Value> {
        timeout(wait, self.next_json_inner())
            .await
            .map_err(|_| message_error(format!("timed out after {wait:?} waiting for SSE event")))?
    }

    async fn next_json_inner(&mut self) -> SpikeResult<Value> {
        loop {
            if let Some(frame) = take_sse_frame(&mut self.buffer) {
                if let Some(data) = sse_data(&frame)? {
                    return Ok(serde_json::from_str(&data)?);
                }
                continue;
            }
            let chunk = self
                .response
                .chunk()
                .await?
                .ok_or_else(|| message_error("OpenCode closed the SSE event stream"))?;
            self.buffer.extend_from_slice(&chunk);
        }
    }
}

fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let end = buffer.windows(2).position(|window| window == b"\n\n")?;
    let frame = buffer.drain(..end).collect();
    buffer.drain(..2);
    Some(frame)
}

fn sse_data(frame: &[u8]) -> SpikeResult<Option<String>> {
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

#[derive(Debug, Default)]
pub struct TurnEvidence {
    pub events: Vec<Value>,
    pub texts: BTreeMap<String, (String, String)>,
    pub assistant_messages: BTreeMap<String, Value>,
    pub user_messages: BTreeMap<String, Value>,
    pub saw_busy: bool,
    pub saw_idle: bool,
}

impl TurnEvidence {
    pub fn observe(&mut self, session_id: &str, event: Value) {
        if event_session_id(&event) != Some(session_id) {
            return;
        }
        let event_type = event.get("type").and_then(Value::as_str);
        if !matches!(
            event_type,
            Some("message.part.delta" | "session.diff" | "session.updated")
        ) {
            println!("SSE {event}");
        }
        match event_type {
            Some("session.status") => match event
                .pointer("/properties/status/type")
                .and_then(Value::as_str)
            {
                Some("busy") => self.saw_busy = true,
                Some("idle") if self.saw_busy => self.saw_idle = true,
                _ => {}
            },
            Some("session.idle") if self.saw_busy => self.saw_idle = true,
            Some("message.updated") => {
                if let Some(info) = event.pointer("/properties/info")
                    && let Some(id) = info.get("id").and_then(Value::as_str)
                {
                    match info.get("role").and_then(Value::as_str) {
                        Some("assistant") => {
                            self.assistant_messages.insert(id.to_string(), info.clone());
                        }
                        Some("user") => {
                            self.user_messages.insert(id.to_string(), info.clone());
                        }
                        _ => {}
                    }
                }
            }
            Some("message.part.updated") => {
                if let Some(part) = event.pointer("/properties/part")
                    && part.get("type").and_then(Value::as_str) == Some("text")
                    && let (Some(part_id), Some(message_id), Some(text)) = (
                        part.get("id").and_then(Value::as_str),
                        part.get("messageID").and_then(Value::as_str),
                        part.get("text").and_then(Value::as_str),
                    )
                {
                    self.texts.insert(
                        part_id.to_string(),
                        (message_id.to_string(), text.to_string()),
                    );
                }
            }
            _ => {}
        }
        self.events.push(event);
    }

    pub fn final_assistant(&self) -> Option<&Value> {
        self.assistant_messages
            .values()
            .filter(|info| info.pointer("/time/completed").is_some())
            .max_by_key(|info| info.pointer("/time/created").and_then(Value::as_i64))
            .or_else(|| self.assistant_messages.values().next_back())
    }

    pub fn final_text(&self) -> String {
        let Some(message_id) = self
            .final_assistant()
            .and_then(|info| info.get("id"))
            .and_then(Value::as_str)
        else {
            return String::new();
        };
        self.texts
            .values()
            .filter(|(candidate, _)| candidate == message_id)
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn usage(&self) -> Option<&Value> {
        self.final_assistant()?.get("tokens")
    }
}

pub async fn collect_until_idle(
    stream: &mut SseStream,
    session_id: &str,
    max_wait: Duration,
) -> SpikeResult<TurnEvidence> {
    let deadline = Instant::now() + max_wait;
    let mut evidence = TurnEvidence::default();
    while !evidence.saw_idle {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(message_error(format!(
                "session {session_id} did not become idle; events={:?}",
                evidence.events
            )));
        }
        let event = stream.next_json(remaining).await?;
        evidence.observe(session_id, event);
    }
    Ok(evidence)
}

pub fn event_session_id(event: &Value) -> Option<&str> {
    event
        .pointer("/properties/sessionID")
        .or_else(|| event.pointer("/properties/info/sessionID"))
        .or_else(|| event.pointer("/properties/part/sessionID"))
        .and_then(Value::as_str)
}

pub fn session_id(session: &Value) -> SpikeResult<&str> {
    session
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| message_error(format!("session response has no id: {session}")))
}

pub fn print_turn_summary(label: &str, turn: &TurnEvidence) {
    println!("{label}.final_text={}", turn.final_text());
    println!(
        "{label}.usage={}",
        turn.usage().cloned().unwrap_or(Value::Null)
    );
    println!(
        "{label}.assistant={}",
        turn.final_assistant().cloned().unwrap_or(Value::Null)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_sse_frames() {
        let mut buffer = b"data: {\"type\":\"one\"}\n\ndata: {\"type\":".to_vec();
        let first = take_sse_frame(&mut buffer).expect("first frame");
        assert_eq!(
            sse_data(&first).unwrap().as_deref(),
            Some("{\"type\":\"one\"}")
        );
        assert!(take_sse_frame(&mut buffer).is_none());
        buffer.extend_from_slice(b"\"two\"}\n\n");
        let second = take_sse_frame(&mut buffer).expect("second frame");
        assert_eq!(
            sse_data(&second).unwrap().as_deref(),
            Some("{\"type\":\"two\"}")
        );
    }

    #[test]
    fn extracts_instance_url() {
        assert_eq!(
            listening_url("opencode server listening on http://127.0.0.1:4096"),
            Some("http://127.0.0.1:4096".to_string())
        );
    }
}
