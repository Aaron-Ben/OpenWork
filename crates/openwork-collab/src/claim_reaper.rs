//! Releases card claims whose owning OpenCode session has stopped.

use std::time::Duration;

use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    event::{CollabEventKind, CollabEventPublisher},
    home::HomeManager,
    mcp::MessageNotice,
    opencode::EngineConnection,
    storage::CollabStorage,
};

const CHECK_INTERVAL: Duration = Duration::from_secs(15);
const RELEASE_GRACE: Duration = Duration::from_secs(60);

pub struct ClaimReaperHandle {
    task: JoinHandle<()>,
}

impl ClaimReaperHandle {
    pub async fn shutdown(self) {
        let _ = self.task.await;
    }
}

pub fn start(
    storage: CollabStorage,
    homes: HomeManager,
    connections: watch::Receiver<Option<EngineConnection>>,
    notices: tokio::sync::mpsc::UnboundedSender<MessageNotice>,
    events: CollabEventPublisher,
    cancel: CancellationToken,
) -> ClaimReaperHandle {
    let task = tokio::spawn(run(storage, homes, connections, notices, events, cancel));
    ClaimReaperHandle { task }
}

async fn run(
    storage: CollabStorage,
    homes: HomeManager,
    connections: watch::Receiver<Option<EngineConnection>>,
    notices: tokio::sync::mpsc::UnboundedSender<MessageNotice>,
    events: CollabEventPublisher,
    cancel: CancellationToken,
) {
    let mut ticker = tokio::time::interval(CHECK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = ticker.tick() => {
                if let Err(error) = sweep(
                    &storage,
                    &homes,
                    &connections,
                    &notices,
                    &events,
                ).await {
                    eprintln!("card claim status check failed: {error}");
                }
            }
        }
    }
}

async fn sweep(
    storage: &CollabStorage,
    homes: &HomeManager,
    connections: &watch::Receiver<Option<EngineConnection>>,
    notices: &tokio::sync::mpsc::UnboundedSender<MessageNotice>,
    events: &CollabEventPublisher,
) -> Result<(), crate::storage::StorageError> {
    let candidates = storage.claim_release_candidates(RELEASE_GRACE).await?;
    if candidates.is_empty() {
        return Ok(());
    }
    let Some(connection) = connections.borrow().clone() else {
        return Ok(());
    };
    for candidate in candidates {
        let running = match candidate.opencode_session_id.as_deref() {
            Some(session_id) => match connection
                .client
                .session_statuses(&homes.agent_home(&candidate.claimed_by))
                .await
            {
                Ok(statuses) => statuses
                    .get(session_id)
                    .is_some_and(|status| status.is_running()),
                Err(error) => {
                    eprintln!(
                        "failed to inspect claim owner {} for card {}: {error}",
                        candidate.claimed_by, candidate.card_id
                    );
                    continue;
                }
            },
            None => false,
        };
        if running {
            continue;
        }
        let Some(mutation) = storage
            .release_card_claim(
                &candidate.card_id,
                &candidate.claimed_by,
                "user",
                "session_not_running",
            )
            .await?
        else {
            continue;
        };
        let _ = notices.send(MessageNotice {
            room_id: mutation.message.room_id.clone(),
            author_id: mutation.message.author_id.clone(),
            body: mutation.message.body.clone(),
            sequence: mutation.message.sequence,
        });
        events
            .publish(CollabEventKind::RoomsChanged {
                room_id: mutation.message.room_id.clone(),
            })
            .await;
        events
            .publish(CollabEventKind::BoardsChanged {
                room_id: mutation.message.room_id,
            })
            .await;
    }
    Ok(())
}
