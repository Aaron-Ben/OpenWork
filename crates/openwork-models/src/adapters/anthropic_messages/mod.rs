pub(crate) mod error;
mod request;
mod response;
mod stream;

use crate::model::{ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream};
use async_trait::async_trait;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

use crate::{
    config::{HttpProviderConfig, HttpTransport},
    error::{ErrorDialect, map_error_response_for, map_reqwest_error, request_id_from_headers},
};

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    transport: HttpTransport,
    config: HttpProviderConfig,
    anthropic_version: String,
}

impl AnthropicProvider {
    pub fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self {
            transport,
            config,
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new("https://api.anthropic.com", api_key),
            transport,
        )
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_str(&self.anthropic_version)
                .map_err(|error| ModelError::invalid_request(error.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&self.config.api_key)
                .map_err(|error| ModelError::invalid_request(error.to_string()))?,
        );
        Ok(headers)
    }

    async fn start_stream(&self, req: ModelRequest) -> Result<ModelStream, ModelError> {
        let body = request::encode_request(&req, true)?;
        let response = self
            .transport
            .post_json(self.config.endpoint("/v1/messages"), self.headers()?, &body)
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response_for(response, ErrorDialect::Anthropic).await);
        }
        let provider_request_id = request_id_from_headers(response.headers());
        Ok(stream::response_stream(
            response,
            provider_request_id,
            req.model,
        ))
    }
}

#[async_trait]
impl ModelPort for AnthropicProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.start_stream(request).await
    }
}
