use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI64, Ordering},
};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::protocol::{
    ComputerHeartbeatRequest, EngineReadinessView, InvalidationEvent, InvalidationKind,
    RunnerStatusView, entity_id,
};

use super::auth::SigningKey;

type HmacSha256 = Hmac<Sha256>;

pub struct RuntimeCredentials {
    pub runtime_session_id: String,
    pub desktop_secret: String,
    pub computer_secret: String,
}

impl RuntimeCredentials {
    pub fn generate() -> Self {
        Self {
            runtime_session_id: format!("runtime-{}", Uuid::new_v4().simple()),
            desktop_secret: random_secret(),
            computer_secret: random_secret(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeSession {
    inner: Arc<RuntimeSessionInner>,
}

struct RuntimeSessionInner {
    id: String,
    started_at: i64,
    last_computer_heartbeat: AtomicI64,
    verifier_key: Vec<u8>,
    desktop_tag: Vec<u8>,
    computer_tag: Vec<u8>,
    signing_key: SigningKey,
    actual_state: RwLock<ComputerHeartbeatRequest>,
    desktop_events: broadcast::Sender<InvalidationEvent>,
    management_events: broadcast::Sender<InvalidationEvent>,
}

impl RuntimeSession {
    pub(crate) fn new(credentials: RuntimeCredentials) -> Result<Self, RuntimeSessionError> {
        if credentials.runtime_session_id.trim().is_empty()
            || credentials.desktop_secret.len() < 32
            || credentials.computer_secret.len() < 32
        {
            return Err(RuntimeSessionError::InvalidCredentials);
        }
        let verifier_key = random_secret().into_bytes();
        let desktop_tag = credential_tag(
            &verifier_key,
            b"desktop",
            credentials.desktop_secret.as_bytes(),
        );
        let computer_tag = credential_tag(
            &verifier_key,
            b"computer",
            credentials.computer_secret.as_bytes(),
        );
        let (desktop_events, _) = broadcast::channel(128);
        let (management_events, _) = broadcast::channel(128);
        Ok(Self {
            inner: Arc::new(RuntimeSessionInner {
                id: credentials.runtime_session_id,
                started_at: time::OffsetDateTime::now_utc().unix_timestamp(),
                last_computer_heartbeat: AtomicI64::new(0),
                verifier_key,
                desktop_tag,
                computer_tag,
                signing_key: SigningKey::ephemeral(),
                actual_state: RwLock::new(ComputerHeartbeatRequest::default()),
                desktop_events,
                management_events,
            }),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.inner.id
    }

    pub(crate) fn started_at(&self) -> i64 {
        self.inner.started_at
    }

    pub(crate) fn last_computer_heartbeat(&self) -> Option<i64> {
        match self.inner.last_computer_heartbeat.load(Ordering::Relaxed) {
            0 => None,
            value => Some(value),
        }
    }

    pub(crate) fn note_computer_heartbeat(&self, mut state: ComputerHeartbeatRequest) {
        self.touch_computer_heartbeat();
        normalize_actual_state(&mut state);
        let (engines_changed, runners_changed) = {
            let mut current = self
                .inner
                .actual_state
                .write()
                .expect("RuntimeSession actual-state lock poisoned");
            let engines_changed = current.engine_readiness != state.engine_readiness;
            let runners_changed = current.runners != state.runners;
            *current = state;
            (engines_changed, runners_changed)
        };
        if engines_changed {
            let _ = self.inner.desktop_events.send(event(
                InvalidationKind::EngineInventory,
                None,
                None,
            ));
        }
        if runners_changed {
            let _ =
                self.inner
                    .desktop_events
                    .send(event(InvalidationKind::RunnerStatus, None, None));
        }
    }

    pub(crate) fn touch_computer_heartbeat(&self) {
        self.inner.last_computer_heartbeat.store(
            time::OffsetDateTime::now_utc().unix_timestamp(),
            Ordering::Relaxed,
        );
    }

    pub(crate) fn engine_readiness(&self) -> Vec<EngineReadinessView> {
        self.inner
            .actual_state
            .read()
            .expect("RuntimeSession actual-state lock poisoned")
            .engine_readiness
            .clone()
    }

    pub(crate) fn runner_statuses(&self) -> Vec<RunnerStatusView> {
        self.inner
            .actual_state
            .read()
            .expect("RuntimeSession actual-state lock poisoned")
            .runners
            .clone()
    }

    pub(crate) fn authorize_desktop(&self, candidate: &str) -> bool {
        verify_credential(
            &self.inner.verifier_key,
            b"desktop",
            candidate.as_bytes(),
            &self.inner.desktop_tag,
        )
    }

    pub(crate) fn authorize_computer(&self, candidate: &str) -> bool {
        verify_credential(
            &self.inner.verifier_key,
            b"computer",
            candidate.as_bytes(),
            &self.inner.computer_tag,
        )
    }

    pub(crate) fn signing_key(&self) -> &SigningKey {
        &self.inner.signing_key
    }

    pub(crate) fn subscribe_desktop(&self) -> broadcast::Receiver<InvalidationEvent> {
        self.inner.desktop_events.subscribe()
    }

    pub(crate) fn subscribe_management(&self) -> broadcast::Receiver<InvalidationEvent> {
        self.inner.management_events.subscribe()
    }

    pub(crate) fn publish_agent_config(&self, agent_id: &str, revision: i64) {
        let event = event(
            InvalidationKind::AgentConfig,
            Some(agent_id.to_string()),
            Some(revision),
        );
        let _ = self.inner.management_events.send(event.clone());
        let _ = self.inner.desktop_events.send(event);
    }

    pub(crate) fn publish_inventory(&self, engine_id: &str) {
        let event = event(
            InvalidationKind::EngineInventory,
            Some(engine_id.to_string()),
            None,
        );
        let _ = self.inner.desktop_events.send(event);
    }

    pub(crate) fn publish_runtime_ready(&self) {
        let event = event(InvalidationKind::RuntimeReady, None, None);
        let _ = self.inner.desktop_events.send(event.clone());
        let _ = self.inner.management_events.send(event);
    }
}

fn normalize_actual_state(state: &mut ComputerHeartbeatRequest) {
    state
        .engine_readiness
        .sort_by(|left, right| left.engine_id.cmp(&right.engine_id));
    state
        .engine_readiness
        .dedup_by(|left, right| left.engine_id == right.engine_id);
    state
        .runners
        .sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    state
        .runners
        .dedup_by(|left, right| left.agent_id == right.agent_id);
}

fn event(
    kind: InvalidationKind,
    subject_id: Option<String>,
    revision: Option<i64>,
) -> InvalidationEvent {
    InvalidationEvent {
        id: entity_id("event"),
        kind,
        subject_id,
        revision,
        published_at: time::OffsetDateTime::now_utc().unix_timestamp(),
    }
}

fn random_secret() -> String {
    format!(
        "{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn credential_tag(key: &[u8], scope: &[u8], credential: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(scope);
    mac.update(&[0]);
    mac.update(credential);
    mac.finalize().into_bytes().to_vec()
}

fn verify_credential(key: &[u8], scope: &[u8], candidate: &[u8], tag: &[u8]) -> bool {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(scope);
    mac.update(&[0]);
    mac.update(candidate);
    mac.verify_slice(tag).is_ok()
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeSessionError {
    #[error("RuntimeSession credentials are invalid")]
    InvalidCredentials,
}

#[cfg(test)]
mod tests {
    use crate::protocol::{
        ComputerHeartbeatRequest, EngineReadinessView, EngineStatus, InvalidationKind, RunnerState,
        RunnerStatusView,
    };

    use super::{RuntimeCredentials, RuntimeSession};

    #[test]
    fn credentials_are_scoped_and_not_interchangeable() {
        let credentials = RuntimeCredentials::generate();
        let desktop = credentials.desktop_secret.clone();
        let computer = credentials.computer_secret.clone();
        let session = RuntimeSession::new(credentials).unwrap();

        assert!(session.authorize_desktop(&desktop));
        assert!(session.authorize_computer(&computer));
        assert!(!session.authorize_desktop(&computer));
        assert!(!session.authorize_computer(&desktop));
    }

    #[tokio::test]
    async fn computer_heartbeat_projects_actual_state_and_only_invalidates_on_change() {
        let session = RuntimeSession::new(RuntimeCredentials::generate()).unwrap();
        let mut events = session.subscribe_desktop();
        let state = ComputerHeartbeatRequest {
            engine_readiness: vec![EngineReadinessView {
                engine_id: "opencode".to_string(),
                status: EngineStatus::Ready,
            }],
            runners: vec![RunnerStatusView {
                agent_id: "helper".to_string(),
                config_revision: 2,
                state: RunnerState::Running,
                last_error: None,
            }],
        };

        session.note_computer_heartbeat(state.clone());
        let first = events.recv().await.unwrap();
        let second = events.recv().await.unwrap();
        assert_eq!(first.kind, InvalidationKind::EngineInventory);
        assert_eq!(second.kind, InvalidationKind::RunnerStatus);
        assert_eq!(session.engine_readiness(), state.engine_readiness);
        assert_eq!(session.runner_statuses(), state.runners);

        session.note_computer_heartbeat(state);
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
