use std::sync::{Arc, Weak};

use async_trait::async_trait;
use thiserror::Error;

use serde::Serialize;

use crate::session::{
    AgentMessageKind, SessionHandle, SessionId, SessionRuntimeSnapshot, TurnOutcome,
};
use crate::storage::is_valid_task_name;
use crate::storage::time::{china_now, to_wire};

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

    /// 在已启动的子 Session 上开始一个 Turn，并接管并发 guard。
    async fn start_sub_agent_turn(
        &self,
        session_id: &SessionId,
        message: String,
        turn_slot: TurnSlot,
    ) -> Result<(), String>;

    /// 返回 Core 拥有的 handle；持久化但未驻留的子 Session 会按需加载。
    async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String>;
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
    #[error("failed to find parent session: {0}")]
    ParentSessionUnavailable(String),
    #[error("failed to deliver sub-agent message: {0}")]
    DeliveryFailed(String),
    #[error("failed to start sub-agent turn: {0}")]
    TurnStartFailed(String),
    #[error("failed to inspect sub-agent: {0}")]
    InspectFailed(String),
    #[error("failed to interrupt sub-agent: {0}")]
    InterruptFailed(String),
}

impl AgentControlError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TurnLimitReached { .. } => "agent_limit_reached",
            Self::DuplicateTaskName(_) => "duplicate_task_name",
            Self::InvalidTaskName(_) => "invalid_task_name",
            Self::UnknownTaskName(_) => "unknown_agent",
            Self::HostUnavailable => "agent_host_unavailable",
            Self::StartFailed(_) => "agent_start_failed",
            Self::ParentSessionUnavailable(_) => "parent_session_unavailable",
            Self::DeliveryFailed(_) => "agent_delivery_failed",
            Self::TurnStartFailed(_) => "agent_turn_start_failed",
            Self::InspectFailed(_) => "agent_inspection_failed",
            Self::InterruptFailed(_) => "agent_interrupt_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubAgentStatus {
    pub task_name: String,
    pub status: String,
    pub started_at: String,
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

    /// 恢复数据库里已有的子 Agent 身份，不启动 Session 或 Turn，也不占并发名额。
    pub(crate) fn restore_agents(
        &self,
        agents: impl IntoIterator<Item = SubAgent>,
    ) -> Result<(), AgentControlError> {
        for agent in agents {
            self.inner.registry.restore(agent)?;
        }
        Ok(())
    }

    pub(crate) fn forget_agent(&self, task_name: &str, session_id: &SessionId) {
        self.inner.registry.remove_registered(task_name, session_id);
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
        message: String,
        spawn_span_id: Option<String>,
    ) -> Result<SubAgent, AgentControlError> {
        if !is_valid_task_name(task_name) {
            return Err(AgentControlError::InvalidTaskName(task_name.to_string()));
        }

        let reservation = self.inner.registry.reserve(task_name)?;
        let turn_slot = self.try_acquire_turn_slot()?;
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
            agent_role: "explorer".to_string(),
            spawn_span_id,
        })
        .await
        .map_err(AgentControlError::StartFailed)?;

        host.start_sub_agent_turn(&session_id, message, turn_slot)
            .await
            .map_err(AgentControlError::TurnStartFailed)?;

        let started_at = china_now();
        let agent = SubAgent {
            task_name: reservation.task_name().to_string(),
            session_id,
            agent_role: "explorer".to_string(),
            started_at: to_wire(started_at).unwrap_or_else(|| started_at.to_string()),
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

    /// 按 `task_name` 返回可寻址的子 Agent。
    pub fn list(&self) -> Vec<SubAgent> {
        self.inner.registry.list()
    }

    pub async fn list_statuses(&self) -> Result<Vec<SubAgentStatus>, AgentControlError> {
        let host = self
            .inner
            .host
            .upgrade()
            .ok_or(AgentControlError::HostUnavailable)?;
        let mut statuses = Vec::new();
        for agent in self.list() {
            let handle = host
                .session_handle(&agent.session_id)
                .await
                .map_err(AgentControlError::InspectFailed)?;
            let snapshot = handle
                .snapshot()
                .await
                .map_err(|error| AgentControlError::InspectFailed(error.to_string()))?;
            statuses.push(SubAgentStatus {
                task_name: agent.task_name,
                status: runtime_status(&snapshot.runtime).to_string(),
                started_at: agent.started_at,
            });
        }
        Ok(statuses)
    }

    pub async fn followup(
        &self,
        task_name: &str,
        message: String,
    ) -> Result<(), AgentControlError> {
        let agent = self.get(task_name)?;
        let turn_slot = self.try_acquire_turn_slot()?;
        let host = self
            .inner
            .host
            .upgrade()
            .ok_or(AgentControlError::HostUnavailable)?;
        host.start_sub_agent_turn(&agent.session_id, message, turn_slot)
            .await
            .map_err(AgentControlError::TurnStartFailed)
    }

    pub async fn interrupt(&self, task_name: &str) -> Result<(), AgentControlError> {
        let agent = self.get(task_name)?;
        let host = self
            .inner
            .host
            .upgrade()
            .ok_or(AgentControlError::HostUnavailable)?;
        let handle = host
            .session_handle(&agent.session_id)
            .await
            .map_err(AgentControlError::InterruptFailed)?;
        let snapshot = handle
            .snapshot()
            .await
            .map_err(|error| AgentControlError::InterruptFailed(error.to_string()))?;
        let SessionRuntimeSnapshot::Running { turn_id, .. } = snapshot.runtime else {
            return Err(AgentControlError::InterruptFailed(format!(
                "sub-agent `{task_name}` has no active turn"
            )));
        };
        handle
            .cancel_turn(turn_id)
            .await
            .map_err(|error| AgentControlError::InterruptFailed(error.to_string()))?;
        Ok(())
    }

    pub async fn deliver_to_parent(
        &self,
        parent_session_id: &SessionId,
        child_session_id: &SessionId,
        child_turn_id: &crate::session::TurnId,
        task_name: &str,
        kind: AgentMessageKind,
        body: &str,
    ) -> Result<(), AgentControlError> {
        let host = self
            .inner
            .host
            .upgrade()
            .ok_or(AgentControlError::HostUnavailable)?;
        let parent = host
            .session_handle(parent_session_id)
            .await
            .map_err(AgentControlError::ParentSessionUnavailable)?;
        parent
            .deliver_agent_message(
                child_session_id.clone(),
                child_turn_id.clone(),
                task_name,
                kind,
                body,
            )
            .await
    }
}

fn runtime_status(runtime: &SessionRuntimeSnapshot) -> &'static str {
    match runtime {
        SessionRuntimeSnapshot::Idle => "idle",
        SessionRuntimeSnapshot::Running { .. } => "running",
        SessionRuntimeSnapshot::Terminal { outcome, .. } => match outcome {
            TurnOutcome::Completed { .. } => "completed",
            TurnOutcome::Failed { .. } => "failed",
            TurnOutcome::Cancelled => "cancelled",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct RecordingHost {
        started: Mutex<Vec<SubAgentSpec>>,
        fail_with: Option<String>,
    }

    #[derive(Default)]
    struct HoldingHost {
        slots: Mutex<HashMap<SessionId, TurnSlot>>,
    }

    impl HoldingHost {
        fn finish(&self, session_id: &SessionId) {
            self.slots.lock().expect("held slots").remove(session_id);
        }
    }

    #[async_trait]
    impl SubAgentHost for HoldingHost {
        async fn start_sub_agent(&self, _spec: SubAgentSpec) -> Result<(), String> {
            Ok(())
        }

        async fn start_sub_agent_turn(
            &self,
            session_id: &SessionId,
            _message: String,
            turn_slot: TurnSlot,
        ) -> Result<(), String> {
            self.slots
                .lock()
                .expect("held slots")
                .insert(session_id.clone(), turn_slot);
            Ok(())
        }

        async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String> {
            Err(format!(
                "session {session_id} has no actor in this limiter test"
            ))
        }
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

        async fn start_sub_agent_turn(
            &self,
            _session_id: &SessionId,
            _message: String,
            _turn_slot: TurnSlot,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn session_handle(&self, session_id: &SessionId) -> Result<SessionHandle, String> {
            Err(format!(
                "session {session_id} is not configured in this host"
            ))
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
            .spawn(
                "find_auth_flow",
                "inspect auth flow".to_string(),
                Some("span-1".to_string()),
            )
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
    async fn spawn_and_followup_share_the_active_turn_limit() {
        let host = Arc::new(HoldingHost::default());
        let control = AgentControl::with_max_active_turns(
            SessionId::new("sess-root"),
            Arc::downgrade(&host) as Weak<dyn SubAgentHost>,
            3,
        );
        let mut agents = Vec::new();
        for index in 0..3 {
            agents.push(
                control
                    .spawn(
                        &format!("lookup_{index}"),
                        format!("inspect area {index}"),
                        None,
                    )
                    .await
                    .expect("within limit"),
            );
        }

        let error = control
            .spawn("lookup_3", "inspect fourth area".to_string(), None)
            .await
            .expect_err("fourth active turn must be rejected");
        assert!(matches!(
            error,
            AgentControlError::TurnLimitReached { max: 3 }
        ));
        assert_eq!(error.code(), "agent_limit_reached");

        host.finish(&agents[0].session_id);
        control
            .followup("lookup_0", "inspect one more detail".to_string())
            .await
            .expect("an idle child frees and can reacquire its slot");
        assert_eq!(control.active_turns(), 3);
    }

    #[tokio::test]
    async fn duplicate_task_names_are_refused_without_reaching_the_host() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        control
            .spawn("find_auth", "inspect auth".to_string(), None)
            .await
            .expect("first");

        let error = control
            .spawn("find_auth", "inspect auth".to_string(), None)
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
                .spawn(name, "inspect".to_string(), None)
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
            .spawn(&longest, "inspect".to_string(), None)
            .await
            .expect("48 characters must be accepted");
    }

    #[tokio::test]
    async fn a_failed_start_frees_the_task_name() {
        let failing = RecordingHost::failing("provider unavailable");
        let control = control_for(&failing, 3);
        let error = control
            .spawn("find_auth", "inspect auth".to_string(), None)
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
            .spawn("find_auth", "inspect auth".to_string(), None)
            .await
            .expect("the name must be reusable after a failure");
    }

    #[tokio::test]
    async fn a_dropped_host_reports_unavailable_rather_than_panicking() {
        let host = RecordingHost::new();
        let control = control_for(&host, 3);
        drop(host);
        let error = control
            .spawn("find_auth", "inspect auth".to_string(), None)
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
            .spawn("find_auth", "inspect auth".to_string(), None)
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
