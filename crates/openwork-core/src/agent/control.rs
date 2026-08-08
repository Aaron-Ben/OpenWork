use std::sync::{Arc, Weak};

use async_trait::async_trait;
use thiserror::Error;

use crate::session::SessionId;
use crate::storage::is_valid_task_name;

use super::limiter::{DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS, TurnSlot, TurnSlots};
use super::registry::{SubAgent, SubAgentRegistry};

/// What the host needs in order to bring one sub-agent Session to life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentSpec {
    pub session_id: SessionId,
    pub parent_session_id: SessionId,
    pub task_name: String,
    pub agent_role: String,
    pub spawn_span_id: Option<String>,
}

/// Creates sub-agent Sessions on behalf of [`AgentControl`].
///
/// Implemented by `OpenWorkCore`, which owns the storage pool, the provider
/// factory and the Session registry. [`AgentControl`] holds it as a [`Weak`] so
/// the cycle `Core → SessionHandle → SessionActor → AgentControl → Core` cannot
/// keep the whole tree alive after the Core is dropped.
///
/// The host inherits the parent's working directory, permission profile and
/// resolved model. None of them are parameters: a sub-agent that could widen its
/// own path boundary or pick its own model would break the invariants in
/// `docs/multi-agent.md` §1.
#[async_trait]
pub trait SubAgentHost: Send + Sync {
    /// Inserts the sub-agent's `sessions` row and starts its actor.
    async fn start_sub_agent(&self, spec: SubAgentSpec) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentControlError {
    #[error("sub-agent `{0}` already exists in this session")]
    DuplicateTaskName(String),
    #[error("task_name {0:?} must match ^[a-z][a-z0-9_]{{0,47}}$")]
    InvalidTaskName(String),
    #[error("no sub-agent named `{0}` in this session")]
    UnknownTaskName(String),
    #[error(
        "at most {max} sub-agent turns may run at once; wait for one to finish before starting another"
    )]
    TurnLimitReached { max: usize },
    #[error("the session host is no longer available")]
    HostUnavailable,
    #[error("failed to start sub-agent: {0}")]
    StartFailed(String),
}

/// Runtime control plane shared by a root Session and every sub-agent under it.
///
/// Cloning is cheap and shares state: the parent hands the same instance to each
/// child so the `task_name` index and the concurrency cap are tree-wide rather
/// than per-Session.
#[derive(Clone)]
pub struct AgentControl {
    inner: Arc<Inner>,
}

struct Inner {
    root_session_id: SessionId,
    host: Weak<dyn SubAgentHost>,
    registry: Arc<SubAgentRegistry>,
    slots: Arc<TurnSlots>,
}

impl std::fmt::Debug for AgentControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentControl")
            .field("root_session_id", &self.inner.root_session_id)
            .field("active_turns", &self.inner.slots.active())
            .field("max_active_turns", &self.inner.slots.max())
            .finish_non_exhaustive()
    }
}

impl AgentControl {
    pub fn new(root_session_id: SessionId, host: Weak<dyn SubAgentHost>) -> Self {
        Self::with_max_active_turns(root_session_id, host, DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS)
    }

    pub fn with_max_active_turns(
        root_session_id: SessionId,
        host: Weak<dyn SubAgentHost>,
        max_active_turns: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                root_session_id,
                host,
                registry: Arc::new(SubAgentRegistry::default()),
                slots: Arc::new(TurnSlots::new(max_active_turns)),
            }),
        }
    }

    /// The Session every sub-agent in this tree descends from.
    pub fn root_session_id(&self) -> &SessionId {
        &self.inner.root_session_id
    }

    pub fn max_active_turns(&self) -> usize {
        self.inner.slots.max()
    }

    pub fn active_turns(&self) -> usize {
        self.inner.slots.active()
    }

    /// Takes a concurrency slot for a sub-agent Turn that is about to start.
    ///
    /// The caller holds it for the life of the Turn and drops it afterwards, so
    /// a cancelled or failed Turn frees the slot exactly like a completed one.
    pub fn try_acquire_turn_slot(&self) -> Result<TurnSlot, AgentControlError> {
        self.inner
            .slots
            .try_acquire()
            .ok_or(AgentControlError::TurnLimitReached {
                max: self.inner.slots.max(),
            })
    }

    /// Creates a sub-agent named `task_name` under the root Session.
    ///
    /// The name is claimed before anything durable happens, so a failure at any
    /// later step frees it again rather than burning it for the whole session.
    pub async fn spawn(
        &self,
        task_name: &str,
        agent_role: &str,
        spawn_span_id: Option<String>,
    ) -> Result<SubAgent, AgentControlError> {
        if !is_valid_task_name(task_name) {
            return Err(AgentControlError::InvalidTaskName(task_name.to_string()));
        }

        let reservation = self.inner.registry.reserve(task_name)?;
        let host = self
            .inner
            .host
            .upgrade()
            .ok_or(AgentControlError::HostUnavailable)?;

        let session_id = SessionId::generate();
        host.start_sub_agent(SubAgentSpec {
            session_id: session_id.clone(),
            parent_session_id: self.inner.root_session_id.clone(),
            task_name: reservation.task_name().to_string(),
            agent_role: agent_role.to_string(),
            spawn_span_id,
        })
        .await
        .map_err(AgentControlError::StartFailed)?;

        let agent = SubAgent {
            task_name: reservation.task_name().to_string(),
            session_id,
            agent_role: agent_role.to_string(),
        };
        reservation.commit(agent.clone());
        Ok(agent)
    }

    pub fn get(&self, task_name: &str) -> Result<SubAgent, AgentControlError> {
        self.inner
            .registry
            .get(task_name)
            .ok_or_else(|| AgentControlError::UnknownTaskName(task_name.to_string()))
    }

    /// Live sub-agents ordered by `task_name`.
    pub fn list(&self) -> Vec<SubAgent> {
        self.inner.registry.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingHost {
        started: Mutex<Vec<SubAgentSpec>>,
        fail_with: Option<String>,
    }

    impl RecordingHost {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                started: Mutex::new(Vec::new()),
                fail_with: None,
            })
        }

        fn failing(message: &str) -> Arc<Self> {
            Arc::new(Self {
                started: Mutex::new(Vec::new()),
                fail_with: Some(message.to_string()),
            })
        }

        fn started(&self) -> Vec<SubAgentSpec> {
            self.started.lock().expect("started specs").clone()
        }
    }

    #[async_trait]
    impl SubAgentHost for RecordingHost {
        async fn start_sub_agent(&self, spec: SubAgentSpec) -> Result<(), String> {
            if let Some(message) = &self.fail_with {
                return Err(message.clone());
            }
            self.started.lock().expect("started specs").push(spec);
            Ok(())
        }
    }

    fn control_for(host: &Arc<RecordingHost>, max_active_turns: usize) -> AgentControl {
        AgentControl::with_max_active_turns(
            SessionId::new("sess-root"),
            Arc::downgrade(host) as Weak<dyn SubAgentHost>,
            max_active_turns,
        )
    }

    #[tokio::test]
    async fn spawn_registers_the_agent_and_tells_the_host_who_its_parent_is() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);

        let agent = control
            .spawn("find_auth_flow", "explorer", Some("span-1".to_string()))
            .await
            .expect("spawn");

        assert_eq!(agent.task_name, "find_auth_flow");
        assert_eq!(agent.agent_role, "explorer");
        assert_eq!(control.get("find_auth_flow").expect("lookup"), agent);
        assert_eq!(control.list(), vec![agent.clone()]);

        let specs = host.started();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].parent_session_id, SessionId::new("sess-root"));
        assert_eq!(specs[0].session_id, agent.session_id);
        assert_eq!(specs[0].spawn_span_id.as_deref(), Some("span-1"));
    }

    #[tokio::test]
    async fn duplicate_task_names_are_refused_without_reaching_the_host() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        control
            .spawn("find_auth", "explorer", None)
            .await
            .expect("first");

        let error = control
            .spawn("find_auth", "explorer", None)
            .await
            .expect_err("second");
        assert!(matches!(error, AgentControlError::DuplicateTaskName(name) if name == "find_auth"));
        assert_eq!(host.started().len(), 1, "the host must not be called twice");
    }

    #[tokio::test]
    async fn malformed_task_names_are_refused_before_reaching_the_host() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        let too_long = "a".repeat(49);
        for name in [
            "",
            "Find_Auth",
            "9lives",
            "find-auth",
            "find auth",
            "_leading",
            too_long.as_str(),
        ] {
            let error = control
                .spawn(name, "explorer", None)
                .await
                .expect_err("must reject");
            assert!(
                matches!(error, AgentControlError::InvalidTaskName(_)),
                "{name:?} produced {error:?}"
            );
        }
        assert!(host.started().is_empty());
    }

    #[tokio::test]
    async fn the_longest_accepted_name_is_forty_eight_characters() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        let longest = format!("a{}", "b".repeat(47));
        assert_eq!(longest.len(), 48);
        control
            .spawn(&longest, "explorer", None)
            .await
            .expect("48 characters must be accepted");
    }

    #[tokio::test]
    async fn a_failed_start_frees_the_task_name() {
        let failing = RecordingHost::failing("provider unavailable");
        let control = control_for(&failing, 3);
        let error = control
            .spawn("find_auth", "explorer", None)
            .await
            .expect_err("fail");
        assert!(matches!(error, AgentControlError::StartFailed(_)));
        assert!(
            control.get("find_auth").is_err(),
            "a failed spawn must leave nothing registered"
        );

        let working = RecordingHost::new();
        let control = control_for(&working, 3);
        control
            .spawn("find_auth", "explorer", None)
            .await
            .expect("the name must be reusable after a failure");
    }

    #[tokio::test]
    async fn a_dropped_host_reports_unavailable_rather_than_panicking() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        drop(host);
        let error = control
            .spawn("find_auth", "explorer", None)
            .await
            .expect_err("no host");
        assert!(matches!(error, AgentControlError::HostUnavailable));
        assert!(
            control.get("find_auth").is_err(),
            "the name must not stay claimed when the host is gone"
        );
    }

    #[tokio::test]
    async fn clones_share_one_registry_and_one_cap() {
        let host = RecordingHost::new();
        let parent = control_for(&host, 1);
        let child_view = parent.clone();

        parent
            .spawn("find_auth", "explorer", None)
            .await
            .expect("spawn");
        assert_eq!(
            child_view.list().len(),
            1,
            "a cloned control must see the same registry"
        );

        let _slot = parent.try_acquire_turn_slot().expect("first slot");
        assert!(
            child_view.try_acquire_turn_slot().is_err(),
            "a cloned control must share the same cap"
        );
    }

    #[test]
    fn turn_slots_are_capped_and_returned_on_drop() {
        let host = RecordingHost::new();
        let control = control_for(&host, 2);
        let first = control.try_acquire_turn_slot().expect("first");
        let _second = control.try_acquire_turn_slot().expect("second");
        assert_eq!(control.active_turns(), 2);

        let error = control.try_acquire_turn_slot().expect_err("third");
        assert!(matches!(
            error,
            AgentControlError::TurnLimitReached { max: 2 }
        ));

        drop(first);
        assert_eq!(control.active_turns(), 1);
        control.try_acquire_turn_slot().expect("slot is reusable");
    }

    #[test]
    fn the_default_cap_leaves_room_for_three_parallel_sub_agents() {
        let host = RecordingHost::new();
        let control = AgentControl::new(
            SessionId::new("sess-root"),
            Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
        );
        assert_eq!(control.max_active_turns(), 3);
    }

    #[test]
    fn unknown_task_names_report_which_name_was_missing() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        let error = control.get("nope").expect_err("unknown");
        assert!(matches!(error, AgentControlError::UnknownTaskName(name) if name == "nope"));
    }
}
