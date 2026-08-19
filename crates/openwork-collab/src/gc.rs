//! Independent retention worker for bounded collaboration-observation cleanup.

use std::time::Duration;

use tokio::{
    task::JoinHandle,
    time::{MissedTickBehavior, interval_at},
};
use tokio_util::sync::CancellationToken;

use crate::storage::{CollabGcPolicy, CollabStorage};

pub const DEFAULT_EVENT_RETENTION_DAYS: u32 = 30;
pub const DEFAULT_TRIAGE_RETENTION_DAYS: u32 = 30;
pub const DEFAULT_BATCH_SIZE: u32 = 500;
pub const DEFAULT_STATEMENT_TIMEOUT_MS: u32 = 2_000;
pub const GC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct GcHandle(JoinHandle<()>);

impl GcHandle {
    pub async fn shutdown(self) {
        let _ = self.0.await;
    }
}

pub fn start(
    storage: CollabStorage,
    policy: CollabGcPolicy,
    cancel: CancellationToken,
) -> GcHandle {
    let task = tokio::spawn(async move {
        let mut ticker = interval_at(tokio::time::Instant::now(), GC_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = ticker.tick() => match storage.garbage_collect(policy).await {
                    Ok(outcome) if outcome.events_deleted > 0 || outcome.triages_deleted > 0 => {
                        eprintln!(
                            "collab.gc events_deleted={} triages_deleted={}",
                            outcome.events_deleted, outcome.triages_deleted
                        );
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("collaboration GC failed: {error}"),
                }
            }
        }
    });
    GcHandle(task)
}
