use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex};

use crate::session::SessionId;

use super::AgentControlError;

/// 根 Session 下一个可通过 `task_name` 寻址的子 Agent。
///
/// Deliberately holds no `SessionHandle`. The handle already lives in the Core
/// session registry; keeping a second copy here would give the same runtime
/// object two owners and let the two views drift once a Session is unloaded.
/// This registry owns exactly one thing: the `task_name` → `SessionId` index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgent {
    /// Unique within the parent Session; this is what the model addresses.
    pub task_name: String,
    pub session_id: SessionId,
    pub agent_role: String,
    /// 子 Agent 首次创建/注册的时间；重启恢复时取持久化的 Session 创建时间。
    pub started_at: String,
}

/// 根 Session 树内的 `task_name` → 子 Agent 寻址索引。
#[derive(Default)]
pub(super) struct SubAgentRegistry {
    entries: Mutex<HashMap<String, Slot>>,
}

enum Slot {
    /// A name claimed by an in-flight spawn that has not completed yet.
    Reserved,
    Registered(SubAgent),
}

impl SubAgentRegistry {
    /// Claims `task_name` for an in-flight spawn.
    ///
    /// Claiming happens *before* the Session row exists so two concurrent spawns
    /// cannot pick the same name and have the second one fail deep inside the
    /// database with a unique-index violation the model cannot act on.
    pub(super) fn reserve(
        self: &Arc<Self>,
        task_name: &str,
    ) -> Result<SpawnReservation, AgentControlError> {
        let mut entries = self.lock();
        match entries.entry(task_name.to_string()) {
            Entry::Occupied(_) => Err(AgentControlError::DuplicateTaskName(task_name.to_string())),
            Entry::Vacant(vacant) => {
                vacant.insert(Slot::Reserved);
                Ok(SpawnReservation {
                    registry: Arc::clone(self),
                    task_name: task_name.to_string(),
                    committed: false,
                })
            }
        }
    }

    pub(super) fn get(&self, task_name: &str) -> Option<SubAgent> {
        match self.lock().get(task_name) {
            Some(Slot::Registered(agent)) => Some(agent.clone()),
            Some(Slot::Reserved) | None => None,
        }
    }

    /// 按 `task_name` 返回可寻址的子 Agent，避免调用者观察到 `HashMap` 的随机顺序。
    pub(super) fn list(&self) -> Vec<SubAgent> {
        let mut agents: Vec<SubAgent> = self
            .lock()
            .values()
            .filter_map(|slot| match slot {
                Slot::Registered(agent) => Some(agent.clone()),
                Slot::Reserved => None,
            })
            .collect();
        agents.sort_by(|left, right| left.task_name.cmp(&right.task_name));
        agents
    }

    /// 把持久化身份恢复进新进程的寻址索引，不代表对应 Session Actor 已经驻留。
    pub(super) fn restore(&self, agent: SubAgent) -> Result<(), AgentControlError> {
        let mut entries = self.lock();
        match entries.entry(agent.task_name.clone()) {
            Entry::Vacant(vacant) => {
                vacant.insert(Slot::Registered(agent));
                Ok(())
            }
            Entry::Occupied(occupied) if matches!(occupied.get(), Slot::Registered(existing) if existing == &agent) => {
                Ok(())
            }
            Entry::Occupied(_) => Err(AgentControlError::DuplicateTaskName(agent.task_name)),
        }
    }

    pub(super) fn remove_registered(&self, task_name: &str, session_id: &SessionId) {
        let mut entries = self.lock();
        let matches_session = matches!(
            entries.get(task_name),
            Some(Slot::Registered(agent)) if &agent.session_id == session_id
        );
        if matches_session {
            entries.remove(task_name);
        }
    }

    fn release(&self, task_name: &str) {
        self.lock().remove(task_name);
    }

    fn commit(&self, agent: SubAgent) {
        self.lock()
            .insert(agent.task_name.clone(), Slot::Registered(agent));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        // A panic while holding this lock would otherwise poison every later
        // spawn in the session tree. The map is a plain name index with no
        // cross-entry invariant, so recovering the guard is safe.
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A claimed `task_name`, released automatically unless committed.
///
/// Spawning is multi-step (claim the name, insert the `sessions` row, start the
/// actor) and any step can fail. Tying the release to `Drop` means no error path
/// has to remember to clean up, and adding a step later cannot reintroduce a
/// leaked name.
pub struct SpawnReservation {
    registry: Arc<SubAgentRegistry>,
    task_name: String,
    committed: bool,
}

impl SpawnReservation {
    pub(super) fn task_name(&self) -> &str {
        &self.task_name
    }

    pub(super) fn commit(mut self, agent: SubAgent) {
        self.registry.commit(agent);
        self.committed = true;
    }
}

impl Drop for SpawnReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.registry.release(&self.task_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(task_name: &str) -> SubAgent {
        SubAgent {
            task_name: task_name.to_string(),
            session_id: SessionId::new(format!("sess-{task_name}")),
            agent_role: "explorer".to_string(),
            started_at: "2026-08-08T00:00:00+08:00".to_string(),
        }
    }

    #[test]
    fn a_reserved_name_blocks_a_second_reservation() {
        let registry = Arc::new(SubAgentRegistry::default());
        let _first = registry.reserve("find_auth").expect("first reservation");
        assert!(matches!(
            registry.reserve("find_auth"),
            Err(AgentControlError::DuplicateTaskName(name)) if name == "find_auth"
        ));
    }

    #[test]
    fn dropping_an_uncommitted_reservation_frees_the_name() {
        let registry = Arc::new(SubAgentRegistry::default());
        drop(registry.reserve("find_auth").expect("reservation"));
        assert!(
            registry.reserve("find_auth").is_ok(),
            "an abandoned spawn must not burn the name"
        );
    }

    #[test]
    fn a_reserved_but_uncommitted_name_is_not_yet_visible() {
        let registry = Arc::new(SubAgentRegistry::default());
        let _reservation = registry.reserve("find_auth").expect("reservation");
        assert!(registry.get("find_auth").is_none());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn committed_agents_are_listed_by_task_name() {
        let registry = Arc::new(SubAgentRegistry::default());
        for name in ["read_config", "find_auth"] {
            registry
                .reserve(name)
                .expect("reservation")
                .commit(agent(name));
        }
        assert_eq!(
            registry
                .list()
                .into_iter()
                .map(|agent| agent.task_name)
                .collect::<Vec<_>>(),
            vec!["find_auth".to_string(), "read_config".to_string()]
        );
        assert_eq!(registry.get("find_auth"), Some(agent("find_auth")));
    }

    #[test]
    fn a_committed_name_stays_taken() {
        let registry = Arc::new(SubAgentRegistry::default());
        registry
            .reserve("find_auth")
            .expect("reservation")
            .commit(agent("find_auth"));
        assert!(matches!(
            registry.reserve("find_auth"),
            Err(AgentControlError::DuplicateTaskName(_))
        ));
    }
}
