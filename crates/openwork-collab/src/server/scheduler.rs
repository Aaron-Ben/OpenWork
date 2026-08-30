use time::OffsetDateTime;
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::protocol::WakeEvent;

use super::{
    redis::{MessageNewEvent, RedisCoordination},
    storage::CollaborationStore,
};

#[derive(Clone)]
pub struct Scheduler {
    store: CollaborationStore,
    redis: RedisCoordination,
}

impl Scheduler {
    pub fn new(store: CollaborationStore, redis: RedisCoordination) -> Self {
        Self { store, redis }
    }

    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let scheduler = self.clone();
        let mut messages = self.redis.subscribe_messages();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => return,
                    event = messages.recv() => match event {
                        Ok(event) => scheduler.schedule(event).await,
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "scheduler lagged behind Redis message events");
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
        })
    }

    pub async fn message_committed(&self, message_id: &str, room_id: &str, author_id: &str) {
        let event = MessageNewEvent {
            message_id: message_id.to_string(),
            room_id: room_id.to_string(),
            author_id: author_id.to_string(),
            published_at: OffsetDateTime::now_utc().unix_timestamp(),
        };
        if let Err(error) = self.redis.publish_message(&event).await {
            tracing::warn!(%error, message_id, "message persisted but Redis wake publish failed");
        }
    }

    pub fn subscribe_wakes(&self) -> broadcast::Receiver<WakeEvent> {
        self.redis.subscribe_wakes()
    }

    pub fn redis_connected(&self) -> bool {
        self.redis.is_connected()
    }

    async fn schedule(&self, event: MessageNewEvent) {
        match self.redis.claim_message(&event.message_id).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(%error, message_id = event.message_id, "Redis wake claim failed");
                return;
            }
        }
        let recipients = match self
            .store
            .wake_recipients(&event.room_id, &event.author_id)
            .await
        {
            Ok(recipients) => recipients,
            Err(error) => {
                tracing::warn!(%error, message_id = event.message_id, "wake recipient query failed");
                return;
            }
        };
        for agent_id in recipients {
            if event.author_id != "user" {
                match self.redis.allow_agent_authored_wake(&agent_id).await {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        tracing::warn!(%error, %agent_id, "turn rate check failed open");
                    }
                }
            }
            let wake = WakeEvent {
                id: Uuid::new_v4().to_string(),
                agent_id,
                message_id: event.message_id.clone(),
                room_id: event.room_id.clone(),
                reason: "message.new".to_string(),
                published_at: OffsetDateTime::now_utc().unix_timestamp(),
            };
            if let Err(error) = self.redis.publish_wake(&wake).await {
                tracing::warn!(%error, message_id = event.message_id, "agent wake publish failed");
            }
        }
    }
}
