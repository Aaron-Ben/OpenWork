use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::{ModelError, ModelEvent, ModelRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTransportSignalKind {
    Started,
    Failed {
        error: ModelError,
        retry_delay_ms: Option<u64>,
    },
    Succeeded {
        provider_request_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTransportSignal {
    pub model_attempt_id: String,
    pub transport_attempt: usize,
    pub kind: ModelTransportSignalKind,
}

/// Best-effort side channel for Gateway attempts. Provider semantic streams do
/// not carry these signals, so retry bookkeeping cannot be mistaken for model output.
pub trait ModelTransportObserver: Send + Sync {
    fn observe(&self, signal: ModelTransportSignal);
}

#[derive(Clone)]
pub struct ModelCallOptions {
    pub model_attempt_id: String,
    pub total_timeout: Duration,
    pub max_transport_attempts: usize,
    transport_observer: Option<Arc<dyn ModelTransportObserver>>,
}

impl std::fmt::Debug for ModelCallOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelCallOptions")
            .field("model_attempt_id", &self.model_attempt_id)
            .field("total_timeout", &self.total_timeout)
            .field("max_transport_attempts", &self.max_transport_attempts)
            .field("has_transport_observer", &self.transport_observer.is_some())
            .finish()
    }
}

impl ModelCallOptions {
    pub fn new(model_attempt_id: impl Into<String>) -> Self {
        Self {
            model_attempt_id: model_attempt_id.into(),
            total_timeout: Duration::from_secs(120),
            max_transport_attempts: 3,
            transport_observer: None,
        }
    }

    pub fn with_transport_observer(mut self, observer: Arc<dyn ModelTransportObserver>) -> Self {
        self.transport_observer = Some(observer);
        self
    }

    pub fn observe_transport(&self, transport_attempt: usize, kind: ModelTransportSignalKind) {
        if let Some(observer) = &self.transport_observer {
            observer.observe(ModelTransportSignal {
                model_attempt_id: self.model_attempt_id.clone(),
                transport_attempt,
                kind,
            });
        }
    }
}

pub type ModelStream = Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send + 'static>>;

#[async_trait]
pub trait ModelPort: Send + Sync {
    async fn invoke(
        &self,
        request: ModelRequest,
        options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError>;
}
