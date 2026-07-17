use crate::model::{ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream};
use crate::model::{ModelErrorCode, RetryHint};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{Map, Value};

use crate::provider::OpenAiChatDialect;
use crate::{HttpProviderConfig, HttpTransport, OpenAiCompatibleChatProvider};

const GLM_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";

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
        Some("1000") | Some("1001") | Some("1003") | Some("1005") => {
            Some((ModelErrorCode::Authentication, RetryHint::Never))
        }
        Some("1113") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("1210") | Some("1213") | Some("1214") | Some("1215") | Some("1261") => {
            Some((ModelErrorCode::InvalidRequest, RetryHint::Never))
        }
        Some("1211") => Some((ModelErrorCode::ModelNotFound, RetryHint::Never)),
        Some("1212") => Some((ModelErrorCode::CapabilityUnsupported, RetryHint::Never)),
        Some("1220") => Some((ModelErrorCode::PermissionDenied, RetryHint::Never)),
        Some("1301") => Some((ModelErrorCode::ContentFiltered, RetryHint::Never)),
        Some("1302") => Some((ModelErrorCode::RateLimited, retry())),
        Some("1305") => Some((ModelErrorCode::Overloaded, retry())),
        Some("1308") | Some("1309") | Some("1310") | Some("1311") | Some("1314") | Some("1315")
        | Some("1316") | Some("1317") | Some("1318") | Some("1319") | Some("1320")
        | Some("1321") => Some((ModelErrorCode::QuotaExhausted, RetryHint::Never)),
        Some("1200") | Some("1230") | Some("1234") => {
            Some((ModelErrorCode::ServerError, RetryHint::Backoff))
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct GlmProvider {
    inner: OpenAiCompatibleChatProvider,
}

impl GlmProvider {
    pub fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self {
            inner: OpenAiCompatibleChatProvider::new(config, transport, OpenAiChatDialect::Glm),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(HttpProviderConfig::new(GLM_BASE_URL, api_key), transport)
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.inner = self.inner.with_extra_body(extra_body);
        self
    }
}

#[async_trait]
impl ModelPort for GlmProvider {
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
    use crate::model::{ModelRequest, ThinkingConfig, ToolDefinition};
    use serde_json::json;

    #[test]
    fn builds_default_glm_provider() {
        let provider = GlmProvider::from_api_key("test-key", HttpTransport::default());
        let req = ModelRequest::text("glm-4.6", "hello");

        let body = provider
            .inner
            .chat_completions_request_body(&req, false)
            .unwrap();

        assert_eq!(body["model"], "glm-4.6");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn accepts_glm_specific_extra_body() {
        let mut extra = Map::new();
        extra.insert("request_id".to_string(), json!("req-test"));
        let provider =
            GlmProvider::from_api_key("test-key", HttpTransport::default()).with_extra_body(extra);
        let req = ModelRequest::text("glm-4.6", "hello");

        let body = provider
            .inner
            .chat_completions_request_body(&req, false)
            .unwrap();

        assert_eq!(body["request_id"], "req-test");
    }

    #[test]
    fn maps_thinking_and_enables_streamed_tools() {
        let provider = GlmProvider::from_api_key("test-key", HttpTransport::default());
        let mut req =
            ModelRequest::text("glm-5.1", "use a tool").with_thinking(ThinkingConfig::enabled());
        req.tools.push(ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        });

        let body = provider
            .inner
            .chat_completions_request_body(&req, true)
            .unwrap();

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["tool_stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }
}
