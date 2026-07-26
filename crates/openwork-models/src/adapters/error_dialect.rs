//! Exact provider error-code mappings, separate from shared HTTP/SSE parsing.

use reqwest::StatusCode;

use crate::model::{ModelErrorCode, RetryHint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorDialect {
    OpenAi,
    Anthropic,
    DeepSeek,
    Kimi,
    Qwen,
    Glm,
}

pub(crate) fn classify(
    dialect: ErrorDialect,
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match dialect {
        ErrorDialect::OpenAi => classify_openai(status, code, retry_after_ms),
        ErrorDialect::Anthropic => classify_anthropic(status, code, retry_after_ms),
        ErrorDialect::DeepSeek => classify_deepseek(status, code, retry_after_ms),
        ErrorDialect::Kimi => classify_kimi(status, code, retry_after_ms),
        ErrorDialect::Qwen => classify_qwen(code, retry_after_ms),
        ErrorDialect::Glm => classify_glm(code, retry_after_ms),
    }
}

fn retry_hint(retry_after_ms: Option<u64>) -> RetryHint {
    retry_after_ms
        .map(RetryHint::AfterMillis)
        .unwrap_or(RetryHint::Backoff)
}

fn classify_openai(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match code {
        Some("context_length_exceeded") => {
            Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision))
        }
        Some("insufficient_quota") | Some("billing_hard_limit_reached") => {
            Some((ModelErrorCode::QuotaExhausted, RetryHint::Never))
        }
        Some("rate_limit_exceeded") | Some("rate_limit_error") => {
            Some((ModelErrorCode::RateLimited, retry_hint(retry_after_ms)))
        }
        Some("invalid_api_key") => Some((ModelErrorCode::Authentication, RetryHint::Never)),
        Some("invalid_prompt") => Some((ModelErrorCode::InvalidRequest, RetryHint::Never)),
        _ if status == StatusCode::CONFLICT => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ => None,
    }
}

fn classify_anthropic(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match code {
        Some("request_too_large") => {
            Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision))
        }
        Some("authentication_error") => Some((ModelErrorCode::Authentication, RetryHint::Never)),
        Some("billing_error") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("permission_error") => Some((ModelErrorCode::PermissionDenied, RetryHint::Never)),
        Some("not_found_error") => Some((ModelErrorCode::ModelNotFound, RetryHint::Never)),
        Some("invalid_request_error") => Some((ModelErrorCode::InvalidRequest, RetryHint::Never)),
        Some("rate_limit_error") => Some((ModelErrorCode::RateLimited, retry_hint(retry_after_ms))),
        Some("overloaded_error") => Some((ModelErrorCode::Overloaded, retry_hint(retry_after_ms))),
        Some("api_error") | Some("timeout_error") => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ if status.as_u16() == 529 => {
            Some((ModelErrorCode::Overloaded, retry_hint(retry_after_ms)))
        }
        _ => None,
    }
}

fn classify_deepseek(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    if matches!(code, Some("context_length_exceeded")) {
        return Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision));
    }
    match status {
        StatusCode::PAYMENT_REQUIRED => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        StatusCode::TOO_MANY_REQUESTS => {
            Some((ModelErrorCode::RateLimited, retry_hint(retry_after_ms)))
        }
        StatusCode::SERVICE_UNAVAILABLE => Some((ModelErrorCode::Overloaded, RetryHint::Backoff)),
        StatusCode::INTERNAL_SERVER_ERROR => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ => None,
    }
}

fn classify_kimi(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match code {
        Some("exceeded_model_token_limit") | Some("context_length_exceeded") => {
            Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision))
        }
        Some("exceeded_current_quota_error") => {
            Some((ModelErrorCode::QuotaExhausted, RetryHint::Never))
        }
        Some("rate_limit_reached_error") => {
            Some((ModelErrorCode::RateLimited, retry_hint(retry_after_ms)))
        }
        Some("engine_overloaded_error") => {
            Some((ModelErrorCode::Overloaded, retry_hint(retry_after_ms)))
        }
        Some("permission_denied_error") => {
            Some((ModelErrorCode::PermissionDenied, RetryHint::Never))
        }
        Some("resource_not_found_error") => Some((ModelErrorCode::ModelNotFound, RetryHint::Never)),
        Some("server_error") | Some("unexpected_output") | Some("server_unavailable") => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        Some("client_closed_request") => Some((ModelErrorCode::Cancelled, RetryHint::Never)),
        _ if status == StatusCode::SERVICE_UNAVAILABLE => {
            Some((ModelErrorCode::Overloaded, RetryHint::Backoff))
        }
        _ => None,
    }
}

fn classify_qwen(
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match code {
        Some("InvalidParameter.InputTooLong") | Some("context_length_exceeded") => {
            Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision))
        }
        Some("Arrearage")
        | Some("CommodityNotPurchased")
        | Some("PrepaidBillOverdue")
        | Some("PostpaidBillOverdue") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("Throttling")
        | Some("Throttling.RateQuota")
        | Some("Throttling.BurstRate")
        | Some("Throttling.AllocationQuota")
        | Some("LimitRequests")
        | Some("limit_requests")
        | Some("limit_burst_rate")
        | Some("insufficient_quota") => {
            Some((ModelErrorCode::RateLimited, retry_hint(retry_after_ms)))
        }
        Some("ModelServingError") | Some("ModelUnavailable") => {
            Some((ModelErrorCode::Overloaded, RetryHint::Backoff))
        }
        _ => None,
    }
}

fn classify_glm(
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    match code {
        Some("1000") | Some("1001") | Some("1003") | Some("1005") => {
            Some((ModelErrorCode::Authentication, RetryHint::Never))
        }
        Some("1113") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("1215") => Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision)),
        Some("1210") | Some("1213") | Some("1214") | Some("1261") => {
            Some((ModelErrorCode::InvalidRequest, RetryHint::Never))
        }
        Some("1211") => Some((ModelErrorCode::ModelNotFound, RetryHint::Never)),
        Some("1212") => Some((ModelErrorCode::CapabilityUnsupported, RetryHint::Never)),
        Some("1220") => Some((ModelErrorCode::PermissionDenied, RetryHint::Never)),
        Some("1301") => Some((ModelErrorCode::ContentFiltered, RetryHint::Never)),
        Some("1302") => Some((ModelErrorCode::RateLimited, retry_hint(retry_after_ms))),
        Some("1305") => Some((ModelErrorCode::Overloaded, retry_hint(retry_after_ms))),
        Some("1308") | Some("1309") | Some("1310") | Some("1311") | Some("1314") | Some("1315")
        | Some("1316") | Some("1317") | Some("1318") | Some("1319") | Some("1320")
        | Some("1321") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("1200") | Some("1230") | Some("1234") => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::{ErrorDialect, classify};
    use crate::model::{ModelErrorCode, RetryHint};

    #[test]
    fn maps_only_explicit_context_overflow_codes() {
        for (dialect, code) in [
            (ErrorDialect::OpenAi, "context_length_exceeded"),
            (ErrorDialect::Anthropic, "request_too_large"),
            (ErrorDialect::DeepSeek, "context_length_exceeded"),
            (ErrorDialect::Kimi, "exceeded_model_token_limit"),
            (ErrorDialect::Qwen, "InvalidParameter.InputTooLong"),
            (ErrorDialect::Glm, "1215"),
        ] {
            assert_eq!(
                classify(dialect, StatusCode::BAD_REQUEST, Some(code), None),
                Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision))
            );
        }
        assert_ne!(
            classify(
                ErrorDialect::OpenAi,
                StatusCode::BAD_REQUEST,
                Some("invalid_prompt"),
                None,
            ),
            Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision))
        );
    }
}
