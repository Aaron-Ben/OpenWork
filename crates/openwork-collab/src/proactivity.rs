//! In-memory gates for autonomous collaboration work.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
    time::Instant,
};

use thiserror::Error;

pub const NUDGE_COOLDOWN: Duration = Duration::from_secs(45 * 60);
pub const NUDGE_CLAIM_FALLBACK: Duration = Duration::from_secs(5 * 60);
pub const NUDGE_DECLINE_CAP: u8 = 3;
pub const SCANNER_FINGERPRINT_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);
pub const DM_AGENT_TRIAGE_EVERY: i64 = 8;
pub const AUTONOMOUS_RATE_INTERVAL: Duration = Duration::from_secs(60);

pub fn should_probe_agent_dm(sequence: i64) -> bool {
    sequence > 0 && sequence % DM_AGENT_TRIAGE_EVERY == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmProgress {
    Continue,
    Stop,
}

pub fn resolve_dm_progress(actionable: bool) -> DmProgress {
    if actionable {
        DmProgress::Continue
    } else {
        DmProgress::Stop
    }
}

pub fn scanner_fingerprint(rooms: &[(String, i64)]) -> String {
    let mut rooms = rooms.to_vec();
    rooms.sort();
    serde_json::to_string(&rooms).unwrap_or_default()
}

#[derive(Debug, Default)]
pub struct IdleRotation {
    last_agent_id: Option<String>,
}

impl IdleRotation {
    pub fn choose(&mut self, candidates: &[String]) -> Option<String> {
        let mut candidates = candidates.to_vec();
        candidates.sort();
        candidates.dedup();
        let chosen = self
            .last_agent_id
            .as_ref()
            .and_then(|last| candidates.iter().position(|candidate| candidate > last))
            .and_then(|index| candidates.get(index))
            .or_else(|| candidates.first())?
            .clone();
        self.last_agent_id = Some(chosen.clone());
        Some(chosen)
    }
}

#[derive(Debug, Default)]
pub struct AutonomousRateGate {
    last_by_agent: HashMap<String, Instant>,
}

impl AutonomousRateGate {
    pub fn try_acquire(&mut self, agent_id: &str, now: Instant) -> bool {
        if self
            .last_by_agent
            .get(agent_id)
            .is_some_and(|last| now.duration_since(*last) < AUTONOMOUS_RATE_INTERVAL)
        {
            return false;
        }
        self.last_by_agent.insert(agent_id.to_string(), now);
        true
    }
}

#[derive(Debug, Default)]
struct ProactivityState {
    nudges: NudgeTracker,
    scanner: ScannerFingerprints,
    rotation: IdleRotation,
    rate: AutonomousRateGate,
}

#[derive(Debug, Clone, Default)]
pub struct ProactivityHub(Arc<Mutex<ProactivityState>>);

impl ProactivityHub {
    pub fn try_claim_stall(
        &self,
        room_id: &str,
        now: Instant,
    ) -> Result<NudgeClaim, ProactivityError> {
        Ok(self.lock()?.nudges.try_claim(room_id, now))
    }

    pub fn finish_stall(
        &self,
        room_id: &str,
        now: Instant,
        necessary: bool,
    ) -> Result<(), ProactivityError> {
        self.lock()?.nudges.finish(room_id, now, necessary);
        Ok(())
    }

    pub fn cancel_stall(&self, room_id: &str) -> Result<(), ProactivityError> {
        self.lock()?.nudges.cancel(room_id);
        Ok(())
    }

    pub fn observe_room_message(&self, room_id: &str) -> Result<(), ProactivityError> {
        self.lock()?.nudges.observe_room_message(room_id);
        Ok(())
    }

    pub fn observe_scanner(
        &self,
        agent_id: &str,
        fingerprint: &str,
        now: Instant,
    ) -> Result<ScannerDecision, ProactivityError> {
        Ok(self.lock()?.scanner.observe(agent_id, fingerprint, now))
    }

    pub fn choose_idle(&self, candidates: &[String]) -> Result<Option<String>, ProactivityError> {
        Ok(self.lock()?.rotation.choose(candidates))
    }

    pub fn acquire_autonomous_rate(
        &self,
        agent_id: &str,
        now: Instant,
    ) -> Result<bool, ProactivityError> {
        Ok(self.lock()?.rate.try_acquire(agent_id, now))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ProactivityState>, ProactivityError> {
        self.0.lock().map_err(|_| ProactivityError::Poisoned)
    }
}

#[derive(Debug, Error)]
pub enum ProactivityError {
    #[error("proactivity state lock is poisoned")]
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeClaim {
    Claimed,
    CoolingDown,
    ClaimedByPeer,
    DeclineCap,
}

#[derive(Debug, Default)]
pub struct NudgeTracker {
    in_flight: HashMap<String, Instant>,
    cooldown_until: HashMap<String, Instant>,
    declines: HashMap<String, u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerDecision {
    Baseline,
    Changed,
    Duplicate,
}

#[derive(Debug, Default)]
pub struct ScannerFingerprints {
    last_by_agent: HashMap<String, String>,
    seen_until: HashMap<(String, String), Instant>,
}

impl ScannerFingerprints {
    pub fn observe(&mut self, agent_id: &str, fingerprint: &str, now: Instant) -> ScannerDecision {
        self.seen_until.retain(|_, until| *until > now);
        let key = (agent_id.to_string(), fingerprint.to_string());
        let previous = self
            .last_by_agent
            .insert(agent_id.to_string(), fingerprint.to_string());
        if previous.is_none() {
            self.seen_until
                .insert(key, now + SCANNER_FINGERPRINT_COOLDOWN);
            return ScannerDecision::Baseline;
        }
        if previous.as_deref() == Some(fingerprint) || self.seen_until.contains_key(&key) {
            return ScannerDecision::Duplicate;
        }
        self.seen_until
            .insert(key, now + SCANNER_FINGERPRINT_COOLDOWN);
        ScannerDecision::Changed
    }
}

impl NudgeTracker {
    pub fn try_claim(&mut self, room_id: &str, now: Instant) -> NudgeClaim {
        self.in_flight.retain(|_, claimed_at| {
            now.saturating_duration_since(*claimed_at) < NUDGE_CLAIM_FALLBACK
        });
        if self.declines.get(room_id).copied().unwrap_or(0) >= NUDGE_DECLINE_CAP {
            return NudgeClaim::DeclineCap;
        }
        if self
            .cooldown_until
            .get(room_id)
            .is_some_and(|until| *until > now)
        {
            return NudgeClaim::CoolingDown;
        }
        if self.in_flight.contains_key(room_id) {
            return NudgeClaim::ClaimedByPeer;
        }
        self.in_flight.insert(room_id.to_string(), now);
        NudgeClaim::Claimed
    }

    pub fn finish(&mut self, room_id: &str, now: Instant, necessary: bool) {
        self.in_flight.remove(room_id);
        self.cooldown_until
            .insert(room_id.to_string(), now + NUDGE_COOLDOWN);
        if !necessary {
            let count = self.declines.entry(room_id.to_string()).or_default();
            *count = count.saturating_add(1);
        } else {
            self.declines.remove(room_id);
        }
    }

    pub fn cancel(&mut self, room_id: &str) {
        self.in_flight.remove(room_id);
    }

    pub fn observe_room_message(&mut self, room_id: &str) {
        self.declines.remove(room_id);
    }
}
