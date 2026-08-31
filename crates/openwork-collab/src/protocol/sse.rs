use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use super::InvalidationEvent;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, SseParseError> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((end, delimiter_len)) = find_event_boundary(&self.buffer) {
            let block = self.buffer.drain(..end).collect::<Vec<_>>();
            self.buffer.drain(..delimiter_len);
            let block = std::str::from_utf8(&block)?;
            if let Some(event) = parse_event(block) {
                events.push(event);
            }
        }
        Ok(events)
    }
}

pub async fn reconnecting_invalidation_loop<C, F>(
    http: reqwest::Client,
    url: String,
    credential: C,
    event_name: &'static str,
    shutdown: CancellationToken,
    mut on_event: F,
) where
    C: Fn() -> String,
    F: FnMut(InvalidationEvent) -> bool,
{
    let mut backoff = Duration::from_secs(1);
    loop {
        if shutdown.is_cancelled() {
            return;
        }
        let connected_at = Instant::now();
        match invalidation_stream_once(
            &http,
            &url,
            &credential(),
            event_name,
            &shutdown,
            &mut on_event,
        )
        .await
        {
            Ok(StreamOutcome::Stopped) => return,
            Ok(StreamOutcome::Disconnected) => {}
            Err(error) => {
                tracing::warn!(%error, %event_name, "Collaboration SSE disconnected");
            }
        }
        if shutdown.is_cancelled() {
            return;
        }
        if connected_at.elapsed() >= Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn invalidation_stream_once<F>(
    http: &reqwest::Client,
    url: &str,
    token: &str,
    event_name: &str,
    shutdown: &CancellationToken,
    on_event: &mut F,
) -> Result<StreamOutcome, SseStreamError>
where
    F: FnMut(InvalidationEvent) -> bool,
{
    let mut response = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?;
    let mut decoder = SseDecoder::default();
    loop {
        let chunk = tokio::select! {
            _ = shutdown.cancelled() => return Ok(StreamOutcome::Stopped),
            chunk = response.chunk() => chunk?,
        };
        let Some(chunk) = chunk else {
            return Ok(StreamOutcome::Disconnected);
        };
        for event in decoder.push(&chunk)? {
            if event.event.as_deref() != Some(event_name) {
                continue;
            }
            let invalidation = serde_json::from_str::<InvalidationEvent>(&event.data)?;
            if !on_event(invalidation) {
                return Ok(StreamOutcome::Stopped);
            }
        }
    }
}

enum StreamOutcome {
    Disconnected,
    Stopped,
}

fn find_event_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}

fn parse_event(block: &str) -> Option<SseEvent> {
    let mut event = SseEvent::default();
    let mut data = Vec::new();
    for line in block.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event.event = Some(value.to_string()),
            "id" => event.id = Some(value.to_string()),
            "data" => data.push(value),
            _ => {}
        }
    }
    if data.is_empty() {
        return None;
    }
    event.data = data.join("\n");
    Some(event)
}

#[derive(Debug, thiserror::Error)]
pub enum SseParseError {
    #[error("SSE stream contained invalid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

#[derive(Debug, thiserror::Error)]
enum SseStreamError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SSE stream was invalid: {0}")]
    Parse(#[from] SseParseError),
    #[error("SSE payload was invalid: {0}")]
    Json(#[from] serde_json::Error),
}
