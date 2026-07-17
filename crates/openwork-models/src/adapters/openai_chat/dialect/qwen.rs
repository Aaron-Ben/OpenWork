use crate::model::{ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream};
use crate::model::{ModelErrorCode, RetryHint};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{Map, Value};

use crate::provider::OpenAiChatDialect;
use crate::{HttpProviderConfig, HttpTransport, OpenAiCompatibleChatProvider};

const DASHSCOPE_COMPATIBLE_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";

pub(crate) fn classify_error(
    _status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> Option<(ModelErrorCode, RetryHint)> {
    let retry = || {
        retry_after_ms
            .map(RetryHint::AfterMillis)
            .unwrap_or(RetryHint::Backoff)
    };
    match code {
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
        | Some("insufficient_quota") => Some((ModelErrorCode::RateLimited, retry())),
        Some("ModelServingError") | Some("ModelUnavailable") => {
            Some((ModelErrorCode::Overloaded, RetryHint::Backoff))
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct QwenProvider {
    chat: OpenAiCompatibleChatProvider,
}

impl QwenProvider {
    pub fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self {
            chat: OpenAiCompatibleChatProvider::new(config, transport, OpenAiChatDialect::Qwen),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new(DASHSCOPE_COMPATIBLE_BASE_URL, api_key),
            transport,
        )
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.chat = self.chat.with_extra_body(extra_body);
        self
    }

    #[cfg(test)]
    pub(crate) fn chat_request_body(&self, req: &ModelRequest) -> Result<Value, ModelError> {
        self.chat.chat_completions_request_body(req, false)
    }
}

#[async_trait]
impl ModelPort for QwenProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.chat.invoke(request, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContentBlock, Message, Role, ThinkingConfig};

    #[test]
    fn builds_qwen_chat_body() {
        let provider = QwenProvider::from_api_key("test-key", HttpTransport::default());
        let req = ModelRequest::text("qwen-plus", "hello");

        let body = provider.chat_request_body(&req).unwrap();

        assert_eq!(body["model"], "qwen-plus");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn builds_qwen_multimodal_chat_body() {
        let provider = QwenProvider::from_api_key("test-key", HttpTransport::default());
        let req = ModelRequest {
            model: "qwen-vl-plus".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: vec![
                    ContentBlock::text("describe this image"),
                    ContentBlock::image_url("https://example.com/image.png", "image/png"),
                ],
            }],
            temperature: None,
            max_output_tokens: None,
            thinking: None,
            tools: Vec::new(),
        };

        let body = provider.chat_request_body(&req).unwrap();

        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    }

    #[test]
    fn maps_thinking_and_requests_stream_usage() {
        let provider = QwenProvider::from_api_key("test-key", HttpTransport::default());
        let req = ModelRequest::text("qwen-plus", "think").with_thinking(ThinkingConfig::enabled());

        let body = provider
            .chat
            .chat_completions_request_body(&req, true)
            .unwrap();

        assert_eq!(body["enable_thinking"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }
}
