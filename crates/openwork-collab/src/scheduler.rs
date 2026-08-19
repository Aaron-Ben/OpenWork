use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use serde_json::json;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{Instant, interval},
};
use tokio_util::sync::CancellationToken;

use crate::{
    home::HomeManager,
    mcp::MessageNotice,
    model::Agent,
    opencode::{EngineConnection, GlobalEvent, OpenCodeClient},
    permission::PermissionTracker,
    storage::CollabStorage,
};

pub const DEBOUNCE: Duration = Duration::from_millis(2_500);
pub type AgentTokens = Arc<RwLock<HashMap<String, String>>>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WakeState {
    running: bool,
    pending_rerun: bool,
}

impl WakeState {
    pub fn on_debounce_elapsed(&mut self) -> WakeAction {
        if self.running {
            self.pending_rerun = true;
            WakeAction::Inject
        } else {
            self.running = true;
            WakeAction::Start
        }
    }

    pub fn on_idle(&mut self) -> IdleAction {
        if self.pending_rerun {
            self.pending_rerun = false;
            IdleAction::Rerun
        } else {
            self.running = false;
            IdleAction::Stop
        }
    }

    pub fn reset(&mut self) {
        self.running = false;
        self.pending_rerun = false;
    }

    pub fn on_engine_restart(&mut self) -> bool {
        if self.running {
            self.pending_rerun = true;
            true
        } else {
            false
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn has_pending_rerun(&self) -> bool {
        self.pending_rerun
    }

    pub fn on_dispatch_failed(&mut self, action: WakeAction) -> bool {
        if action == WakeAction::Start {
            self.reset();
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeAction {
    Start,
    Inject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    Stop,
    Rerun,
}

struct RuntimeState {
    wake: WakeState,
    room_id: Option<String>,
    active_run_id: Option<String>,
}

impl RuntimeState {
    fn new(_agent: &Agent) -> Self {
        Self {
            wake: WakeState::default(),
            room_id: None,
            active_run_id: None,
        }
    }
}

pub struct SchedulerHandle {
    scheduler: JoinHandle<()>,
    events: JoinHandle<()>,
}

impl SchedulerHandle {
    pub async fn shutdown(self) {
        let _ = self.scheduler.await;
        let _ = self.events.await;
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn start(
    storage: CollabStorage,
    homes: HomeManager,
    tokens: AgentTokens,
    connections: watch::Receiver<Option<EngineConnection>>,
    mut notices: mpsc::UnboundedReceiver<MessageNotice>,
    permissions: PermissionTracker,
    cancel: CancellationToken,
) -> Result<SchedulerHandle, crate::storage::StorageError> {
    let agents = storage.agents().await?;
    let mut states = HashMap::new();
    let mut sessions = HashMap::new();
    for agent in agents {
        if let Some(session_id) = &agent.opencode_session_id {
            sessions.insert(session_id.clone(), agent.id.clone());
        }
        states.insert(agent.id.clone(), RuntimeState::new(&agent));
    }
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let event_cancel = cancel.clone();
    let events = tokio::spawn(forward_global_events(
        connections.clone(),
        event_tx,
        permissions,
        event_cancel,
    ));
    let scheduler_cancel = cancel.clone();
    let scheduler = tokio::spawn(async move {
        let mut deadlines = HashMap::<String, Instant>::new();
        let mut ticker = interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = scheduler_cancel.cancelled() => return,
                Some(notice) = notices.recv() => {
                    match storage.mentioned_agents(&notice.room_id, &notice.body).await {
                        Ok(mentioned) => {
                            for agent in mentioned.into_iter().filter(|agent| agent.id != notice.author_id) {
                                states.entry(agent.id.clone()).or_insert_with(|| RuntimeState::new(&agent)).room_id = Some(notice.room_id.clone());
                                deadlines.insert(agent.id, Instant::now() + DEBOUNCE);
                            }
                        }
                        Err(error) => eprintln!("failed to resolve @ mentions: {error}"),
                    }
                }
                Some(event) = event_rx.recv() => {
                    if event.event_type() == Some("openwork.engine.restarted") {
                        let affected = states
                            .iter_mut()
                            .filter_map(|(agent_id, state)| {
                                state.wake.on_engine_restart().then(|| {
                                    (agent_id.clone(), state.active_run_id.take())
                                })
                            })
                            .collect::<Vec<_>>();
                        for (agent_id, run_id) in affected {
                            if let Some(run_id) = run_id {
                                let _ = storage.finish_run(
                                    &run_id,
                                    "interrupted",
                                    Some(("opencode_restarted", "opencode serve restarted during the run")),
                                ).await;
                            }
                            handle_idle(
                                &storage,
                                &homes,
                                &tokens,
                                &connections,
                                &mut states,
                                &mut sessions,
                                &agent_id,
                            ).await;
                        }
                        continue;
                    }
                    if event.session_status() == Some("idle")
                        && let Some(session_id) = event.session_id()
                        && let Some(agent_id) = sessions.get(session_id).cloned()
                    {
                        handle_idle(
                            &storage,
                            &homes,
                            &tokens,
                            &connections,
                            &mut states,
                            &mut sessions,
                            &agent_id,
                        ).await;
                    }
                    if event.event_type() == Some("session.error")
                        && let Some(session_id) = event.session_id()
                        && let Some(agent_id) = sessions.get(session_id)
                        && let Some(state) = states.get_mut(agent_id)
                        && let Some(run_id) = state.active_run_id.take()
                    {
                        let message = event.payload.to_string();
                        let _ = storage.finish_run(&run_id, "failed", Some(("opencode_session_error", &message))).await;
                    }
                }
                _ = ticker.tick() => {
                    let now = Instant::now();
                    let due = deadlines
                        .iter()
                        .filter(|(_, deadline)| **deadline <= now)
                        .map(|(agent_id, _)| agent_id.clone())
                        .collect::<Vec<_>>();
                    for agent_id in due {
                        deadlines.remove(&agent_id);
                        let retry = handle_due(
                            &storage,
                            &homes,
                            &tokens,
                            &connections,
                            &mut states,
                            &mut sessions,
                            &agent_id,
                        ).await;
                        if retry {
                            deadlines.insert(agent_id, Instant::now() + DEBOUNCE);
                        }
                    }
                }
            }
        }
    });
    Ok(SchedulerHandle { scheduler, events })
}

async fn handle_due(
    storage: &CollabStorage,
    homes: &HomeManager,
    tokens: &AgentTokens,
    connections: &watch::Receiver<Option<EngineConnection>>,
    states: &mut HashMap<String, RuntimeState>,
    sessions: &mut HashMap<String, String>,
    agent_id: &str,
) -> bool {
    let Some(state) = states.get_mut(agent_id) else {
        return false;
    };
    let action = state.wake.on_debounce_elapsed();
    let trigger = match action {
        WakeAction::Start => "message",
        WakeAction::Inject => "message",
    };
    if let Err(error) = dispatch(
        storage,
        homes,
        tokens,
        connections,
        sessions,
        agent_id,
        state,
        trigger,
        action == WakeAction::Start,
    )
    .await
    {
        eprintln!("failed to wake Agent {agent_id}: {error}");
        return state.wake.on_dispatch_failed(action);
    }
    false
}

async fn handle_idle(
    storage: &CollabStorage,
    homes: &HomeManager,
    tokens: &AgentTokens,
    connections: &watch::Receiver<Option<EngineConnection>>,
    states: &mut HashMap<String, RuntimeState>,
    sessions: &mut HashMap<String, String>,
    agent_id: &str,
) {
    let Some(state) = states.get_mut(agent_id) else {
        return;
    };
    if let Some(run_id) = state.active_run_id.take() {
        let _ = storage.finish_run(&run_id, "completed", None).await;
    }
    if state.wake.on_idle() == IdleAction::Rerun
        && let Err(error) = dispatch(
            storage,
            homes,
            tokens,
            connections,
            sessions,
            agent_id,
            state,
            "rerun",
            true,
        )
        .await
    {
        eprintln!("failed to rerun Agent {agent_id}: {error}");
        state.wake.reset();
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch(
    storage: &CollabStorage,
    homes: &HomeManager,
    tokens: &AgentTokens,
    connections: &watch::Receiver<Option<EngineConnection>>,
    sessions: &mut HashMap<String, String>,
    agent_id: &str,
    state: &mut RuntimeState,
    trigger: &str,
    start_run: bool,
) -> Result<(), String> {
    let agent = storage
        .agent(agent_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Agent {agent_id} no longer exists"))?;
    if !agent.enabled {
        state.wake.reset();
        return Ok(());
    }
    let token = tokens
        .read()
        .map_err(|_| "Agent token registry lock is poisoned".to_string())?
        .get(agent_id)
        .cloned()
        .ok_or_else(|| format!("Agent {agent_id} has no daemon token"))?;
    let home = homes
        .repair(&agent, &token)
        .await
        .map_err(|error| error.to_string())?;
    let connection = connections
        .borrow()
        .clone()
        .ok_or_else(|| "OpenCode is restarting".to_string())?;
    let session_id = ensure_session(storage, &connection.client, &home, &agent)
        .await
        .map_err(|error| error.to_string())?;
    sessions.insert(session_id.clone(), agent.id.clone());
    let inbox = storage
        .inbox(agent_id)
        .await
        .map_err(|error| error.to_string())?;
    let room_id = state.room_id.as_deref();
    let prompt = json!({
        "instruction": "You were explicitly @mentioned. Read the unread messages below, then use openwork_reply to answer in the relevant room. Natural-language output without a successful reply tool call does not publish a response.",
        "roomId": room_id,
        "unread": inbox,
        "delivery": "Unread is authoritative. This prompt does not clear it."
    })
    .to_string();
    let run_id = if start_run {
        Some(
            storage
                .begin_run(&agent, room_id, trigger)
                .await
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    if let Err(error) = connection
        .client
        .prompt_async(&home, &session_id, &agent, &prompt)
        .await
    {
        if let Some(run_id) = &run_id {
            let message = error.to_string();
            let _ = storage
                .finish_run(run_id, "failed", Some(("prompt_async_failed", &message)))
                .await;
        }
        return Err(error.to_string());
    }
    if let Some(run_id) = run_id {
        state.active_run_id = Some(run_id);
    }
    Ok(())
}

async fn ensure_session(
    storage: &CollabStorage,
    client: &OpenCodeClient,
    home: &std::path::Path,
    agent: &Agent,
) -> Result<String, crate::opencode::OpenCodeError> {
    if let Some(session_id) = &agent.opencode_session_id
        && client.get_session(home, session_id).await?.is_some()
    {
        return Ok(session_id.clone());
    }
    let session = client
        .create_session(
            home,
            &format!("OpenWork collaborator {}", agent.display_name),
        )
        .await?;
    storage
        .set_opencode_session(&agent.id, &session.id)
        .await
        .map_err(|error| crate::opencode::OpenCodeError::Persistence(error.to_string()))?;
    Ok(session.id)
}

async fn forward_global_events(
    mut connections: watch::Receiver<Option<EngineConnection>>,
    events: mpsc::UnboundedSender<GlobalEvent>,
    permissions: PermissionTracker,
    cancel: CancellationToken,
) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let Some(connection) = connections.borrow().clone() else {
            tokio::select! {
                _ = cancel.cancelled() => return,
                changed = connections.changed() => {
                    if changed.is_err() { return; }
                    continue;
                }
            }
        };
        let generation = connection.generation;
        let mut stream = match connection.client.global_events().await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("failed to subscribe /global/event: {error}");
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => continue,
                }
            }
        };
        if generation > 1 {
            let restarted = GlobalEvent {
                directory: None,
                project: None,
                payload: json!({"type": "openwork.engine.restarted", "properties": {}}),
            };
            permissions.observe(&restarted).await;
            if events.send(restarted).is_err() {
                return;
            }
        }
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                changed = connections.changed() => {
                    if changed.is_err() { return; }
                    if connections.borrow().as_ref().is_none_or(|current| current.generation != generation) {
                        break;
                    }
                }
                event = stream.next() => match event {
                    Ok(event) => {
                        permissions.observe(&event).await;
                        if events.send(event).is_err() { return; }
                    }
                    Err(error) => {
                        eprintln!("/global/event disconnected: {error}");
                        break;
                    }
                }
            }
        }
    }
}
