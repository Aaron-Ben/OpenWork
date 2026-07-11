use futures_util::StreamExt;
use openwork_protocol::model::{ModelError, ModelErrorCode, ModelFailurePhase, RetryHint};
use reqwest::{Response, StatusCode};
use serde_json::Value;
use std::time::SystemTime;
use time::{OffsetDateTime, format_description::well_known::Rfc2822};

const MAX_ERROR_BODY_CHARS: usize = 4_096;
const MAX_ERROR_BODY_BYTES: usize = 4_096;
const MAX_ERROR_CODE_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorDialect {
    OpenAi,
    Anthropic,
    DeepSeek,
    Kimi,
    Qwen,
    Glm,
}

pub fn map_reqwest_error(error: reqwest::Error) -> ModelError {
    if error.is_timeout() {
        return ModelError::timeout();
    }

    if error.is_decode() {
        return ModelError::protocol(error.to_string());
    }

    ModelError::network(error.to_string())
}

pub(crate) fn decode_stream_json(data: &str) -> Result<Value, ModelError> {
    serde_json::from_str(data).map_err(|error| ModelError::protocol(error.to_string()))
}

pub(crate) async fn map_error_response_for(
    response: Response,
    dialect: ErrorDialect,
) -> ModelError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let retry_after_ms_header = response
        .headers()
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let retry_after_ms = retry_after_millis(
        retry_after.as_deref(),
        retry_after_ms_header.as_deref(),
        SystemTime::now(),
    );
    let header_request_id = request_id_from_headers(response.headers());
    let body = read_bounded_error_body(response).await;

    classify_http_error_for_dialect(dialect, status, &body, retry_after_ms, header_request_id)
}

async fn read_bounded_error_body(response: Response) -> String {
    let mut bytes = Vec::with_capacity(MAX_ERROR_BODY_BYTES);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            break;
        };
        append_bounded_error_bytes(&mut bytes, &chunk);
        if bytes.len() == MAX_ERROR_BODY_BYTES {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn append_bounded_error_bytes(body: &mut Vec<u8>, chunk: &[u8]) {
    let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(body.len());
    body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
}

pub(crate) fn request_id_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "request-id"].into_iter().find_map(|name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(truncate_code)
    })
}

fn retry_after_millis(
    retry_after: Option<&str>,
    retry_after_ms: Option<&str>,
    now: SystemTime,
) -> Option<u64> {
    if let Some(milliseconds) = retry_after_ms.and_then(|value| value.trim().parse::<u64>().ok()) {
        return Some(milliseconds);
    }
    let value = retry_after?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1_000));
    }
    let target = OffsetDateTime::parse(value, &Rfc2822).ok()?;
    let now = OffsetDateTime::from(now);
    let milliseconds = (target - now).whole_milliseconds();
    Some(milliseconds.max(0).min(u64::MAX as i128) as u64)
}

#[cfg(test)]
pub(crate) fn map_stream_error_event(event: &Value) -> Option<ModelError> {
    map_stream_error_event_for(event, ErrorDialect::OpenAi)
}

pub(crate) fn map_stream_error_event_for(
    event: &Value,
    dialect: ErrorDialect,
) -> Option<ModelError> {
    let is_error = matches!(
        event.get("type").and_then(Value::as_str),
        Some("error") | Some("response.failed")
    ) || event.get("error").is_some_and(|error| !error.is_null());
    if !is_error {
        return None;
    }

    let provider_code = provider_code_from_body(event);
    let message = provider_message_from_body(event)
        .unwrap_or_else(|| "provider returned an SSE error event".to_string());
    let signal = format!(
        "{} {}",
        provider_code.as_deref().unwrap_or_default(),
        message
    )
    .to_ascii_lowercase();
    let provider_request_id = event
        .get("request_id")
        .or_else(|| event.get("requestId"))
        .and_then(Value::as_str)
        .map(truncate_code);
    let (kind, retry_hint) =
        classify_dialect(dialect, StatusCode::OK, provider_code.as_deref(), None)
            .unwrap_or_else(|| classify_stream_signal(&signal));

    let mut error = ModelError::http(
        kind,
        StatusCode::OK.as_u16(),
        message,
        provider_code,
        provider_request_id,
        retry_hint,
    );
    error.phase = ModelFailurePhase::StreamDecode;
    Some(error)
}

#[cfg(test)]
fn classify_http_error(
    status: StatusCode,
    body: &str,
    retry_after_ms: Option<u64>,
    header_request_id: Option<String>,
) -> ModelError {
    classify_http_error_for_dialect(
        ErrorDialect::OpenAi,
        status,
        body,
        retry_after_ms,
        header_request_id,
    )
}

fn classify_http_error_for_dialect(
    dialect: ErrorDialect,
    status: StatusCode,
    body: &str,
    retry_after_ms: Option<u64>,
    header_request_id: Option<String>,
) -> ModelError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let provider_code = parsed.as_ref().and_then(provider_code_from_body);
    let message = parsed
        .as_ref()
        .and_then(provider_message_from_body)
        .unwrap_or_else(|| truncate_body(body));
    let provider_request_id = header_request_id.or_else(|| {
        parsed.as_ref().and_then(|value| {
            value
                .get("request_id")
                .or_else(|| value.get("requestId"))
                .and_then(Value::as_str)
                .map(truncate_code)
        })
    });
    let signal = format!(
        "{} {}",
        provider_code.as_deref().unwrap_or_default(),
        message
    )
    .to_ascii_lowercase();

    let (kind, retry_hint) =
        classify_dialect(dialect, status, provider_code.as_deref(), retry_after_ms)
            .unwrap_or_else(|| classify_status(status, &signal, retry_after_ms));
    ModelError::http(
        kind,
        status.as_u16(),
        if message.is_empty() {
            format!("provider returned HTTP {}", status.as_u16())
        } else {
            message
        },
        provider_code,
        provider_request_id,
        retry_hint,
    )
}

fn classify_dialect(
    dialect: ErrorDialect,
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match dialect {
        ErrorDialect::OpenAi => {
            crate::adapters::openai_responses::error::classify(status, code, retry_after_ms)
        }
        ErrorDialect::Anthropic => {
            crate::adapters::anthropic_messages::error::classify(status, code, retry_after_ms)
        }
        ErrorDialect::DeepSeek => crate::adapters::openai_chat::dialect::deepseek::classify_error(
            status,
            code,
            retry_after_ms,
        ),
        ErrorDialect::Kimi => crate::adapters::openai_chat::dialect::kimi::classify_error(
            status,
            code,
            retry_after_ms,
        ),
        ErrorDialect::Qwen => crate::adapters::openai_chat::dialect::qwen::classify_error(
            status,
            code,
            retry_after_ms,
        ),
        ErrorDialect::Glm => {
            crate::adapters::openai_chat::dialect::glm::classify_error(status, code, retry_after_ms)
        }
    }
}

fn classify_status(
    status: StatusCode,
    signal: &str,
    retry_after_ms: Option<u64>,
) -> (ModelErrorCode, RetryHint) {
    let quota_exhausted = [
        "quota",
        "billing",
        "balance",
        "insufficient",
        "credit",
        "resource package",
    ]
    .iter()
    .any(|needle| signal.contains(needle));
    let overloaded = ["overload", "engine_busy", "service unavailable"]
        .iter()
        .any(|needle| signal.contains(needle));
    let content_filtered = ["content_filter", "content filter", "safety"]
        .iter()
        .any(|needle| signal.contains(needle));

    if content_filtered {
        return (ModelErrorCode::ContentFiltered, RetryHint::Never);
    }

    match status {
        StatusCode::UNAUTHORIZED => (ModelErrorCode::Authentication, RetryHint::Never),
        StatusCode::PAYMENT_REQUIRED => (ModelErrorCode::QuotaExhausted, RetryHint::Never),
        StatusCode::FORBIDDEN => (ModelErrorCode::PermissionDenied, RetryHint::Never),
        StatusCode::NOT_FOUND => (ModelErrorCode::ModelNotFound, RetryHint::Never),
        StatusCode::REQUEST_TIMEOUT => (ModelErrorCode::Timeout, RetryHint::Backoff),
        StatusCode::TOO_MANY_REQUESTS if quota_exhausted => {
            (ModelErrorCode::QuotaExhausted, RetryHint::Never)
        }
        StatusCode::TOO_MANY_REQUESTS if overloaded => (
            ModelErrorCode::Overloaded,
            retry_after_ms
                .map(RetryHint::AfterMillis)
                .unwrap_or(RetryHint::Backoff),
        ),
        StatusCode::TOO_MANY_REQUESTS => (
            ModelErrorCode::RateLimited,
            retry_after_ms
                .map(RetryHint::AfterMillis)
                .unwrap_or(RetryHint::Backoff),
        ),
        status if status.as_u16() == 529 => (
            ModelErrorCode::Overloaded,
            retry_after_ms
                .map(RetryHint::AfterMillis)
                .unwrap_or(RetryHint::Backoff),
        ),
        status if status.is_server_error() => (ModelErrorCode::ServerError, RetryHint::Backoff),
        StatusCode::BAD_REQUEST
        | StatusCode::CONFLICT
        | StatusCode::PAYLOAD_TOO_LARGE
        | StatusCode::UNPROCESSABLE_ENTITY => (ModelErrorCode::InvalidRequest, RetryHint::Never),
        _ => (ModelErrorCode::Unknown, RetryHint::CallerDecision),
    }
}

fn classify_stream_signal(signal: &str) -> (ModelErrorCode, RetryHint) {
    if ["quota", "billing", "balance", "insufficient", "credit"]
        .iter()
        .any(|needle| signal.contains(needle))
    {
        return (ModelErrorCode::QuotaExhausted, RetryHint::Never);
    }
    if ["rate_limit", "rate limit", "too many requests"]
        .iter()
        .any(|needle| signal.contains(needle))
    {
        return (ModelErrorCode::RateLimited, RetryHint::Backoff);
    }
    if ["overload", "engine_busy", "service unavailable"]
        .iter()
        .any(|needle| signal.contains(needle))
    {
        return (ModelErrorCode::Overloaded, RetryHint::Backoff);
    }
    if signal.contains("auth") || signal.contains("api key") {
        return (ModelErrorCode::Authentication, RetryHint::Never);
    }
    if signal.contains("permission") || signal.contains("forbidden") {
        return (ModelErrorCode::PermissionDenied, RetryHint::Never);
    }
    if ["content_filter", "content filter", "safety"]
        .iter()
        .any(|needle| signal.contains(needle))
    {
        return (ModelErrorCode::ContentFiltered, RetryHint::Never);
    }
    (ModelErrorCode::ServerError, RetryHint::Backoff)
}

fn provider_code_from_body(body: &Value) -> Option<String> {
    body.pointer("/error/code")
        .or_else(|| body.pointer("/error/type"))
        .or_else(|| body.pointer("/response/error/code"))
        .or_else(|| body.pointer("/response/error/type"))
        .or_else(|| body.get("code"))
        .or_else(|| body.get("type"))
        .and_then(|value| match value {
            Value::String(value) => Some(value.chars().take(MAX_ERROR_CODE_CHARS).collect()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

fn provider_message_from_body(body: &Value) -> Option<String> {
    body.pointer("/error/message")
        .or_else(|| body.pointer("/response/error/message"))
        .or_else(|| body.get("message"))
        .and_then(Value::as_str)
        .map(truncate_body)
}

fn truncate_body(body: &str) -> String {
    body.chars().take(MAX_ERROR_BODY_CHARS).collect()
}

fn truncate_code(value: &str) -> String {
    value.chars().take(MAX_ERROR_CODE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn classifies_retryable_rate_limit_and_preserves_request_id() {
        let error = classify_http_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"type":"rate_limit_error","message":"too many requests"}}"#,
            Some(2_000),
            Some("req_123".to_string()),
        );

        assert_eq!(error.code(), ModelErrorCode::RateLimited);
        assert_eq!(error.retry_hint(), RetryHint::AfterMillis(2_000));
        assert_eq!(error.provider_request_id(), Some("req_123"));
    }

    #[test]
    fn does_not_retry_quota_exhaustion_disguised_as_429() {
        let error = classify_http_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":"insufficient_quota","message":"billing quota exhausted"}}"#,
            None,
            None,
        );

        assert_eq!(error.code(), ModelErrorCode::QuotaExhausted);
        assert_eq!(error.retry_hint(), RetryHint::Never);
    }

    #[test]
    fn anthropic_overload_is_retryable() {
        let error = classify_http_error(
            StatusCode::from_u16(529).unwrap(),
            r#"{"type":"error","error":{"type":"overloaded_error","message":"overloaded"},"request_id":"req_529"}"#,
            None,
            None,
        );

        assert_eq!(error.code(), ModelErrorCode::Overloaded);
        assert_eq!(error.retry_hint(), RetryHint::Backoff);
        assert_eq!(error.provider_request_id(), Some("req_529"));
    }

    #[test]
    fn maps_mid_stream_error_even_after_http_200() {
        let error = map_stream_error_event(&serde_json::json!({
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": "service overloaded"
            },
            "request_id": "req_stream"
        }))
        .unwrap();

        assert_eq!(error.code(), ModelErrorCode::Overloaded);
        assert_eq!(error.retry_hint(), RetryHint::Backoff);
        assert_eq!(error.provider_request_id(), Some("req_stream"));
        assert_eq!(
            error.phase,
            openwork_protocol::model::ModelFailurePhase::StreamDecode
        );
    }

    #[test]
    fn parses_retry_after_delta_seconds_milliseconds_and_http_date() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        assert_eq!(retry_after_millis(Some("2"), None, now), Some(2_000));
        assert_eq!(retry_after_millis(None, Some("250"), now), Some(250));
        assert_eq!(
            retry_after_millis(Some("Tue, 14 Nov 2023 22:13:25 GMT"), None, now),
            Some(5_000)
        );
    }

    #[test]
    fn classifies_vendor_codes_without_english_message_guessing() {
        let glm_billing = classify_http_error_for_dialect(
            ErrorDialect::Glm,
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":"1113","message":"您的账户已欠费"}}"#,
            None,
            None,
        );
        assert_eq!(glm_billing.code(), ModelErrorCode::QuotaExhausted);
        assert_eq!(glm_billing.retry_hint(), RetryHint::Never);

        let qwen_throttle = classify_http_error_for_dialect(
            ErrorDialect::Qwen,
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":"Throttling.AllocationQuota","message":"TPS limit"}}"#,
            None,
            None,
        );
        assert_eq!(qwen_throttle.code(), ModelErrorCode::RateLimited);
        assert_eq!(qwen_throttle.retry_hint(), RetryHint::Backoff);

        let kimi_quota = classify_http_error_for_dialect(
            ErrorDialect::Kimi,
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"type":"exceeded_current_quota_error","message":"账户额度不足"}}"#,
            None,
            None,
        );
        assert_eq!(kimi_quota.code(), ModelErrorCode::QuotaExhausted);
        assert_eq!(kimi_quota.retry_hint(), RetryHint::Never);
    }

    #[test]
    fn bounds_error_body_before_decoding() {
        let mut body = Vec::new();
        append_bounded_error_bytes(&mut body, &[b'a'; 3_000]);
        append_bounded_error_bytes(&mut body, &[b'b'; 3_000]);

        assert_eq!(body.len(), MAX_ERROR_BODY_BYTES);
        assert_eq!(body[2_999], b'a');
        assert_eq!(body[3_000], b'b');
    }
}
