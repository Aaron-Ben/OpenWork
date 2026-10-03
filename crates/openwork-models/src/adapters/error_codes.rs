//! 已知的 Provider 错误码，与厂商无关。
//!
//! 各家走同一个 Responses 协议，但错误码仍各自定义（例如 Kimi 的上下文超限是
//! `exceeded_model_token_limit`，智谱是数字码 `1215`）。不同厂商的错误码几乎不重名，
//! 所以合成一张表，不按厂商分支。
//!
//! 来源：OpenAI 与 Codex（`codex-rs/codex-api/src/sse/responses.rs` 的 `response.failed` 分类）、
//! 各厂商的错误码文档。

use crate::model::{ModelErrorCode, RetryHint};

/// 按错误码分类。返回 `None` 时，调用方按 HTTP 状态码与消息分类。
pub(crate) fn classify(
    code: &str,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    let retry = || {
        retry_after_ms
            .map(RetryHint::AfterMillis)
            .unwrap_or(RetryHint::Backoff)
    };
    let classified = match code {
        // 上下文超限：交给调用方决定是否压缩后重试。
        "context_length_exceeded" | "exceeded_model_token_limit" | "1215" => {
            (ModelErrorCode::ContextOverflow, RetryHint::CallerDecision)
        }
        // 额度与账单。
        "insufficient_quota"
        | "billing_hard_limit_reached"
        | "usage_not_included"
        | "exceeded_current_quota_error"
        | "1113"
        | "1308"
        | "1309"
        | "1310"
        | "1311"
        | "1314"
        | "1315"
        | "1316"
        | "1317"
        | "1318"
        | "1319"
        | "1320"
        | "1321" => (ModelErrorCode::QuotaExhausted, RetryHint::Never),
        // 限流。
        "rate_limit_exceeded" | "rate_limit_error" | "rate_limit_reached_error" | "1302" => {
            (ModelErrorCode::RateLimited, retry())
        }
        // 过载。
        "server_is_overloaded" | "slow_down" | "engine_overloaded_error" | "1305" => {
            (ModelErrorCode::Overloaded, retry())
        }
        // 认证与权限。
        "invalid_api_key" | "1000" | "1001" | "1003" | "1005" => {
            (ModelErrorCode::Authentication, RetryHint::Never)
        }
        "permission_denied_error" | "1220" => (ModelErrorCode::PermissionDenied, RetryHint::Never),
        // 请求本身的问题。
        "invalid_prompt" | "1210" | "1213" | "1214" | "1261" => {
            (ModelErrorCode::InvalidRequest, RetryHint::Never)
        }
        "model_not_found" | "resource_not_found_error" | "1211" => {
            (ModelErrorCode::ModelNotFound, RetryHint::Never)
        }
        "1212" => (ModelErrorCode::CapabilityUnsupported, RetryHint::Never),
        "content_filter" | "1301" => (ModelErrorCode::ContentFiltered, RetryHint::Never),
        // 服务端错误。
        "server_error" | "unexpected_output" | "server_unavailable" | "1200" | "1230" | "1234" => {
            (ModelErrorCode::ServerError, RetryHint::Backoff)
        }
        "client_closed_request" => (ModelErrorCode::Cancelled, RetryHint::Never),
        _ => return None,
    };
    Some(classified)
}

#[cfg(test)]
mod tests {
    use super::classify;
    use crate::model::{ModelErrorCode, RetryHint};

    #[test]
    fn maps_every_known_context_overflow_code() {
        for code in [
            "context_length_exceeded",
            "exceeded_model_token_limit",
            "1215",
        ] {
            assert_eq!(
                classify(code, None),
                Some((ModelErrorCode::ContextOverflow, RetryHint::CallerDecision)),
                "{code}"
            );
        }
        assert_eq!(
            classify("invalid_prompt", None),
            Some((ModelErrorCode::InvalidRequest, RetryHint::Never))
        );
    }

    #[test]
    fn rate_limits_honor_the_server_delay() {
        assert_eq!(
            classify("rate_limit_exceeded", Some(1_500)),
            Some((ModelErrorCode::RateLimited, RetryHint::AfterMillis(1_500)))
        );
        assert_eq!(
            classify("1302", None),
            Some((ModelErrorCode::RateLimited, RetryHint::Backoff))
        );
    }

    #[test]
    fn unknown_codes_fall_through() {
        assert_eq!(classify("something_new", None), None);
    }
}
