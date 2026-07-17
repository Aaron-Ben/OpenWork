//! OpenAI Responses exact error mapping.

use crate::model::{ModelErrorCode, RetryHint};
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
        Some("insufficient_quota") | Some("billing_hard_limit_reached") => {
            Some((ModelErrorCode::QuotaExhausted, RetryHint::Never))
        }
        Some("rate_limit_exceeded") | Some("rate_limit_error") => {
            Some((ModelErrorCode::RateLimited, retry()))
        }
        Some("invalid_api_key") => Some((ModelErrorCode::Authentication, RetryHint::Never)),
        Some("invalid_prompt") => Some((ModelErrorCode::InvalidRequest, RetryHint::Never)),
        _ if status == StatusCode::CONFLICT => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ => None,
    }
}
