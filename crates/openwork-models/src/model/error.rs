use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelErrorCode {
    Authentication,
    PermissionDenied,
    InvalidRequest,
    ContextOverflow,
    ModelNotFound,
    CapabilityUnsupported,
    RateLimited,
    QuotaExhausted,
    Overloaded,
    Timeout,
    Network,
    ServerError,
    ContentFiltered,
    ProtocolError,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailurePhase {
    RequestEncode,
    Connect,
    ResponseHeaders,
    ResponseBody,
    StreamDecode,
    ResponseDecode,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    NotSent,
    PossiblySent,
    AcceptedNoSemanticOutput,
    SemanticOutputEmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryHint {
    Never,
    Backoff,
    AfterMillis(u64),
    CallerDecision,
}

/// Provider 失败的唯一公共表示。厂商响应体只提取到白名单字段，不保留原始 body。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
pub struct ModelError {
    pub kind: ModelErrorCode,
    pub phase: ModelFailurePhase,
    pub delivery: DeliveryState,
    pub retry: RetryHint,
    pub message: String,
    pub http_status: Option<u16>,
    pub provider_code: Option<String>,
    pub provider_request_id: Option<String>,
}

impl ModelError {
    pub fn new(
        kind: ModelErrorCode,
        phase: ModelFailurePhase,
        delivery: DeliveryState,
        retry: RetryHint,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            phase,
            delivery,
            retry,
            message: message.into(),
            http_status: None,
            provider_code: None,
            provider_request_id: None,
        }
    }

    pub fn http(
        kind: ModelErrorCode,
        status: u16,
        message: impl Into<String>,
        provider_code: Option<String>,
        provider_request_id: Option<String>,
        retry: RetryHint,
    ) -> Self {
        Self {
            kind,
            phase: ModelFailurePhase::ResponseHeaders,
            delivery: DeliveryState::AcceptedNoSemanticOutput,
            retry,
            message: message.into(),
            http_status: Some(status),
            provider_code,
            provider_request_id,
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(
            ModelErrorCode::InvalidRequest,
            ModelFailurePhase::RequestEncode,
            DeliveryState::NotSent,
            RetryHint::Never,
            format!("invalid provider request: {}", message.into()),
        )
    }

    pub fn context_overflow(message: impl Into<String>) -> Self {
        Self::new(
            ModelErrorCode::ContextOverflow,
            ModelFailurePhase::ResponseHeaders,
            DeliveryState::AcceptedNoSemanticOutput,
            RetryHint::CallerDecision,
            message,
        )
    }

    pub fn timeout() -> Self {
        Self::new(
            ModelErrorCode::Timeout,
            ModelFailurePhase::Connect,
            DeliveryState::PossiblySent,
            RetryHint::Backoff,
            "provider request timed out",
        )
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(
            ModelErrorCode::Network,
            ModelFailurePhase::Connect,
            DeliveryState::PossiblySent,
            RetryHint::Backoff,
            format!("provider network error: {}", message.into()),
        )
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(
            ModelErrorCode::ProtocolError,
            ModelFailurePhase::StreamDecode,
            DeliveryState::AcceptedNoSemanticOutput,
            RetryHint::Never,
            format!("provider protocol error: {}", message.into()),
        )
    }

    pub fn cancelled() -> Self {
        Self::new(
            ModelErrorCode::Cancelled,
            ModelFailurePhase::Cancelled,
            DeliveryState::PossiblySent,
            RetryHint::Never,
            "model call cancelled",
        )
    }

    pub fn with_delivery(mut self, delivery: DeliveryState) -> Self {
        self.delivery = delivery;
        self
    }

    pub fn code(&self) -> ModelErrorCode {
        self.kind
    }

    pub fn retry_hint(&self) -> RetryHint {
        self.retry
    }

    pub fn provider_request_id(&self) -> Option<&str> {
        self.provider_request_id.as_deref()
    }
}
