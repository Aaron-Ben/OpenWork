pub(crate) mod error;
mod request;
mod response;
mod stream;

use async_trait::async_trait;
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::{
    config::{HttpProviderConfig, HttpTransport},
    error::{ErrorDialect, map_error_response_for, map_reqwest_error, request_id_from_headers},
};

#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    transport: HttpTransport,
    config: HttpProviderConfig,
}

impl OpenAiProvider {
    pub fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self { transport, config }
    }

    pub fn from_api_key(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new("https://api.openai.com", api_key),
            transport,
        )
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
            .map_err(|error| ModelError::invalid_request(error.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    async fn start_stream(&self, req: ModelRequest) -> Result<ModelStream, ModelError> {
        let body = request::encode_request(&req, true)?;
        let response = self
            .transport
            .post_json(
                self.config.endpoint("/v1/responses"),
                self.headers()?,
                &body,
            )
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response_for(response, ErrorDialect::OpenAi).await);
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
impl ModelPort for OpenAiProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.start_stream(request).await
    }
}
