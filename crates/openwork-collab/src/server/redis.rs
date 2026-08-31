use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use redis::{Client, ErrorKind, RedisError, RedisResult};
use serde::{Deserialize, Serialize};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::protocol::WakeEvent;

const MESSAGE_CHANNEL: &str = "openwork:message.new";
const WAKE_PATTERN: &str = "openwork:wake:*";
const REDIS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageNewEvent {
    pub message_id: String,
    pub room_id: String,
    pub author_id: String,
    pub published_at: i64,
}

#[derive(Clone)]
pub struct RedisCoordination {
    client: Client,
    message_events: broadcast::Sender<MessageNewEvent>,
    wake_events: Arc<AgentWakeHub>,
    connected: Arc<AtomicBool>,
}

#[derive(Default)]
struct AgentWakeHub {
    senders: Mutex<HashMap<String, broadcast::Sender<WakeEvent>>>,
}

impl AgentWakeHub {
    fn subscribe(&self, agent_id: &str) -> broadcast::Receiver<WakeEvent> {
        self.senders
            .lock()
            .expect("Agent wake hub lock poisoned")
            .entry(agent_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    fn publish(&self, event: WakeEvent) {
        let sender = self
            .senders
            .lock()
            .expect("Agent wake hub lock poisoned")
            .get(&event.agent_id)
            .cloned();
        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
    }
}

impl RedisCoordination {
    pub async fn start(
        redis_url: &str,
        shutdown: CancellationToken,
    ) -> Result<(Self, JoinHandle<()>), RedisError> {
        let client = Client::open(redis_url)?;
        let (message_events, _) = broadcast::channel(256);
        let connected = Arc::new(AtomicBool::new(false));
        let coordination = Self {
            client,
            message_events,
            wake_events: Arc::new(AgentWakeHub::default()),
            connected,
        };
        let subscriber = coordination.clone();
        let task = tokio::spawn(async move { subscriber.subscribe_loop(shutdown).await });
        coordination.wait_until_connected().await;
        Ok((coordination, task))
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn subscribe_messages(&self) -> broadcast::Receiver<MessageNewEvent> {
        self.message_events.subscribe()
    }

    pub fn subscribe_wakes(&self, agent_id: &str) -> broadcast::Receiver<WakeEvent> {
        self.wake_events.subscribe(agent_id)
    }

    pub async fn publish_message(&self, event: &MessageNewEvent) -> RedisResult<i64> {
        self.publish(MESSAGE_CHANNEL, event).await
    }

    pub async fn publish_wake(&self, event: &WakeEvent) -> RedisResult<i64> {
        self.publish(&format!("openwork:wake:{}", event.agent_id), event)
            .await
    }

    pub async fn claim_message(&self, message_id: &str) -> RedisResult<bool> {
        let mut connection = self.connection().await?;
        let result = tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("SET")
                .arg(format!("openwork:wake-claim:{message_id}"))
                .arg("1")
                .arg("NX")
                .arg("EX")
                .arg(60)
                .query_async::<Option<String>>(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        Ok(result.is_some())
    }

    pub async fn allow_agent_authored_wake(&self, agent_id: &str) -> RedisResult<bool> {
        let mut connection = self.connection().await?;
        let key = format!("openwork:turn-rate:{agent_id}");
        let count = tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("INCR")
                .arg(&key)
                .query_async::<i64>(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        if count == 1 {
            let _: bool = tokio::time::timeout(
                REDIS_TIMEOUT,
                redis::cmd("EXPIRE")
                    .arg(&key)
                    .arg(60)
                    .query_async(&mut connection),
            )
            .await
            .map_err(|_| timeout_error())??;
        }
        Ok(count <= 30)
    }

    async fn publish<T: Serialize>(&self, channel: &str, event: &T) -> RedisResult<i64> {
        let payload = serde_json::to_string(event).map_err(|error| {
            RedisError::from((
                ErrorKind::UnexpectedReturnType,
                "invalid Redis event",
                error.to_string(),
            ))
        })?;
        let mut connection = self.connection().await?;
        tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("PUBLISH")
                .arg(channel)
                .arg(payload)
                .query_async::<i64>(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())?
    }

    pub(crate) async fn connection(&self) -> RedisResult<redis::aio::MultiplexedConnection> {
        tokio::time::timeout(
            REDIS_TIMEOUT,
            self.client.get_multiplexed_async_connection(),
        )
        .await
        .map_err(|_| timeout_error())?
    }

    async fn wait_until_connected(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while !self.is_connected() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
    }

    async fn subscribe_loop(self, shutdown: CancellationToken) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            match tokio::time::timeout(REDIS_TIMEOUT, self.client.get_async_pubsub()).await {
                Ok(Ok(mut pubsub)) => {
                    let subscribed = pubsub.subscribe(MESSAGE_CHANNEL).await.is_ok()
                        && pubsub.psubscribe(WAKE_PATTERN).await.is_ok();
                    if subscribed {
                        self.connected.store(true, Ordering::Relaxed);
                        backoff = Duration::from_secs(1);
                        let mut messages = pubsub.on_message();
                        loop {
                            tokio::select! {
                                _ = shutdown.cancelled() => return,
                                message = messages.next() => {
                                    let Some(message) = message else { break };
                                    let channel = message.get_channel_name();
                                    let Ok(payload) = message.get_payload::<String>() else {
                                        continue;
                                    };
                                    if channel == MESSAGE_CHANNEL {
                                        if let Ok(event) = serde_json::from_str(&payload) {
                                            let _ = self.message_events.send(event);
                                        }
                                    } else if channel.starts_with("openwork:wake:")
                                        && let Ok(event) = serde_json::from_str(&payload)
                                    {
                                        self.wake_events.publish(event);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Err(error)) => tracing::warn!(%error, "Redis wake subscriber disconnected"),
                Err(_) => tracing::warn!("Redis wake subscriber connection timed out"),
            }
            self.connected.store(false, Ordering::Relaxed);
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }
}

fn timeout_error() -> RedisError {
    RedisError::from((ErrorKind::Io, "Redis operation timed out"))
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::TryRecvError;

    use super::AgentWakeHub;
    use crate::protocol::WakeEvent;

    #[test]
    fn agent_wake_hub_does_not_fan_out_other_agents_events() {
        let hub = AgentWakeHub::default();
        let mut alpha = hub.subscribe("alpha");
        let mut beta = hub.subscribe("beta");
        hub.publish(WakeEvent {
            id: "event-1".to_string(),
            agent_id: "alpha".to_string(),
            message_id: "message-1".to_string(),
            room_id: "room-1".to_string(),
            reason: "message.new".to_string(),
            published_at: 1,
        });

        assert_eq!(alpha.try_recv().unwrap().agent_id, "alpha");
        assert!(matches!(beta.try_recv(), Err(TryRecvError::Empty)));
    }
}
