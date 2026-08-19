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
    autonomy::{self, AutonomousWake},
    coordination::CoordinationHub,
    event::{CollabEventKind, CollabEventPublisher},
    global_events,
    home::HomeManager,
    mcp::MessageNotice,
    model::{Agent, Inbox, RoomGlance, RoomMember},
    observation::{ObservationSink, OwnedObservation, record_engine_observation},
    opencode::{EngineConnection, OpenCodeClient},
    permission::PermissionTracker,
    proactivity::ProactivityHub,
    runtime_events::{self, InstanceEventSubscriptions},
    storage::CollabStorage,
    triage::{TriageClient, TriageSource},
    wake_triage,
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
    proactive: Option<ProactivePrompt>,
}

struct ProactivePrompt {
    reason: String,
    context: serde_json::Value,
}

impl RuntimeState {
    fn new(_agent: &Agent) -> Self {
        Self {
            wake: WakeState::default(),
            room_id: None,
            active_run_id: None,
            pending: HashMap::new(),
            prompt_note: None,
            proactive: None,
        }
    }
}

pub struct SchedulerHandle {
    scheduler: JoinHandle<()>,
    global_events: JoinHandle<()>,
    instance_events: JoinHandle<()>,
    autonomy: autonomy::AutonomyHandle,
}

struct DispatchServices<'a> {
    storage: &'a CollabStorage,
    homes: &'a HomeManager,
    tokens: &'a AgentTokens,
    connections: &'a watch::Receiver<Option<EngineConnection>>,
    coordination: &'a CoordinationHub,
    runtime: &'a AgentRuntimeRegistry,
    subscriptions: &'a InstanceEventSubscriptions,
    observations: &'a ObservationSink,
}

impl SchedulerHandle {
    pub async fn shutdown(self) {
        let _ = self.scheduler.await;
        let _ = self.global_events.await;
        let _ = self.instance_events.await;
        self.autonomy.shutdown().await;
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
    observations: ObservationSink,
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
    let global_events = global_events::start(
        connections.clone(),
        event_tx,
        permissions,
        published_events.clone(),
        event_cancel,
    );
    let proactivity = ProactivityHub::default();
    let (autonomous_tx, mut autonomous_rx) = mpsc::unbounded_channel();
    let autonomy = autonomy::start(
        storage.clone(),
        proactivity.clone(),
        autonomous_tx,
        observations.clone(),
        cancel.clone(),
    );
    let scheduler_cancel = cancel.clone();
    let scheduler = tokio::spawn(async move {
        let triage = TriageClient::new(storage.pool().clone());
        let mut deadlines = HashMap::<(String, String), Instant>::new();
        let mut ticker = interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = scheduler_cancel.cancelled() => return,
                Some(notice) = notices.recv() => {
                    if let Err(error) = proactivity.observe_room_message(&notice.room_id) {
                        eprintln!("failed to reset stalled-room declines: {error}");
                    }
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
                Some(wake) = autonomous_rx.recv() => {
                    let services = DispatchServices {
                        storage: &storage,
                        homes: &homes,
                        tokens: &tokens,
                        connections: &connections,
                        coordination: &coordination,
                        runtime: &runtime,
                        subscriptions: &subscriptions,
                        observations: &observations,
                    };
                    handle_autonomous(
                        &services,
                        &mut states,
                        &mut sessions,
                        &published_events,
                        &proactivity,
                        wake,
                    ).await;
                }
                Some(event) = event_rx.recv() => {
                    if let Some(session_id) = event.session_id()
                        && let Some(agent_id) = sessions.get(session_id)
                        && let Some(state) = states.get(agent_id)
                        && let Some(run_id) = state.active_run_id.as_deref()
                    {
                        record_engine_observation(
                            &observations, run_id, agent_id, state.room_id.as_deref(), &event,
                        );
                    }
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
                            observations.set_active_run(&agent_id, None);
                            if let Some(run_id) = run_id {
                                observations.record(OwnedObservation::linked(
                                    Some(&run_id),
                                    Some(&agent_id),
                                    states.get(&agent_id).and_then(|state| state.room_id.as_deref()),
                                    "prompt.interrupted",
                                    json!({"reason": "opencode_restarted"}),
                                ));
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
                                observations: &observations,
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
                            observations: &observations,
                        };
                        handle_idle(&services, &mut states, &mut sessions, &agent_id).await;
                    }
                    if event.event_type() == Some("session.error")
                        && let Some(session_id) = event.session_id()
                        && let Some(agent_id) = sessions.get(session_id)
                        && let Some(state) = states.get_mut(agent_id)
                        && let Some(run_id) = state.active_run_id.take()
                    {
                        observations.set_active_run(agent_id, None);
                        let message = event.payload.to_string();
                        observations.record(OwnedObservation::linked(
                            Some(&run_id),
                            Some(agent_id),
                            state.room_id.as_deref(),
                            "prompt.failed",
                            json!({"error": message}),
                        ));
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
                            observations: &observations,
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
        autonomy,
    })
}

async fn handle_autonomous(
    services: &DispatchServices<'_>,
    states: &mut HashMap<String, RuntimeState>,
    sessions: &mut HashMap<String, String>,
    events: &CollabEventPublisher,
    proactivity: &ProactivityHub,
    wake: AutonomousWake,
) {
    let stalled_claim = wake.stalled_claim;
    let claimed_room_id = wake.room_id.clone();
    let agent = match services.storage.agent(&wake.agent_id).await {
        Ok(Some(agent)) if agent.enabled => agent,
        Ok(_) => {
            cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
            return;
        }
        Err(error) => {
            eprintln!("failed to load autonomous Agent {}: {error}", wake.agent_id);
            cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
            return;
        }
    };
    let state = states
        .entry(agent.id.clone())
        .or_insert_with(|| RuntimeState::new(&agent));
    if state.wake.is_running() {
        autonomy::record_dispatch_short_circuit(
            services.storage,
            services.observations,
            &wake,
            TriageSource::LoopCap,
            "Agent already owns an active turn token",
        )
        .await;
        cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
        return;
    }
    match proactivity.acquire_autonomous_rate(&agent.id, std::time::Instant::now()) {
        Ok(true) => {}
        Ok(false) => {
            autonomy::record_dispatch_short_circuit(
                services.storage,
                services.observations,
                &wake,
                TriageSource::RateLimited,
                "Agent autonomous wake rate gate is cooling down",
            )
            .await;
            cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
            return;
        }
        Err(error) => {
            eprintln!("failed to acquire autonomous rate gate: {error}");
            cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
            return;
        }
    }
    let marker = match services
        .storage
        .insert_proactive_marker(&wake.room_id, &agent.id, wake.trigger, &wake.reason)
        .await
    {
        Ok(marker) => marker,
        Err(error) => {
            eprintln!("failed to write proactive room marker: {error}");
            cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
            return;
        }
    };
    events
        .publish(CollabEventKind::RoomsChanged {
            room_id: marker.room_id,
        })
        .await;
    state.room_id = Some(wake.room_id);
    state.prompt_note = Some(wake.prompt_note);
    state.proactive = Some(ProactivePrompt {
        reason: wake.reason,
        context: wake.context,
    });
    let action = state.wake.on_debounce_elapsed();
    if let Err(error) = dispatch(
        services,
        sessions,
        &agent.id,
        state,
        wake.trigger,
        action == WakeAction::Start,
    )
    .await
    {
        eprintln!(
            "failed to start {} wake for Agent {}: {error}",
            wake.trigger, agent.id
        );
        state.wake.on_dispatch_failed(action);
        cancel_stalled_wake(proactivity, stalled_claim, &claimed_room_id);
    } else if stalled_claim
        && let Err(error) =
            proactivity.finish_stall(&claimed_room_id, std::time::Instant::now(), true)
    {
        eprintln!("failed to finish stalled-room claim: {error}");
    }
}

fn cancel_stalled_wake(proactivity: &ProactivityHub, stalled_claim: bool, room_id: &str) {
    if stalled_claim && let Err(error) = proactivity.cancel_stall(room_id) {
        eprintln!("failed to cancel stalled-room claim: {error}");
    }
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
    let triage = wake_triage::evaluate(
        services.storage,
        triage,
        agent_id,
        room_id,
        &batch,
        services.observations,
    )
    .await;
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
        services.observations.set_active_run(agent_id, None);
        services.observations.record(OwnedObservation::linked(
            Some(&run_id),
            Some(agent_id),
            state.room_id.as_deref(),
            "prompt.completed",
            json!({"status": "completed"}),
        ));
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
    let prompt = build_wake_prompt(WakePromptParts {
        room_id,
        inbox: &inbox,
        glance: glance.as_ref(),
        roster: roster.as_deref(),
        memory: &memory,
        prompt_note: state.prompt_note.as_deref(),
        proactive: state.proactive.take(),
        trigger,
    });
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
    if let Some(run_id) = run_id.as_deref() {
        services.observations.record(OwnedObservation::linked(
            Some(run_id),
            Some(&agent.id),
            room_id,
            "prompt.started",
            json!({"trigger": trigger, "sessionId": session_id}),
        ));
    } else if let Some(run_id) = state.active_run_id.as_deref() {
        services.observations.record(OwnedObservation::linked(
            Some(run_id),
            Some(&agent.id),
            room_id,
            "prompt.injected",
            json!({"trigger": trigger, "sessionId": session_id}),
        ));
    }
    if let Err(error) = connection
        .client
        .prompt_async(&home, &session_id, &agent, &prompt)
        .await
    {
        if let Some(run_id) = &run_id {
            let message = error.to_string();
            services.observations.record(OwnedObservation::linked(
                Some(run_id),
                Some(&agent.id),
                room_id,
                "prompt.failed",
                json!({"error": message}),
            ));
            let _ = services
                .storage
                .finish_run(run_id, "failed", Some(("prompt_async_failed", &message)))
                .await;
        }
        return Err(error.to_string());
    }
    if let Some(run_id) = run_id {
        services
            .observations
            .set_active_run(&agent.id, Some(&run_id));
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

struct WakePromptParts<'a> {
    room_id: Option<&'a str>,
    inbox: &'a Inbox,
    glance: Option<&'a RoomGlance>,
    roster: Option<&'a [RoomMember]>,
    memory: &'a str,
    prompt_note: Option<&'a str>,
    proactive: Option<ProactivePrompt>,
    trigger: &'a str,
}

/// Assemble the JSON prompt injected with `prompt_async`. The roster carries
/// each member's `id` — the standing prompt (AGENTS.md) tells the Agent to
/// address teammates by that id, so the two must stay in sync.
fn build_wake_prompt(parts: WakePromptParts<'_>) -> String {
    let WakePromptParts {
        room_id,
        inbox,
        glance,
        roster,
        memory,
        prompt_note,
        proactive,
        trigger,
    } = parts;
    proactive.map_or_else(
        || {
            json!({
                "instruction": "Triage found this room update actionable for you. Read the published state, decide independently whether speaking still adds value, then use openwork_reply or openwork_react. Natural-language output without a successful tool call does not publish a response.",
                "roomId": room_id,
                "unread": inbox,
                "recentPublishedState": glance,
                "roster": roster,
                "memory": memory,
                "triagePromptNote": prompt_note,
                "delivery": "Unread is authoritative. This prompt and its seen coordination cursor do not clear or update last_read_seq."
            })
            .to_string()
        },
        |proactive| {
            json!({
                "instruction": "This is an explicitly autonomous collaboration turn. Execute the focused brief using the published state and collaboration tools. Do not spend the turn merely deciding whether work exists; the cheap agenda gate already did that. Natural-language output without a successful tool call does not publish a response.",
                "trigger": trigger,
                "reason": proactive.reason,
                "focusedContext": proactive.context,
                "roomId": room_id,
                "unread": inbox,
                "recentPublishedState": glance,
                "roster": roster,
                "memory": memory,
                "triagePromptNote": prompt_note,
                "delivery": "Unread is authoritative. Autonomous dispatch and seen coordination never update last_read_seq."
            })
            .to_string()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, display_name: &str) -> RoomMember {
        RoomMember {
            id: id.to_string(),
            display_name: display_name.to_string(),
            kind: "agent".to_string(),
            enabled: true,
        }
    }

    fn parts<'a>(roster: Option<&'a [RoomMember]>, inbox: &'a Inbox) -> WakePromptParts<'a> {
        WakePromptParts {
            room_id: Some("general"),
            inbox,
            glance: None,
            roster,
            memory: "# Memory",
            prompt_note: None,
            proactive: None,
            trigger: "room_message",
        }
    }

    #[test]
    fn wake_prompt_roster_carries_member_ids() {
        let roster = [
            member("user", "你"),
            member("alice", "小艾"),
            member("code_review", "Code Review"),
        ];
        let inbox = Inbox {
            messages: Vec::new(),
            unread_count: 0,
        };
        let prompt: serde_json::Value =
            serde_json::from_str(&build_wake_prompt(parts(Some(&roster), &inbox))).unwrap();
        let roster = prompt["roster"].as_array().unwrap();
        let ids: Vec<&str> = roster
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["user", "alice", "code_review"]);
        assert_eq!(roster[1]["displayName"].as_str().unwrap(), "小艾");
    }

    #[test]
    fn proactive_wake_prompt_keeps_the_roster_and_ids() {
        let roster = [member("bob", "Bob")];
        let inbox = Inbox {
            messages: Vec::new(),
            unread_count: 0,
        };
        let prompt: serde_json::Value = serde_json::from_str(&build_wake_prompt(WakePromptParts {
            proactive: Some(ProactivePrompt {
                reason: "stalled room".to_string(),
                context: serde_json::json!({"roomId": "general"}),
            }),
            ..parts(Some(&roster), &inbox)
        }))
        .unwrap();
        assert_eq!(prompt["roster"][0]["id"].as_str().unwrap(), "bob");
        assert_eq!(prompt["trigger"].as_str().unwrap(), "room_message");
    }
}
