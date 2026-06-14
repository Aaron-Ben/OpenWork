use thiserror::Error;

use super::ModelCapability;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider authentication failed")]
    Authentication,
    #[error("provider permission denied")]
    PermissionDenied,
    #[error("provider rate limited request")]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("provider request timed out")]
    Timeout,
    #[error("invalid provider request: {message}")]
    InvalidRequest { message: String },
    #[error("model unavailable: {model}")]
    ModelUnavailable { model: String },
    #[error("model {model} does not support capability {capability:?}")]
    CapabilityUnsupported {
        model: String,
        capability: ModelCapability,
    },
    #[error("provider server error with status {status}")]
    ProviderServerError { status: u16 },
    #[error("provider network error: {message}")]
    Network { message: String },
    #[error("provider serialization error: {message}")]
    Serialization { message: String },
}
