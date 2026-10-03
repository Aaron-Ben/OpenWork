pub(crate) mod error;
mod error_codes;
pub(crate) mod responses;

use async_trait::async_trait;

use crate::model::{ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream};
use crate::{HttpProviderConfig, HttpTransport};

/// Responses 协议的 HTTP 适配器。线协议只有这一种（照搬 Codex，见
/// `.agents/notes/proposed/simplification/2026-10-03-responses-only-model-access.md`）。
#[derive(Debug, Clone)]
pub(crate) struct ResponsesAdapter {
    config: HttpProviderConfig,
    transport: HttpTransport,
}

impl ResponsesAdapter {
    pub(crate) fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self { config, transport }
    }
}

#[async_trait]
impl ModelPort for ResponsesAdapter {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        responses::start_stream(&self.config, &self.transport, request).await
    }
}
