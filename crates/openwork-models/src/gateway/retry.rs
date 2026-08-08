use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::model::{
    ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest, ModelStream,
    ModelTransportSignalKind, RetryHint,
};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryDecision {
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

    pub(crate) fn decision(
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
    /// 整段调用的绝对上限，只在流开始时算一次。
    fn total_deadline(&mut self) -> tokio::time::Instant {
        *self
            .deadline
            .get_or_insert_with(|| tokio::time::Instant::now() + self.options.total_timeout)
    }

    /// 下一次等待的截止点。
    ///
    /// 每次调用都从"现在"重新起算空闲上限 —— 这正是这个函数存在的意义：只要流上有
    /// 东西进来就重置计时，Provider 正常吐字时永远不会超时。绝对上限取较早者兜底。
    fn next_wait_deadline(&mut self) -> tokio::time::Instant {
        let total = self.total_deadline();
        let idle = tokio::time::Instant::now() + self.options.idle_timeout;
        idle.min(total)
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
        if tokio::time::Instant::now() + delay >= self.total_deadline() {
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

                if tokio::time::Instant::now() >= state.total_deadline() {
                    return Err(ModelError::timeout());
                }

                loop {
                    if state.current.is_none() {
                        state
                            .options
                            .observe_transport(state.attempt, ModelTransportSignalKind::Started);
                        let connect_deadline = state.next_wait_deadline();
                        let result = tokio::time::timeout_at(
                            connect_deadline,
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

                    // 每读一个块都重新起算：收到东西 = 连接活着，凭什么因为前面花了
                    // 很久就掐掉它。
                    let chunk_deadline = state.next_wait_deadline();
                    let next = {
                        let current = state.current.as_mut().ok_or_else(|| {
                            ModelError::protocol("retry stream attempt is unavailable")
                        })?;
                        tokio::time::timeout_at(chunk_deadline, current.next())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FinishReason, ModelResponse};
    use std::sync::Mutex;

    /// 一个按脚本吐事件的假 Provider：每个事件前先等一段时间。
    struct ScriptedPort {
        /// (吐出这个事件之前先等多久, 事件)
        script: Mutex<Vec<(Duration, ModelEvent)>>,
    }

    impl ScriptedPort {
        fn new(script: Vec<(Duration, ModelEvent)>) -> Self {
            Self {
                script: Mutex::new(script),
            }
        }
    }

    #[async_trait]
    impl ModelPort for ScriptedPort {
        async fn invoke(
            &self,
            _request: ModelRequest,
            _options: ModelCallOptions,
        ) -> Result<ModelStream, ModelError> {
            let script = std::mem::take(&mut *self.script.lock().unwrap());
            Ok(Box::pin(stream::unfold(
                script.into_iter(),
                |mut remaining| async move {
                    let (wait, event) = remaining.next()?;
                    tokio::time::sleep(wait).await;
                    Some((Ok(event), remaining))
                },
            )))
        }
    }

    fn completed() -> ModelEvent {
        ModelEvent::ResponseCompleted {
            response: Box::new(ModelResponse {
                response_id: Some("response".to_string()),
                provider_request_id: Some("request".to_string()),
                model: Some("test-model".to_string()),
                text: "done".to_string(),
                reasoning_text: None,
                tool_calls: Vec::new(),
                provider_opaque_blocks: Vec::new(),
                finish_reason: FinishReason::Stop,
                raw_finish_reason: None,
                usage: None,
            }),
        }
    }

    fn delta(index: u32) -> ModelEvent {
        ModelEvent::ReasoningDelta {
            index,
            delta: "thinking".to_string(),
        }
    }

    async fn drain(
        script: Vec<(Duration, ModelEvent)>,
        options: ModelCallOptions,
    ) -> Result<usize, ModelError> {
        let port = RetryingModelPort::new(
            Box::new(ScriptedPort::new(script)),
            // 关掉重试，让测试只观察超时本身。
            RetryPolicy::new(1, Duration::from_millis(0), Duration::from_millis(0)),
        );
        let mut stream = port
            .invoke(ModelRequest::text("test-model", "hi"), options)
            .await?;
        let mut count = 0;
        while let Some(event) = stream.next().await {
            event?;
            count += 1;
        }
        Ok(count)
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_but_steady_stream_is_not_killed_by_the_idle_timeout() {
        // 每 90 秒吐一个块，总共花 6 分钟 —— 远超旧的 120 秒总预算，但从没静默超过
        // 空闲上限。改之前这个调用会在两分钟整被砍掉。
        let script = (0..3)
            .map(|index| (Duration::from_secs(90), delta(index)))
            .chain(std::iter::once((Duration::from_secs(90), completed())))
            .collect();

        let events = drain(
            script,
            ModelCallOptions::new("attempt-1")
                .with_idle_timeout(Duration::from_secs(120))
                .with_total_timeout(Duration::from_secs(30 * 60)),
        )
        .await
        .expect("a stream that keeps producing must not time out");

        assert_eq!(events, 4);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_stream_still_times_out() {
        // 先正常吐两个块，然后卡死。空闲上限必须照样抓到它。
        let script = vec![
            (Duration::from_secs(1), delta(0)),
            (Duration::from_secs(1), delta(1)),
            (Duration::from_secs(600), completed()),
        ];

        let error = drain(
            script,
            ModelCallOptions::new("attempt-1")
                .with_idle_timeout(Duration::from_secs(120))
                .with_total_timeout(Duration::from_secs(30 * 60)),
        )
        .await
        .expect_err("a stream that goes silent must time out");

        assert_eq!(error.code(), crate::model::ModelErrorCode::Timeout);
    }

    #[tokio::test(start_paused = true)]
    async fn the_absolute_cap_still_bounds_a_pathologically_slow_stream() {
        // 一直滴水但永远不结束：每次都在空闲上限之内，只有绝对上限能停住它。
        let script = (0..1000)
            .map(|index| (Duration::from_secs(60), delta(index)))
            .collect();

        let error = drain(
            script,
            ModelCallOptions::new("attempt-1")
                .with_idle_timeout(Duration::from_secs(120))
                .with_total_timeout(Duration::from_secs(300)),
        )
        .await
        .expect_err("the absolute cap must still bound the call");

        assert_eq!(error.code(), crate::model::ModelErrorCode::Timeout);
    }

    #[test]
    fn the_default_idle_budget_is_far_below_the_absolute_cap() {
        let options = ModelCallOptions::new("attempt-1");

        assert!(
            options.idle_timeout < options.total_timeout,
            "空闲上限若不小于绝对上限，绝对上限就永远轮不到生效"
        );
        assert!(
            options.total_timeout >= Duration::from_secs(10 * 60),
            "绝对上限定得太小会重新变成误杀长调用的那把刀"
        );
    }
}
