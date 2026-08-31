use std::{sync::Arc, time::Duration};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::engine::EngineError;

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use super::RunnerResources;

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
