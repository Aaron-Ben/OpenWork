use std::{sync::Arc, time::Duration};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::engine::EngineError;

/// 只有用户能解决的 Engine 失败（未登录、凭证无效）之后的暂停时长；
/// 数字来自 Cumora `daemon.ts` 的 `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`。
const OPERATOR_FIX_BACKOFF: Duration = Duration::from_secs(15 * 60);
/// Engine 没有给出 retry-after 时的限流冷却。
const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct RunnerResources {
    main_slots: Arc<Semaphore>,
    triage_slots: Arc<Semaphore>,
    pacer: AdaptivePacer,
}

impl RunnerResources {
    pub fn local() -> Self {
        Self {
            main_slots: Arc::new(Semaphore::new(2)),
            triage_slots: Arc::new(Semaphore::new(4)),
            pacer: AdaptivePacer::new(Duration::from_millis(250)),
        }
    }

    pub async fn main_permit(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<OwnedSemaphorePermit, EngineError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(EngineError::Cancelled),
            permit = self.main_slots.clone().acquire_owned() => {
                Ok(permit.expect("main Engine semaphore is never closed"))
            }
        }
    }

    pub async fn triage_permit(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<OwnedSemaphorePermit, EngineError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(EngineError::Cancelled),
            permit = self.triage_slots.clone().acquire_owned() => {
                Ok(permit.expect("triage Engine semaphore is never closed"))
            }
        }
    }

    pub async fn gate(&self, cancellation: &CancellationToken) -> Result<(), EngineError> {
        self.pacer.gate(cancellation).await
    }

    pub async fn observe_result<T>(&self, result: &Result<T, EngineError>) {
        match result {
            Ok(_) => self.pacer.on_success().await,
            Err(EngineError::RateLimited { retry_after, .. }) => {
                self.pacer.on_rate_limited(*retry_after).await;
            }
            Err(_) => {}
        }
    }
}

#[derive(Clone)]
struct AdaptivePacer {
    state: Arc<Mutex<PacerState>>,
    base_gap: Duration,
}

struct PacerState {
    next_start: tokio::time::Instant,
    gap: Duration,
}

impl AdaptivePacer {
    fn new(base_gap: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(PacerState {
                next_start: tokio::time::Instant::now(),
                gap: base_gap,
            })),
            base_gap,
        }
    }

    async fn gate(&self, cancellation: &CancellationToken) -> Result<(), EngineError> {
        loop {
            let wait = {
                let mut state = self.state.lock().await;
                let now = tokio::time::Instant::now();
                if now >= state.next_start {
                    state.next_start = now + state.gap;
                    return Ok(());
                }
                state.next_start - now
            };
            tokio::select! {
                _ = cancellation.cancelled() => return Err(EngineError::Cancelled),
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }

    async fn on_success(&self) {
        let mut state = self.state.lock().await;
        state.gap = state.gap.saturating_sub(Duration::from_millis(100));
        state.gap = state.gap.max(self.base_gap);
    }

    async fn on_rate_limited(&self, retry_after: Option<Duration>) {
        let mut state = self.state.lock().await;
        state.gap = (state.gap * 2).min(Duration::from_secs(10));
        state.next_start = state
            .next_start
            .max(tokio::time::Instant::now() + retry_after.unwrap_or(Duration::from_secs(60)));
    }
}

/// 一次失败的正式 Turn 之后，该 Agent 暂停多久才能再拉起 Engine；`None` 表示照常重试。
///
/// 失败的 Run 不推进 delivery，下一次轮询会再次触发同一批消息。不暂停的话，
/// 未登录的 OpenCode 会在每次轮询时被重新拉起并留下一条失败 Run。
pub(super) fn engine_backoff_after(error: &EngineError) -> Option<Duration> {
    match error {
        EngineError::RateLimited { retry_after, .. } => {
            Some(retry_after.unwrap_or(RATE_LIMIT_BACKOFF))
        }
        EngineError::Unauthenticated { .. } => Some(OPERATOR_FIX_BACKOFF),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use super::{OPERATOR_FIX_BACKOFF, RATE_LIMIT_BACKOFF, RunnerResources, engine_backoff_after};
    use crate::computer::engine::EngineError;

    /// 未登录这类失败重试也不会好，暂停 15 分钟；限流按 retry-after 冷却；其他失败照常重试。
    #[test]
    fn engine_failures_that_need_the_user_pause_the_agent_longer_than_rate_limits() {
        let unauthenticated = EngineError::Unauthenticated {
            detail: "not logged in".to_string(),
        };
        assert_eq!(
            engine_backoff_after(&unauthenticated),
            Some(OPERATOR_FIX_BACKOFF)
        );
        assert_eq!(OPERATOR_FIX_BACKOFF, Duration::from_secs(15 * 60));

        let told = EngineError::RateLimited {
            retry_after: Some(Duration::from_secs(7)),
            detail: "429".to_string(),
        };
        assert_eq!(engine_backoff_after(&told), Some(Duration::from_secs(7)));
        let untold = EngineError::RateLimited {
            retry_after: None,
            detail: "429".to_string(),
        };
        assert_eq!(engine_backoff_after(&untold), Some(RATE_LIMIT_BACKOFF));

        for transient in [
            EngineError::Process {
                detail: "exit status 1".to_string(),
            },
            EngineError::Cancelled,
            EngineError::Timeout { operation: "turn" },
        ] {
            assert_eq!(engine_backoff_after(&transient), None, "{transient}");
        }
    }

    #[tokio::test]
    async fn local_resources_allow_two_agents_but_bound_a_third_main_turn() {
        let resources = RunnerResources::local();
        let cancellation = CancellationToken::new();
        let first = resources.main_permit(&cancellation).await.unwrap();
        let second = resources.main_permit(&cancellation).await.unwrap();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                resources.main_permit(&cancellation),
            )
            .await
            .is_err()
        );

        drop(first);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                resources.main_permit(&cancellation),
            )
            .await
            .is_ok()
        );
        drop(second);
    }

    #[tokio::test]
    async fn local_resources_allow_four_parallel_triage_calls() {
        let resources = RunnerResources::local();
        let cancellation = CancellationToken::new();
        let mut permits = Vec::new();
        for _ in 0..4 {
            permits.push(resources.triage_permit(&cancellation).await.unwrap());
        }
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                resources.triage_permit(&cancellation),
            )
            .await
            .is_err()
        );
        drop(permits);
    }
}
