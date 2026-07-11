use async_trait::async_trait;
#[cfg(test)]
use openwork_protocol::model::ModelResponse;
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream,
};
use openwork_protocol::provider::OpenAiChatDialect;
use reqwest::StatusCode;
use serde_json::{Map, Value};

use crate::{HttpProviderConfig, OpenAiCompatibleChatProvider};

const KIMI_BASE_URL: &str = "https://api.moonshot.cn/v1";

pub(crate) fn classify_error(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(
    openwork_protocol::model::ModelErrorCode,
    openwork_protocol::model::RetryHint,
)> {
    use openwork_protocol::model::{ModelErrorCode, RetryHint};
    let retry = || {
        retry_after_ms
            .map(RetryHint::AfterMillis)
            .unwrap_or(RetryHint::Backoff)
    };
    match code {
        Some("exceeded_current_quota_error") => {
            Some((ModelErrorCode::QuotaExhausted, RetryHint::Never))
        }
        Some("rate_limit_reached_error") => Some((ModelErrorCode::RateLimited, retry())),
        Some("engine_overloaded_error") => Some((ModelErrorCode::Overloaded, retry())),
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

#[derive(Debug, Clone)]
pub struct KimiProvider {
    inner: OpenAiCompatibleChatProvider,
}

impl KimiProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            inner: OpenAiCompatibleChatProvider::new(config).with_dialect(OpenAiChatDialect::Kimi),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(KIMI_BASE_URL, api_key))
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.inner = self.inner.with_extra_body(extra_body);
        self
    }

    #[cfg(test)]
    pub(crate) fn chat_completions_request_body(
        &self,
        req: &ModelRequest,
        stream: bool,
    ) -> Result<Value, ModelError> {
        self.inner.chat_completions_request_body(req, stream)
    }

    #[cfg(test)]
    pub(crate) fn parse_generate_response(raw: Value) -> ModelResponse {
        OpenAiCompatibleChatProvider::parse_generate_response(raw)
    }
}

#[async_trait]
impl ModelPort for KimiProvider {
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
    use openwork_protocol::model::{ModelRequest, ThinkingConfig, ToolDefinition};
    use serde_json::json;

    #[test]
    fn builds_kimi_request_with_thinking_enabled() {
        let provider = KimiProvider::from_api_key("test-key");
        let req =
            ModelRequest::text("kimi-k2.6", "solve it").with_thinking(ThinkingConfig::enabled());

        let body = provider.chat_completions_request_body(&req, false).unwrap();

        assert_eq!(body["model"], "kimi-k2.6");
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn builds_kimi_request_with_thinking_disabled() {
        let provider = KimiProvider::from_api_key("test-key");
        let req = ModelRequest::text("kimi-k2.6", "answer directly")
            .with_thinking(ThinkingConfig::disabled());

        let body = provider.chat_completions_request_body(&req, false).unwrap();

        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn parses_kimi_reasoning_content() {
        let raw = json!({
            "choices": [{
                "message": {
                    "reasoning_content": "first think",
                    "content": "final answer"
                }
            }]
        });

        let response = KimiProvider::parse_generate_response(raw);

        assert_eq!(response.reasoning_text.as_deref(), Some("first think"));
        assert_eq!(response.text, "final answer");
    }

    #[test]
    fn uses_the_documented_v1_chat_endpoint() {
        let provider = KimiProvider::from_api_key("test-key");

        assert_eq!(
            provider.inner.config.endpoint("/chat/completions"),
            "https://api.moonshot.cn/v1/chat/completions"
        );
    }

    #[test]
    fn supports_tools_and_uses_max_completion_tokens() {
        let provider = KimiProvider::from_api_key("test-key");
        let mut req = ModelRequest::text("kimi-k2.7-code", "list files");
        req.max_output_tokens = Some(256);
        req.tools.push(ToolDefinition {
            name: "list_files".to_string(),
            description: "List files".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        });

        let body = provider.chat_completions_request_body(&req, true).unwrap();

        assert_eq!(body["max_completion_tokens"], 256);
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["tools"][0]["function"]["name"], "list_files");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }
}
