//! Anthropic Messages exact error mapping.

use openwork_protocol::model::{ModelErrorCode, RetryHint};
use reqwest::StatusCode;

pub(crate) fn classify(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    let retry = || {
        retry_after_ms
            .map(RetryHint::AfterMillis)
            .unwrap_or(RetryHint::Backoff)
    };
    match code {
        Some("authentication_error") => Some((ModelErrorCode::Authentication, RetryHint::Never)),
        Some("billing_error") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("permission_error") => Some((ModelErrorCode::PermissionDenied, RetryHint::Never)),
        Some("not_found_error") => Some((ModelErrorCode::ModelNotFound, RetryHint::Never)),
        Some("invalid_request_error") | Some("request_too_large") => {
            Some((ModelErrorCode::InvalidRequest, RetryHint::Never))
        }
        Some("rate_limit_error") => Some((ModelErrorCode::RateLimited, retry())),
        Some("overloaded_error") => Some((ModelErrorCode::Overloaded, retry())),
        Some("api_error") | Some("timeout_error") => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ if status.as_u16() == 529 => Some((ModelErrorCode::Overloaded, retry())),
        _ => None,
    }
}
