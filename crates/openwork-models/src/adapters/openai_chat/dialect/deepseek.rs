use crate::model::{ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream};
use crate::model::{ModelErrorCode, RetryHint};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{Map, Value};

use crate::provider::OpenAiChatDialect;
use crate::{HttpProviderConfig, HttpTransport, OpenAiCompatibleChatProvider};

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

pub(crate) fn classify_error(
    status: StatusCode,
    _code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    let retry = retry_after_ms
        .map(RetryHint::AfterMillis)
        .unwrap_or(RetryHint::Backoff);
    match status {
        StatusCode::PAYMENT_REQUIRED => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        StatusCode::TOO_MANY_REQUESTS => Some((ModelErrorCode::RateLimited, retry)),
        StatusCode::SERVICE_UNAVAILABLE => Some((ModelErrorCode::Overloaded, RetryHint::Backoff)),
        StatusCode::INTERNAL_SERVER_ERROR => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct DeepSeekProvider {
    inner: OpenAiCompatibleChatProvider,
}

impl DeepSeekProvider {
    pub fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self {
            inner: OpenAiCompatibleChatProvider::new(
                config,
                transport,
                OpenAiChatDialect::Deepseek,
            ),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new(DEEPSEEK_BASE_URL, api_key),
            transport,
        )
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.inner = self.inner.with_extra_body(extra_body);
        self
    }
}

#[async_trait]
impl ModelPort for DeepSeekProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.inner.invoke(request, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelRequest, ThinkingConfig};
    use serde_json::json;

    #[test]
    fn builds_default_deepseek_provider() {
        let provider = DeepSeekProvider::from_api_key("test-key", HttpTransport::default());
        let req = ModelRequest::text("deepseek-chat", "hello");

        let body = provider
            .inner
            .chat_completions_request_body(&req, false)
            .unwrap();

        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn accepts_deepseek_specific_extra_body() {
        let mut extra = Map::new();
        extra.insert("reasoning_effort".to_string(), json!("high"));
        let provider = DeepSeekProvider::from_api_key("test-key", HttpTransport::default())
            .with_extra_body(extra);
        let req = ModelRequest::text("deepseek-reasoner", "think");

        let body = provider
            .inner
            .chat_completions_request_body(&req, false)
            .unwrap();

        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn maps_typed_thinking_and_requests_stream_usage() {
        let provider = DeepSeekProvider::from_api_key("test-key", HttpTransport::default());
        let req =
            ModelRequest::text("deepseek-v4-pro", "think").with_thinking(ThinkingConfig::enabled());

        let body = provider
            .inner
            .chat_completions_request_body(&req, true)
            .unwrap();

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }
}
