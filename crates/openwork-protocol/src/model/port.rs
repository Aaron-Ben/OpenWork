use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;
use std::time::Duration;

use super::{ModelError, ModelEvent, ModelRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCallOptions {
    pub model_attempt_id: String,
    pub total_timeout: Duration,
    pub max_transport_attempts: usize,
}

impl ModelCallOptions {
    pub fn new(model_attempt_id: impl Into<String>) -> Self {
        Self {
            model_attempt_id: model_attempt_id.into(),
            total_timeout: Duration::from_secs(120),
            max_transport_attempts: 3,
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
