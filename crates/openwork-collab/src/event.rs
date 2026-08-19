use std::{collections::VecDeque, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};

pub const COLLAB_EVENT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollabEventKind {
    RoomsChanged { room_id: String },
    AgentsChanged,
    PermissionsChanged,
    EngineChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabEventEnvelope {
    pub version: u32,
    pub sequence: u64,
    #[serde(flatten)]
    pub event: CollabEventKind,
}

struct EventState {
    next_sequence: u64,
    replay: VecDeque<CollabEventEnvelope>,
}

#[derive(Clone)]
pub struct CollabEventPublisher {
    capacity: usize,
    state: Arc<Mutex<EventState>>,
    sender: broadcast::Sender<CollabEventEnvelope>,
}

pub struct CollabEventSubscription {
    pub replay: Vec<CollabEventEnvelope>,
    pub receiver: broadcast::Receiver<CollabEventEnvelope>,
}

impl CollabEventPublisher {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Self {
            capacity,
            state: Arc::new(Mutex::new(EventState {
                next_sequence: 1,
                replay: VecDeque::with_capacity(capacity),
            })),
            sender,
        }
    }

    pub async fn publish(&self, event: CollabEventKind) -> CollabEventEnvelope {
        let mut state = self.state.lock().await;
        let envelope = CollabEventEnvelope {
            version: COLLAB_EVENT_VERSION,
            sequence: state.next_sequence,
            event,
        };
        state.next_sequence = state.next_sequence.saturating_add(1);
        if state.replay.len() == self.capacity {
            state.replay.pop_front();
        }
        state.replay.push_back(envelope.clone());
        let _ = self.sender.send(envelope.clone());
        envelope
    }

    pub async fn subscribe(&self, after_sequence: u64) -> CollabEventSubscription {
        let state = self.state.lock().await;
        let receiver = self.sender.subscribe();
        let replay = state
            .replay
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .cloned()
            .collect();
        CollabEventSubscription { replay, receiver }
    }
}

impl Default for CollabEventPublisher {
    fn default() -> Self {
        Self::new(512)
    }
}
