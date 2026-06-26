use async_trait::async_trait;

use super::{
    EmbeddingRequest, EmbeddingResponse, GenerateRequest, GenerateResponse, GenerateStreamEvent,
    ProviderError,
};

pub type GenerateStreamCallback = Box<dyn FnMut(GenerateStreamEvent) + Send>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError>;

    async fn stream_generate(
        &self,
        _req: GenerateRequest,
        _on_event: GenerateStreamCallback,
    ) -> Result<GenerateResponse, ProviderError> {
        Err(ProviderError::InvalidRequest {
            message: "streaming is not implemented for this provider".to_string(),
        })
    }
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, req: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError>;
}
