use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest, ModelStream,
    ModelTransportSignalKind, RetryHint,
};

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

struct RetryStreamState {
    inner: Arc<dyn ModelPort>,
    policy: RetryPolicy,
    request: ModelRequest,
    options: ModelCallOptions,
    deadline: Option<tokio::time::Instant>,
    attempt: usize,
    current: Option<ModelStream>,
    semantic_output_emitted: bool,
    completed: bool,
}

impl RetryStreamState {
    fn deadline(&mut self) -> tokio::time::Instant {
        *self
            .deadline
            .get_or_insert_with(|| tokio::time::Instant::now() + self.options.total_timeout)
    }

    async fn prepare_retry(&mut self, error: ModelError) -> Result<(), ModelError> {
        if self.attempt >= self.options.max_transport_attempts {
            self.options.observe_transport(
                self.attempt,
                ModelTransportSignalKind::Failed {
                    error: error.clone(),
                    retry_delay_ms: None,
                },
            );
            return Err(error);
        }
        let RetryDecision::RetryAfter(delay) =
            self.policy
                .decision(self.attempt, &error, self.semantic_output_emitted)
        else {
            self.options.observe_transport(
                self.attempt,
                ModelTransportSignalKind::Failed {
                    error: error.clone(),
                    retry_delay_ms: None,
                },
            );
            return Err(error);
        };
        if tokio::time::Instant::now() + delay >= self.deadline() {
            self.options.observe_transport(
                self.attempt,
                ModelTransportSignalKind::Failed {
                    error: error.clone(),
                    retry_delay_ms: None,
                },
            );
            return Err(error);
        }

        self.options.observe_transport(
            self.attempt,
            ModelTransportSignalKind::Failed {
                error,
                retry_delay_ms: Some(delay.as_millis().min(u64::MAX as u128) as u64),
            },
        );
        self.current = None;
        tokio::time::sleep(delay).await;
        self.attempt += 1;
        self.semantic_output_emitted = false;
        Ok(())
    }
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
        let state = RetryStreamState {
            inner: Arc::clone(&self.inner),
            policy: self.policy.clone(),
            request,
            options,
            deadline: None,
            attempt: 1,
            current: None,
            semantic_output_emitted: false,
            completed: false,
        };

        Ok(Box::pin(stream::try_unfold(
            state,
            |mut state| async move {
                if state.completed {
                    return Ok(None);
                }

                let deadline = state.deadline();
                if tokio::time::Instant::now() >= deadline {
                    return Err(ModelError::timeout());
                }

                loop {
                    if state.current.is_none() {
                        state
                            .options
                            .observe_transport(state.attempt, ModelTransportSignalKind::Started);
                        let result = tokio::time::timeout_at(
                            deadline,
                            state
                                .inner
                                .invoke(state.request.clone(), state.options.clone()),
                        )
                        .await
                        .map_err(|_| ModelError::timeout());
                        match result {
                            Ok(Ok(stream)) => {
                                state.current = Some(stream);
                                state.semantic_output_emitted = false;
                            }
                            Ok(Err(error)) | Err(error) => {
                                state.prepare_retry(error).await?;
                                continue;
                            }
                        }
                    }

                    let next = {
                        let current = state.current.as_mut().ok_or_else(|| {
                            ModelError::protocol("retry stream attempt is unavailable")
                        })?;
                        tokio::time::timeout_at(deadline, current.next())
                            .await
                            .map_err(|_| ModelError::timeout())?
                    };

                    match next {
                        Some(Ok(event)) => {
                            if matches!(event, ModelEvent::ResponseCompleted { .. }) {
                                let provider_request_id = match &event {
                                    ModelEvent::ResponseCompleted { response } => {
                                        response.provider_request_id.clone()
                                    }
                                    _ => None,
                                };
                                state.options.observe_transport(
                                    state.attempt,
                                    ModelTransportSignalKind::Succeeded {
                                        provider_request_id,
                                    },
                                );
                                state.completed = true;
                                state.current = None;
                            } else {
                                state.semantic_output_emitted = true;
                            }
                            return Ok(Some((event, state)));
                        }
                        Some(Err(error)) => {
                            state.current = None;
                            state.prepare_retry(error).await?;
                        }
                        None => {
                            state.current = None;
                            state
                                .prepare_retry(ModelError::protocol(
                                    "provider stream ended without ResponseCompleted",
                                ))
                                .await?;
                        }
                    }
                }
            },
        )))
    }
}
