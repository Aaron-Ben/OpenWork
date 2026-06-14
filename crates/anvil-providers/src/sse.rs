use futures_util::StreamExt;
use reqwest::Response;
use serde_json::Value;

use crate::error::map_reqwest_error;
use anvil_core::ai::ProviderError;

pub(crate) async fn consume_sse_response<F>(
    response: Response,
    mut on_data: F,
) -> Result<Vec<Value>, ProviderError>
where
    F: FnMut(&Value) -> Result<(), ProviderError>,
{
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_reqwest_error)?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some((index, delimiter_len)) = find_event_boundary(&buffer) {
            let event = buffer[..index].to_string();
            buffer.drain(..index + delimiter_len);

            for data in data_lines(&event) {
                if data == "[DONE]" {
                    return Ok(events);
                }
                let value = serde_json::from_str::<Value>(&data).map_err(|error| {
                    ProviderError::Serialization {
                        message: error.to_string(),
                    }
                })?;
                on_data(&value)?;
                events.push(value);
            }
        }
    }

    if !buffer.trim().is_empty() {
        for data in data_lines(&buffer) {
            if data == "[DONE]" {
                break;
            }
            let value = serde_json::from_str::<Value>(&data).map_err(|error| {
                ProviderError::Serialization {
                    message: error.to_string(),
                }
            })?;
            on_data(&value)?;
            events.push(value);
        }
    }

    Ok(events)
}

fn data_lines(event: &str) -> Vec<String> {
    event
        .lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            line.strip_prefix("data:")
                .map(|data| data.trim_start().to_string())
        })
        .collect()
}

fn find_event_boundary(buffer: &str) -> Option<(usize, usize)> {
    let lf = buffer.find("\n\n").map(|index| (index, 2));
    let crlf = buffer.find("\r\n\r\n").map(|index| (index, 4));

    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 < right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_data_lines_from_event() {
        let lines = data_lines("event: message\ndata: {\"x\":1}\n\n");

        assert_eq!(lines, vec!["{\"x\":1}".to_string()]);
    }

    #[test]
    fn finds_lf_and_crlf_boundaries() {
        assert_eq!(find_event_boundary("a\n\nb"), Some((1, 2)));
        assert_eq!(find_event_boundary("a\r\n\r\nb"), Some((1, 4)));
    }
}
