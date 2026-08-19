use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;
use uuid::Uuid;

pub const SEEN_TTL: Duration = Duration::from_secs(10 * 60);
pub const HELD_TOKEN_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy)]
struct SeenEntry {
    sequence: i64,
    observed_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldDecision {
    Allow,
    Hold,
}

pub fn held_precheck(
    member_count: usize,
    highest_sequence: i64,
    seen_sequence: Option<i64>,
) -> HeldDecision {
    if member_count <= 2 {
        return HeldDecision::Allow;
    }
    match seen_sequence {
        Some(seen) if highest_sequence > seen => HeldDecision::Hold,
        Some(_) | None => HeldDecision::Allow,
    }
}

#[derive(Debug, Clone)]
struct HeldEntry {
    agent_id: String,
    room_id: String,
    peer_sequence: i64,
    issued_at: Instant,
}

#[derive(Debug)]
pub struct HeldTokens {
    ttl: Duration,
    entries: HashMap<String, HeldEntry>,
}

impl HeldTokens {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    pub fn issue(
        &mut self,
        agent_id: &str,
        room_id: &str,
        peer_sequence: i64,
        now: Instant,
    ) -> String {
        let token = format!("held_{}", Uuid::new_v4().simple());
        self.entries.insert(
            token.clone(),
            HeldEntry {
                agent_id: agent_id.to_string(),
                room_id: room_id.to_string(),
                peer_sequence,
                issued_at: now,
            },
        );
        token
    }

    pub fn confirm(
        &mut self,
        token: &str,
        agent_id: &str,
        room_id: &str,
        now: Instant,
    ) -> Option<i64> {
        let entry = self.entries.get(token)?;
        if now.saturating_duration_since(entry.issued_at) >= self.ttl {
            self.entries.remove(token);
            return None;
        }
        (entry.agent_id == agent_id && entry.room_id == room_id).then_some(entry.peer_sequence)
    }
}

#[derive(Debug)]
struct CoordinationState {
    seen: SeenCursors,
    held: HeldTokens,
}

#[derive(Debug, Clone)]
pub struct CoordinationHub {
    state: Arc<Mutex<CoordinationState>>,
}

impl CoordinationHub {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CoordinationState {
                seen: SeenCursors::new(SEEN_TTL),
                held: HeldTokens::new(HELD_TOKEN_TTL),
            })),
        }
    }

    pub async fn observe(&self, agent_id: &str, room_id: &str, sequence: i64) {
        self.state
            .lock()
            .await
            .seen
            .observe(agent_id, room_id, sequence, Instant::now());
    }

    pub async fn effective_seen(
        &self,
        agent_id: &str,
        room_id: &str,
        held_token: Option<&str>,
    ) -> Option<i64> {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        let seen = state.seen.highest(agent_id, room_id, now);
        let confirmed =
            held_token.and_then(|token| state.held.confirm(token, agent_id, room_id, now));
        match (seen, confirmed) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(sequence), None) | (None, Some(sequence)) => Some(sequence),
            (None, None) => None,
        }
    }

    pub async fn issue_held(&self, agent_id: &str, room_id: &str, sequence: i64) -> String {
        self.state
            .lock()
            .await
            .held
            .issue(agent_id, room_id, sequence, Instant::now())
    }
}

impl Default for CoordinationHub {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct SeenCursors {
    ttl: Duration,
    entries: HashMap<(String, String), SeenEntry>,
}

impl SeenCursors {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    pub fn observe(&mut self, agent_id: &str, room_id: &str, sequence: i64, now: Instant) {
        let key = (agent_id.to_string(), room_id.to_string());
        self.entries
            .entry(key)
            .and_modify(|entry| {
                entry.sequence = entry.sequence.max(sequence);
                entry.observed_at = now;
            })
            .or_insert(SeenEntry {
                sequence,
                observed_at: now,
            });
    }

    pub fn highest(&mut self, agent_id: &str, room_id: &str, now: Instant) -> Option<i64> {
        let key = (agent_id.to_string(), room_id.to_string());
        let entry = self.entries.get(&key).copied()?;
        if now.saturating_duration_since(entry.observed_at) < self.ttl {
            Some(entry.sequence)
        } else {
            self.entries.remove(&key);
            None
        }
    }
}
