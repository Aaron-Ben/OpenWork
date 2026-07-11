use crate::error::map_reqwest_error;
use futures_util::StreamExt;
use openwork_protocol::model::ModelError;
use reqwest::Response;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseFramer {
    buffer: Vec<u8>,
}

impl SseFramer {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseFrame>, ModelError> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while let Some((index, delimiter_len)) = find_event_boundary_bytes(&self.buffer) {
            let bytes = self.buffer.drain(..index).collect::<Vec<_>>();
            self.buffer.drain(..delimiter_len);
            if let Some(frame) = parse_frame(&bytes)? {
                frames.push(frame);
            }
        }
        Ok(frames)
    }

    pub fn finish(&mut self) -> Result<Vec<SseFrame>, ModelError> {
        if self.buffer.iter().all(u8::is_ascii_whitespace) {
            self.buffer.clear();
            return Ok(Vec::new());
        }
        let bytes = std::mem::take(&mut self.buffer);
        Ok(parse_frame(&bytes)?.into_iter().collect())
    }
}

pub(crate) async fn consume_sse_response<F>(
    response: Response,
    mut on_data: F,
) -> Result<(), ModelError>
where
    F: FnMut(&str) -> Result<bool, ModelError>,
{
    let mut stream = response.bytes_stream();
    let mut framer = SseFramer::default();
    let mut terminal_seen = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_reqwest_error)?;
        for frame in framer.push(&chunk)? {
            terminal_seen |= on_data(&frame.data)?;
        }
    }

    for frame in framer.finish()? {
        terminal_seen |= on_data(&frame.data)?;
    }

    if terminal_seen {
        Ok(())
    } else {
        Err(ModelError::network(
            "provider stream ended before a terminal event",
        ))
    }
}

fn parse_frame(bytes: &[u8]) -> Result<Option<SseFrame>, ModelError> {
    let event = std::str::from_utf8(bytes)
        .map_err(|error| ModelError::protocol(format!("invalid UTF-8 in SSE frame: {error}")))?;
    let mut event_name = None;
    let mut id = None;
    let mut data = Vec::new();
    for line in event.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start().to_string());
        }
    }
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some(SseFrame {
        event: event_name,
        id,
        data: data.join("\n"),
    }))
}

fn find_event_boundary_bytes(buffer: &[u8]) -> Option<(usize, usize)> {
    for index in 0..buffer.len() {
        if buffer[index..].starts_with(b"\r\n\r\n") {
            return Some((index, 4));
        }
        if buffer[index..].starts_with(b"\n\n") {
            return Some((index, 2));
        }
    }
    None
}
