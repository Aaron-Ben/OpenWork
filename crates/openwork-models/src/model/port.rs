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
    /// 单次等待的上限：等连接建立，或等流上的下一个数据块。
    ///
    /// 流式调用该杀的是"**多久没动静**"，不是"总共花了多久"。只要 Provider 还在吐字，
    /// 计时就重置 —— 推理模型在一个大任务上花掉几分钟是正常的，不该和连接卡死同等对待。
    pub idle_timeout: Duration,
    /// 整段调用的绝对上限，兜底用。
    ///
    /// 空闲超时已经能抓住卡死的连接，这一条只防"一直缓慢滴水但永远不结束"的病态流。
    /// 定得宽，因为误杀一个正在正常工作的长调用，代价比多等一会儿大得多。
    pub total_timeout: Duration,
    pub max_transport_attempts: usize,
    transport_observer: Option<Arc<dyn ModelTransportObserver>>,
}

impl std::fmt::Debug for ModelCallOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelCallOptions")
            .field("model_attempt_id", &self.model_attempt_id)
            .field("idle_timeout", &self.idle_timeout)
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
            idle_timeout: Duration::from_secs(120),
            total_timeout: Duration::from_secs(30 * 60),
            max_transport_attempts: 3,
            transport_observer: None,
        }
    }

    pub fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    pub fn with_total_timeout(mut self, total_timeout: Duration) -> Self {
        self.total_timeout = total_timeout;
        self
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
