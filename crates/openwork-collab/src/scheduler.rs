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
    activity::AgentRuntimeRegistry,
    coordination::CoordinationHub,
    event::{CollabEventKind, CollabEventPublisher},
    home::HomeManager,
    mcp::MessageNotice,
    model::{Agent, TriageRecordInput},
    opencode::{EngineConnection, GlobalEvent, OpenCodeClient},
    permission::PermissionTracker,
    runtime_events::{self, InstanceEventSubscriptions},
    storage::CollabStorage,
    triage::{TriageClient, TriageContext, TriageMessage, TriageSource, resolve_failure},
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
    pending: HashMap<String, Vec<MessageNotice>>,
    prompt_note: Option<String>,
}

impl RuntimeState {
    fn new(_agent: &Agent) -> Self {
        Self {
            wake: WakeState::default(),
            room_id: None,
            active_run_id: None,
            pending: HashMap::new(),
            prompt_note: None,
        }
    }
}

pub struct SchedulerHandle {
    scheduler: JoinHandle<()>,
    global_events: JoinHandle<()>,
    instance_events: JoinHandle<()>,
}

struct DispatchServices<'a> {
    storage: &'a CollabStorage,
    homes: &'a HomeManager,
    tokens: &'a AgentTokens,
    connections: &'a watch::Receiver<Option<EngineConnection>>,
    coordination: &'a CoordinationHub,
    runtime: &'a AgentRuntimeRegistry,
    subscriptions: &'a InstanceEventSubscriptions,
}

impl SchedulerHandle {
    pub async fn shutdown(self) {
        let _ = self.scheduler.await;
        let _ = self.global_events.await;
        let _ = self.instance_events.await;
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
    published_events: CollabEventPublisher,
    coordination: CoordinationHub,
    runtime: AgentRuntimeRegistry,
    cancel: CancellationToken,
) -> Result<SchedulerHandle, crate::storage::StorageError> {
    let agents = storage.agents().await?;
    let initial_subscriptions = agents
        .iter()
        .filter(|agent| agent.enabled)
        .map(|agent| (agent.id.clone(), homes.agent_home(&agent.id)))
        .collect::<Vec<_>>();
    let mut states = HashMap::new();
    let mut sessions = HashMap::new();
    for agent in agents {
        if let Some(session_id) = &agent.opencode_session_id {
            sessions.insert(session_id.clone(), agent.id.clone());
            runtime.register_session(session_id, &agent.id).await;
        }
        states.insert(agent.id.clone(), RuntimeState::new(&agent));
    }
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let (subscriptions, instance_events) = runtime_events::start(
        connections.clone(),
        event_tx.clone(),
        published_events.clone(),
        runtime.clone(),
        cancel.clone(),
    );
    for (agent_id, directory) in initial_subscriptions {
        if let Err(error) = subscriptions.ensure(&agent_id, directory).await {
            eprintln!("failed to subscribe Agent {agent_id} to /event: {error}");
        }
    }
    let event_cancel = cancel.clone();
    let global_events = tokio::spawn(forward_global_events(
        connections.clone(),
        event_tx,
        permissions,
        published_events.clone(),
        event_cancel,
    ));
    let scheduler_cancel = cancel.clone();
    let scheduler = tokio::spawn(async move {
        let triage = TriageClient::new(storage.pool().clone());
        let mut deadlines = HashMap::<(String, String), Instant>::new();
        let mut ticker = interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = scheduler_cancel.cancelled() => return,
                Some(notice) = notices.recv() => {
                    published_events
                        .publish(CollabEventKind::RoomsChanged {
                            room_id: notice.room_id.clone(),
                        })
                        .await;
                    match storage.candidate_agents(&notice.room_id, &notice.author_id).await {
                        Ok(candidates) => {
                            for agent in candidates {
                                let state = states
                                    .entry(agent.id.clone())
                                    .or_insert_with(|| RuntimeState::new(&agent));
                                state
                                    .pending
                                    .entry(notice.room_id.clone())
                                    .or_default()
                                    .push(notice.clone());
                                deadlines.insert(
                                    (agent.id, notice.room_id.clone()),
                                    Instant::now() + DEBOUNCE,
                                );
                            }
                        }
                        Err(error) => eprintln!("failed to resolve collaboration candidates: {error}"),
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
                            let services = DispatchServices {
                                storage: &storage,
                                homes: &homes,
                                tokens: &tokens,
                                connections: &connections,
                                coordination: &coordination,
                                runtime: &runtime,
                                subscriptions: &subscriptions,
                            };
                            handle_idle(&services, &mut states, &mut sessions, &agent_id).await;
                        }
                        continue;
                    }
                    if (event.session_status() == Some("idle")
                        || event.event_type() == Some("session.idle"))
                        && let Some(session_id) = event.session_id()
                        && let Some(agent_id) = sessions.get(session_id).cloned()
                    {
                        let services = DispatchServices {
                            storage: &storage,
                            homes: &homes,
                            tokens: &tokens,
                            connections: &connections,
                            coordination: &coordination,
                            runtime: &runtime,
                            subscriptions: &subscriptions,
                        };
                        handle_idle(&services, &mut states, &mut sessions, &agent_id).await;
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
                        .map(|(key, _)| key.clone())
                        .collect::<Vec<_>>();
                    for (agent_id, room_id) in due {
                        deadlines.remove(&(agent_id.clone(), room_id.clone()));
                        let services = DispatchServices {
                            storage: &storage,
                            homes: &homes,
                            tokens: &tokens,
                            connections: &connections,
                            coordination: &coordination,
                            runtime: &runtime,
                            subscriptions: &subscriptions,
                        };
                        let retry = handle_due(
                            &services,
                            &mut states,
                            &mut sessions,
                            &triage,
                            &agent_id,
                            &room_id,
                        ).await;
                        if retry {
                            deadlines.insert((agent_id, room_id), Instant::now() + DEBOUNCE);
                        }
                    }
                    for agent_id in runtime.mark_unresponsive(Duration::from_secs(5 * 60)).await {
                        published_events.publish(CollabEventKind::AgentActivityChanged {
                            agent_id,
                            activity: crate::model::AgentActivity::Unresponsive,
                        }).await;
                    }
                }
            }
        }
    });
    Ok(SchedulerHandle {
        scheduler,
        global_events,
        instance_events,
    })
}

async fn handle_due(
    services: &DispatchServices<'_>,
    states: &mut HashMap<String, RuntimeState>,
    sessions: &mut HashMap<String, String>,
    triage: &TriageClient,
    agent_id: &str,
    room_id: &str,
) -> bool {
    let Some(batch) = states
        .get_mut(agent_id)
        .and_then(|state| state.pending.remove(room_id))
    else {
        return false;
    };
    let triage = evaluate_triage(services.storage, triage, agent_id, room_id, &batch).await;
    if !triage.actionable {
        return false;
    }
    let Some(state) = states.get_mut(agent_id) else {
        return false;
    };
    state.room_id = Some(room_id.to_string());
    state.prompt_note = triage.prompt_note;
    let action = state.wake.on_debounce_elapsed();
    if let Err(error) = dispatch(
        services,
        sessions,
        agent_id,
        state,
        "message",
        action == WakeAction::Start,
    )
    .await
    {
        eprintln!("failed to wake Agent {agent_id}: {error}");
        return state.wake.on_dispatch_failed(action);
    }
    false
}

struct WakeTriage {
    actionable: bool,
    prompt_note: Option<String>,
}

async fn evaluate_triage(
    storage: &CollabStorage,
    triage: &TriageClient,
    agent_id: &str,
    room_id: &str,
    batch: &[MessageNotice],
) -> WakeTriage {
    let Some(up_to_sequence) = batch.iter().map(|notice| notice.sequence).max() else {
        return WakeTriage {
            actionable: false,
            prompt_note: None,
        };
    };
    let human_waiting = batch.iter().any(|notice| notice.author_id == "user");
    let messages = batch
        .iter()
        .map(|notice| TriageMessage {
            author_id: notice.author_id.clone(),
            sequence: notice.sequence,
            body: notice.body.clone(),
        })
        .collect::<Vec<_>>();
    let agent = match storage.agent(agent_id).await {
        Ok(Some(agent)) => agent,
        Ok(None) => {
            return WakeTriage {
                actionable: false,
                prompt_note: None,
            };
        }
        Err(error) => {
            eprintln!("failed to load Agent {agent_id} for triage: {error}");
            return WakeTriage {
                actionable: human_waiting,
                prompt_note: None,
            };
        }
    };
    let settings = match storage.triage_settings().await {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("failed to load triage settings: {error}");
            None
        }
    };
    let started = std::time::Instant::now();
    let (actionable, response_mode, source, reason, prompt_note, input_tokens, output_tokens) =
        match settings.as_ref() {
            Some(settings) => match triage
                .decide(
                    settings,
                    TriageContext {
                        agent: &agent,
                        room_id,
                        messages: &messages,
                    },
                )
                .await
            {
                Ok(result) => (
                    result.decision.actionable,
                    Some(result.decision.response_mode.as_database_str()),
                    TriageSource::SupportModel,
                    Some(result.decision.reason),
                    Some(result.decision.prompt_note),
                    result.input_tokens,
                    result.output_tokens,
                ),
                Err(error) => {
                    let fallback = resolve_failure(human_waiting, error.to_string());
                    (
                        fallback.actionable,
                        None,
                        fallback.source,
                        Some(fallback.reason),
                        None,
                        None,
                        None,
                    )
                }
            },
            None => {
                let fallback = resolve_failure(human_waiting, "triage model is not configured");
                (
                    fallback.actionable,
                    None,
                    fallback.source,
                    Some(fallback.reason),
                    None,
                    None,
                    None,
                )
            }
        };
    let latency_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let record = TriageRecordInput {
        agent_id,
        room_id,
        up_to_sequence,
        actionable,
        response_mode,
        source: source.as_str(),
        reason: reason.as_deref(),
        prompt_note: prompt_note.as_deref(),
        provider_id: settings.as_ref().map(|value| value.provider_id.as_str()),
        model_id: settings.as_ref().map(|value| value.model_id.as_str()),
        input_tokens,
        output_tokens,
        latency_ms,
    };
    if let Err(error) = storage.record_triage(record).await {
        eprintln!("failed to record triage for Agent {agent_id}: {error}");
    }
    WakeTriage {
        actionable,
        prompt_note,
    }
}

async fn handle_idle(
    services: &DispatchServices<'_>,
    states: &mut HashMap<String, RuntimeState>,
    sessions: &mut HashMap<String, String>,
    agent_id: &str,
) {
    let Some(state) = states.get_mut(agent_id) else {
        return;
    };
    if let Some(run_id) = state.active_run_id.take() {
        let _ = services
            .storage
            .finish_run(&run_id, "completed", None)
            .await;
    }
    if state.wake.on_idle() == IdleAction::Rerun
        && let Err(error) = dispatch(services, sessions, agent_id, state, "rerun", true).await
    {
        eprintln!("failed to rerun Agent {agent_id}: {error}");
        state.wake.reset();
    }
}

async fn dispatch(
    services: &DispatchServices<'_>,
    sessions: &mut HashMap<String, String>,
    agent_id: &str,
    state: &mut RuntimeState,
    trigger: &str,
    start_run: bool,
) -> Result<(), String> {
    let agent = services
        .storage
        .agent(agent_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Agent {agent_id} no longer exists"))?;
    if !agent.enabled {
        state.wake.reset();
        return Ok(());
    }
    let token = services
        .tokens
        .read()
        .map_err(|_| "Agent token registry lock is poisoned".to_string())?
        .get(agent_id)
        .cloned()
        .ok_or_else(|| format!("Agent {agent_id} has no daemon token"))?;
    let home = services
        .homes
        .repair(&agent, &token)
        .await
        .map_err(|error| error.to_string())?;
    services
        .subscriptions
        .ensure(&agent.id, home.clone())
        .await?;
    let connection = services
        .connections
        .borrow()
        .clone()
        .ok_or_else(|| "OpenCode is restarting".to_string())?;
    let session_id = ensure_session(services.storage, &connection.client, &home, &agent)
        .await
        .map_err(|error| error.to_string())?;
    sessions.insert(session_id.clone(), agent.id.clone());
    services
        .runtime
        .register_session(&session_id, &agent.id)
        .await;
    let inbox = services
        .storage
        .inbox(agent_id)
        .await
        .map_err(|error| error.to_string())?;
    let room_id = state.room_id.as_deref();
    let glance = match room_id {
        Some(room_id) => {
            let glance = services
                .storage
                .glance(room_id, 50)
                .await
                .map_err(|error| error.to_string())?;
            services
                .coordination
                .observe(agent_id, room_id, glance.highest_sequence)
                .await;
            Some(glance)
        }
        None => None,
    };
    let roster = match room_id {
        Some(room_id) => services
            .storage
            .room_summaries(agent_id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|room| room.id == room_id)
            .map(|room| room.members),
        None => None,
    };
    let memory = tokio::fs::read_to_string(home.join("memory/MEMORY.md"))
        .await
        .map_err(|error| error.to_string())?;
    let prompt = json!({
        "instruction": "Triage found this room update actionable for you. Read the published state, decide independently whether speaking still adds value, then use openwork_reply or openwork_react. Natural-language output without a successful tool call does not publish a response.",
        "roomId": room_id,
        "unread": inbox,
        "recentPublishedState": glance,
        "roster": roster,
        "memory": memory,
        "triagePromptNote": state.prompt_note.as_deref(),
        "delivery": "Unread is authoritative. This prompt and its seen coordination cursor do not clear or update last_read_seq."
    })
    .to_string();
    let run_id = if start_run {
        Some(
            services
                .storage
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
            let _ = services
                .storage
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
    published_events: CollabEventPublisher,
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
            published_events
                .publish(CollabEventKind::EngineChanged)
                .await;
            published_events
                .publish(CollabEventKind::PermissionsChanged)
                .await;
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
                        let permissions_changed = matches!(
                            event.event_type(),
                            Some("permission.asked" | "permission.replied")
                        );
                        permissions.observe(&event).await;
                        if permissions_changed {
                            published_events
                                .publish(CollabEventKind::PermissionsChanged)
                                .await;
                        }
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
