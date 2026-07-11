use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures_util::StreamExt;
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest, ModelStream, RetryHint,
};

use crate::stream::model_stream_from_callback;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    DoNotRetry,
    RetryAfter(Duration),
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
    max_retry_after: Duration,
    jitter: bool,
}

impl RetryPolicy {
    /// `max_attempts` 包含第一次调用；传入 3 表示最多调用三次。
    pub fn new(max_attempts: usize, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            base_delay,
            max_delay,
            max_retry_after: Duration::from_secs(30),
            jitter: true,
        }
    }

    pub fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    pub fn with_max_retry_after(mut self, max_retry_after: Duration) -> Self {
        self.max_retry_after = max_retry_after;
        self
    }

    pub fn decision(
        &self,
        failed_attempt: usize,
        error: &ModelError,
        semantic_output_emitted: bool,
    ) -> RetryDecision {
        if semantic_output_emitted || failed_attempt >= self.max_attempts {
            return RetryDecision::DoNotRetry;
        }

        let delay = match error.retry_hint() {
            RetryHint::Never | RetryHint::CallerDecision => {
                return RetryDecision::DoNotRetry;
            }
            RetryHint::AfterMillis(delay) => {
                let delay = Duration::from_millis(delay);
                if delay > self.max_retry_after {
                    return RetryDecision::DoNotRetry;
                }
                delay
            }
            RetryHint::Backoff => self.backoff_delay(failed_attempt),
        };

        RetryDecision::RetryAfter(delay)
    }

    fn backoff_delay(&self, failed_attempt: usize) -> Duration {
        let exponent = failed_attempt.saturating_sub(1).min(31) as u32;
        let capped = self
            .base_delay
            .saturating_mul(2_u32.saturating_pow(exponent))
            .min(self.max_delay);

        if !self.jitter || capped.is_zero() {
            return capped;
        }

        // Retry 并非加密用途，使用进程时间源即可实现 full jitter，且不引入随机数依赖。
        let ceiling_ms = capped.as_millis().min(u64::MAX as u128) as u64;
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64;
        Duration::from_millis(seed % ceiling_ms.saturating_add(1))
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(3, Duration::from_millis(500), Duration::from_secs(8))
    }
}

pub struct RetryingModelPort {
    inner: Arc<dyn ModelPort>,
    policy: RetryPolicy,
}

impl RetryingModelPort {
    pub fn new(inner: Box<dyn ModelPort>, policy: RetryPolicy) -> Self {
        Self {
            inner: Arc::from(inner),
            policy,
        }
    }
}

#[async_trait]
impl ModelPort for RetryingModelPort {
    async fn invoke(
        &self,
        request: ModelRequest,
        options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        let inner = Arc::clone(&self.inner);
        let policy = self.policy.clone();
        Ok(model_stream_from_callback(move |mut on_event| async move {
            let started = tokio::time::Instant::now();
            let deadline = started + options.total_timeout;
            let mut attempt = 1;
            loop {
                if tokio::time::Instant::now() >= deadline {
                    return Err(ModelError::timeout());
                }
                let mut semantic_output_emitted = false;
                let result = tokio::time::timeout_at(
                    deadline,
                    inner.invoke(request.clone(), options.clone()),
                )
                .await
                .map_err(|_| ModelError::timeout())?;
                let error = match result {
                    Err(error) => error,
                    Ok(mut stream) => loop {
                        match tokio::time::timeout_at(deadline, stream.next())
                            .await
                            .map_err(|_| ModelError::timeout())?
                        {
                            Some(Ok(ModelEvent::ResponseCompleted { response })) => {
                                return Ok(*response);
                            }
                            Some(Ok(event)) => {
                                semantic_output_emitted = true;
                                on_event(event);
                            }
                            Some(Err(error)) => break error,
                            None => {
                                break ModelError::protocol(
                                    "provider stream ended without ResponseCompleted",
                                );
                            }
                        }
                    },
                };

                if attempt >= options.max_transport_attempts {
                    return Err(error);
                }
                let RetryDecision::RetryAfter(delay) =
                    policy.decision(attempt, &error, semantic_output_emitted)
                else {
                    return Err(error);
                };
                if tokio::time::Instant::now() + delay >= deadline {
                    return Err(error);
                }
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }))
    }
}
