use std::{
    collections::{HashMap, HashSet},
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
const MAX_INJECTIONS_PER_RUN: u8 = 4;
const MEMORY_MAX_CHARS: usize = 16_000;
pub type AgentTokens = Arc<RwLock<HashMap<String, String>>>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WakeState {
    running: bool,
    pending_rerun: bool,
    injections: u8,
}

impl WakeState {
    pub fn on_debounce_elapsed(&mut self) -> WakeAction {
        if self.running {
            self.pending_rerun = true;
            if self.injections < MAX_INJECTIONS_PER_RUN {
                self.injections += 1;
                WakeAction::Inject
            } else {
                WakeAction::Defer
            }
        } else {
            self.running = true;
            self.injections = 0;
            WakeAction::Start
        }
    }

    pub fn on_idle(&mut self) -> IdleAction {
        if self.pending_rerun {
            self.pending_rerun = false;
            self.injections = 0;
            IdleAction::Rerun
        } else {
            self.running = false;
            IdleAction::Stop
        }
    }

    pub fn reset(&mut self) {
        self.running = false;
        self.pending_rerun = false;
        self.injections = 0;
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
    Defer,
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
    deferred_room_id: Option<String>,
    deferred_prompt_note: Option<String>,
    proactive: Option<ProactivePrompt>,
    needs_full_context: bool,
    delivered_sequences: HashMap<String, i64>,
    unpublished_batch: Option<UnpublishedBatch>,
}

struct UnpublishedBatch {
    delivered_sequences: HashMap<String, i64>,
    attempts: u8,
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
            deferred_room_id: None,
            deferred_prompt_note: None,
            proactive: None,
            needs_full_context: true,
            delivered_sequences: HashMap::new(),
            unpublished_batch: None,
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
                        observations.observe_assistant_text(run_id, &event);
                    }
                    if event.event_type() == Some("session.compacted")
                        && let Some(session_id) = event.session_id()
                        && let Some(agent_id) = sessions.get(session_id)
                        && let Some(state) = states.get_mut(agent_id)
                    {
                        state.needs_full_context = true;
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
                                    None,
                                    Some(("opencode_restarted", "opencode serve restarted during the run")),
                                ).await;
                                observations.take_run_evidence(&run_id);
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
                        let _ = storage.finish_run(&run_id, "failed", None, Some(("opencode_session_error", &message))).await;
                        observations.take_run_evidence(&run_id);
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
    let authors = batch
        .iter()
        .map(|notice| notice.author_id.as_str())
        .collect::<HashSet<_>>();
    let active_teammates = active_teammates_for_room(states, agent_id, room_id, &authors);
    let triage = wake_triage::evaluate(
        services.storage,
        triage,
        agent_id,
        room_id,
        &batch,
        &active_teammates,
        services.observations,
    )
    .await;
    if !triage.actionable {
        return false;
    }
    let Some(state) = states.get_mut(agent_id) else {
        return false;
    };
    let action = state.wake.on_debounce_elapsed();
    if action == WakeAction::Defer
        || (action == WakeAction::Inject && state.room_id.as_deref() != Some(room_id))
    {
        state.deferred_room_id = Some(room_id.to_string());
        state.deferred_prompt_note = triage.prompt_note;
        return false;
    }
    state.room_id = Some(room_id.to_string());
    state.prompt_note = triage.prompt_note;
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

fn active_teammates_for_room(
    states: &HashMap<String, RuntimeState>,
    candidate_id: &str,
    room_id: &str,
    authors: &HashSet<&str>,
) -> HashSet<String> {
    states
        .iter()
        .filter(|(teammate_id, state)| {
            teammate_id.as_str() != candidate_id
                && state.active_run_id.is_some()
                && state.room_id.as_deref() == Some(room_id)
                && !authors.contains(teammate_id.as_str())
        })
        .map(|(teammate_id, _)| teammate_id.clone())
        .collect()
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
        let outcome = services.observations.take_run_evidence(&run_id).outcome();
        let delivered_sequences = std::mem::take(&mut state.delivered_sequences);
        settle_read_cursor(
            services,
            state,
            agent_id,
            &run_id,
            outcome,
            delivered_sequences,
        )
        .await;
        services.observations.record(OwnedObservation::linked(
            Some(&run_id),
            Some(agent_id),
            state.room_id.as_deref(),
            "prompt.completed",
            json!({"status": "completed", "outcome": outcome.as_str()}),
        ));
        if let Err(error) = services
            .storage
            .finish_run(&run_id, "completed", Some(outcome.as_str()), None)
            .await
        {
            eprintln!("failed to settle collaboration run {run_id}: {error}");
        }
    }
    if state.wake.on_idle() == IdleAction::Rerun {
        if let Some(room_id) = state.deferred_room_id.take() {
            state.room_id = Some(room_id);
            state.prompt_note = state.deferred_prompt_note.take();
        }
        if let Err(error) = dispatch(services, sessions, agent_id, state, "rerun", true).await {
            eprintln!("failed to rerun Agent {agent_id}: {error}");
            state.wake.reset();
        }
    }
}

async fn settle_read_cursor(
    services: &DispatchServices<'_>,
    state: &mut RuntimeState,
    agent_id: &str,
    run_id: &str,
    outcome: crate::model::RunOutcome,
    delivered_sequences: HashMap<String, i64>,
) {
    let settlement = cursor_settlement(state, outcome, &delivered_sequences);
    if let CursorSettlement::ForceAdvance { attempts } = settlement {
        services.observations.record(OwnedObservation::linked(
            Some(run_id),
            Some(agent_id),
            state.room_id.as_deref(),
            "inbox.force_advanced",
            json!({
                "attempts": attempts,
                "deliveredSequences": &delivered_sequences,
            }),
        ));
    }
    if settlement == CursorSettlement::Hold {
        return;
    }
    for (room_id, sequence) in delivered_sequences {
        if let Err(error) = services
            .storage
            .mark_read(&room_id, agent_id, sequence)
            .await
        {
            eprintln!(
                "failed to advance inbox cursor for Agent {agent_id} in room {room_id}: {error}"
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorSettlement {
    Advance,
    Hold,
    ForceAdvance { attempts: u8 },
}

fn cursor_settlement(
    state: &mut RuntimeState,
    outcome: crate::model::RunOutcome,
    delivered_sequences: &HashMap<String, i64>,
) -> CursorSettlement {
    match outcome {
        crate::model::RunOutcome::Acted | crate::model::RunOutcome::Silent => {
            state.unpublished_batch = None;
            CursorSettlement::Advance
        }
        crate::model::RunOutcome::Unpublished => {
            let attempts = state.unpublished_batch.as_ref().map_or(1, |previous| {
                if previous.delivered_sequences == *delivered_sequences {
                    previous.attempts.saturating_add(1)
                } else {
                    1
                }
            });
            if attempts >= 2 && !delivered_sequences.is_empty() {
                state.unpublished_batch = None;
                CursorSettlement::ForceAdvance { attempts }
            } else {
                state.unpublished_batch = Some(UnpublishedBatch {
                    delivered_sequences: delivered_sequences.clone(),
                    attempts,
                });
                CursorSettlement::Hold
            }
        }
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
    let room_id_owned = state.room_id.clone();
    let room_id = room_id_owned.as_deref();
    let full_context = state.needs_full_context;
    let glance = match (full_context, room_id) {
        (true, Some(room_id)) => {
            let glance = services
                .storage
                .glance(room_id, 50)
                .await
                .map_err(|error| error.to_string())?;
            Some(glance)
        }
        _ => None,
    };
    if let Some(room_id) = room_id {
        let delivered_highest = inbox
            .messages
            .iter()
            .filter(|message| message.room_id == room_id)
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0);
        let seen_highest = glance
            .as_ref()
            .map_or(delivered_highest, |glance| glance.highest_sequence);
        services
            .coordination
            .observe(agent_id, room_id, seen_highest)
            .await;
    }
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
    let memory = if full_context {
        Some(bounded_memory(
            &tokio::fs::read_to_string(home.join("memory/MEMORY.md"))
                .await
                .map_err(|error| error.to_string())?,
        ))
    } else {
        None
    };
    let delivered_sequences = delivered_sequences(&inbox);
    let prompt = build_wake_prompt(WakePromptParts {
        room_id,
        inbox: &inbox,
        glance: glance.as_ref(),
        roster: roster.as_deref(),
        memory: memory.as_deref(),
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
        services
            .observations
            .set_active_run(&agent.id, Some(run_id));
        state.active_run_id = Some(run_id.to_string());
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
                .finish_run(
                    run_id,
                    "failed",
                    None,
                    Some(("prompt_async_failed", &message)),
                )
                .await;
            services.observations.set_active_run(&agent.id, None);
            services.observations.take_run_evidence(run_id);
            state.active_run_id = None;
        }
        return Err(error.to_string());
    }
    if start_run {
        state.delivered_sequences = delivered_sequences;
    }
    if full_context {
        state.needs_full_context = false;
    }
    Ok(())
}

/// Highest sequence delivered to the engine this turn, per room.
///
/// Safe as a read-cursor target only because `CollabStorage::inbox` delivers
/// each room's OLDEST unread first: the delivered set is then a contiguous
/// prefix per room, so nothing below the maximum was skipped. If that ordering
/// is ever flipped, this function starts advancing the cursor past messages the
/// Agent never saw, and they become unreachable.
fn delivered_sequences(inbox: &Inbox) -> HashMap<String, i64> {
    let mut delivered = HashMap::<String, i64>::new();
    for message in &inbox.messages {
        delivered
            .entry(message.room_id.clone())
            .and_modify(|sequence| *sequence = (*sequence).max(message.sequence))
            .or_insert(message.sequence);
    }
    delivered
}

fn bounded_memory(memory: &str) -> String {
    let total_chars = memory.chars().count();
    if total_chars <= MEMORY_MAX_CHARS {
        return memory.to_string();
    }
    let visible: String = memory.chars().take(MEMORY_MAX_CHARS).collect();
    format!(
        "{visible}\n\n[MEMORY.md truncated: showing {MEMORY_MAX_CHARS} of {total_chars} characters]"
    )
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
    memory: Option<&'a str>,
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
    let (instruction, delivery) = if proactive.is_some() {
        (
            "This is an explicitly autonomous collaboration turn. Execute the focused brief using the published state and collaboration tools. Do not spend the turn merely deciding whether work exists; the cheap agenda gate already did that. Natural-language output without a successful tool call does not publish a response.",
            "Unread is authoritative. Autonomous dispatch and seen coordination never update last_read_seq.",
        )
    } else {
        (
            "Triage found this room update actionable for you. Read the published state, decide independently whether speaking still adds value, then use openwork_reply or openwork_react. Natural-language output without a successful tool call does not publish a response.",
            "Unread is authoritative. This prompt and its seen coordination cursor do not clear or update last_read_seq.",
        )
    };
    let mut prompt = serde_json::Map::new();
    prompt.insert("instruction".to_string(), json!(instruction));
    if let Some(proactive) = proactive {
        prompt.insert("trigger".to_string(), json!(trigger));
        prompt.insert("reason".to_string(), json!(proactive.reason));
        prompt.insert("focusedContext".to_string(), proactive.context);
    }
    prompt.insert("roomId".to_string(), json!(room_id));
    prompt.insert("unread".to_string(), prompt_inbox(inbox, glance));
    if let Some(notice) = inbox.omission_notice.as_deref() {
        prompt.insert("unreadOmissionNotice".to_string(), json!(notice));
    }
    prompt.insert("roster".to_string(), json!(roster));
    prompt.insert("triagePromptNote".to_string(), json!(prompt_note));
    prompt.insert("delivery".to_string(), json!(delivery));
    if glance.is_some() || memory.is_some() {
        prompt.insert("fullContext".to_string(), json!(true));
    }
    if let Some(glance) = glance {
        prompt.insert("recentPublishedState".to_string(), json!(glance));
    }
    if let Some(memory) = memory {
        prompt.insert("memory".to_string(), json!(memory));
    }
    serde_json::Value::Object(prompt).to_string()
}

fn prompt_inbox(inbox: &Inbox, glance: Option<&RoomGlance>) -> serde_json::Value {
    let messages = inbox
        .messages
        .iter()
        .map(|message| {
            if glance
                .is_some_and(|glance| glance.messages.iter().any(|recent| recent.id == message.id))
            {
                json!({
                    "id": message.id,
                    "roomId": message.room_id,
                    "sequence": message.sequence,
                    "includedInRecentPublishedState": true,
                })
            } else {
                json!(message)
            }
        })
        .collect::<Vec<_>>();
    json!({
        "messages": messages,
        "unreadCount": inbox.unread_count,
        "omittedCount": inbox.omitted_count,
        "omissionNotice": inbox.omission_notice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RunOutcome;

    fn member(id: &str, display_name: &str) -> RoomMember {
        RoomMember {
            id: id.to_string(),
            display_name: display_name.to_string(),
            kind: "agent".to_string(),
            enabled: true,
            muted: false,
        }
    }

    fn parts<'a>(roster: Option<&'a [RoomMember]>, inbox: &'a Inbox) -> WakePromptParts<'a> {
        WakePromptParts {
            room_id: Some("general"),
            inbox,
            glance: None,
            roster,
            memory: None,
            prompt_note: None,
            proactive: None,
            trigger: "room_message",
        }
    }

    fn message(id: &str, sequence: i64, body: &str) -> crate::model::Message {
        crate::model::Message {
            id: id.to_string(),
            room_id: "general".to_string(),
            sequence,
            author_id: "user".to_string(),
            kind: "normal".to_string(),
            body: body.to_string(),
            system_payload: None,
            created_at: "2026-08-20T10:00:00+08:00".to_string(),
        }
    }

    fn runtime(agent_id: &str, room_id: &str) -> RuntimeState {
        let agent = Agent {
            id: agent_id.to_string(),
            display_name: agent_id.to_string(),
            role: None,
            bio: None,
            system_prompt: String::new(),
            provider_id: "provider".to_string(),
            model_id: "model".to_string(),
            opencode_session_id: None,
            enabled: true,
            scanner_enabled: false,
        };
        let mut state = RuntimeState::new(&agent);
        state.room_id = Some(room_id.to_string());
        state.active_run_id = Some(format!("run_{agent_id}"));
        state
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
            omitted_count: 0,
            omission_notice: None,
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
            omitted_count: 0,
            omission_notice: None,
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

    #[test]
    fn regular_wake_is_incremental_and_carries_the_omission_notice_in_prompt_text() {
        let inbox = Inbox {
            messages: vec![message("msg_2", 2, "new")],
            unread_count: 12,
            omitted_count: 11,
            omission_notice: Some("另有 11 条更早未读已省略".to_string()),
        };
        let prompt = build_wake_prompt(parts(None, &inbox));
        let value: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert!(value.get("recentPublishedState").is_none());
        assert!(value.get("memory").is_none());
        assert_eq!(value["unreadOmissionNotice"], "另有 11 条更早未读已省略");
        assert!(prompt.contains("另有 11 条更早未读已省略"));
    }

    #[test]
    fn full_context_replaces_duplicate_unread_bodies_with_id_references() {
        let duplicate = message("msg_2", 2, "already in recent state");
        let inbox = Inbox {
            messages: vec![
                duplicate.clone(),
                message("msg_other", 1, "other room detail"),
            ],
            unread_count: 2,
            omitted_count: 0,
            omission_notice: None,
        };
        let glance = RoomGlance {
            room_id: "general".to_string(),
            highest_sequence: 2,
            messages: vec![duplicate],
        };
        let value: serde_json::Value = serde_json::from_str(&build_wake_prompt(WakePromptParts {
            glance: Some(&glance),
            memory: Some("# Memory"),
            ..parts(None, &inbox)
        }))
        .unwrap();
        assert_eq!(value["fullContext"], true);
        assert_eq!(
            value["unread"]["messages"][0]["includedInRecentPublishedState"],
            true
        );
        assert!(value["unread"]["messages"][0].get("body").is_none());
        assert_eq!(
            value["recentPublishedState"]["messages"][0]["body"],
            "already in recent state"
        );
    }

    #[test]
    fn memory_truncation_is_unicode_safe_and_explicit() {
        let memory = "中文🧠".repeat(MEMORY_MAX_CHARS);
        let bounded = bounded_memory(&memory);
        assert_eq!(
            bounded
                .split("\n\n[MEMORY.md truncated:")
                .next()
                .unwrap()
                .chars()
                .count(),
            MEMORY_MAX_CHARS
        );
        assert!(bounded.contains("[MEMORY.md truncated:"));
    }

    #[test]
    fn triage_active_set_is_room_scoped_and_excludes_candidate_and_authors() {
        let states = HashMap::from([
            ("alice".to_string(), runtime("alice", "general")),
            ("bob".to_string(), runtime("bob", "other")),
            ("carol".to_string(), runtime("carol", "general")),
            ("dave".to_string(), runtime("dave", "general")),
        ]);
        let authors = HashSet::from(["carol"]);
        assert_eq!(
            active_teammates_for_room(&states, "dave", "general", &authors),
            HashSet::from(["alice".to_string()])
        );
    }

    #[test]
    fn cursor_settlement_holds_then_forces_the_same_unpublished_batch() {
        let mut state = runtime("alice", "general");
        let first_batch = HashMap::from([("general".to_string(), 12)]);

        assert_eq!(
            cursor_settlement(&mut state, RunOutcome::Unpublished, &first_batch),
            CursorSettlement::Hold
        );
        assert_eq!(
            cursor_settlement(&mut state, RunOutcome::Unpublished, &first_batch),
            CursorSettlement::ForceAdvance { attempts: 2 }
        );
        assert!(state.unpublished_batch.is_none());
    }

    #[test]
    fn cursor_settlement_resets_for_a_different_batch_and_for_success() {
        let mut state = runtime("alice", "general");
        let first_batch = HashMap::from([("general".to_string(), 12)]);
        let next_batch = HashMap::from([("general".to_string(), 13)]);

        assert_eq!(
            cursor_settlement(&mut state, RunOutcome::Unpublished, &first_batch),
            CursorSettlement::Hold
        );
        assert_eq!(
            cursor_settlement(&mut state, RunOutcome::Unpublished, &next_batch),
            CursorSettlement::Hold
        );
        assert_eq!(
            cursor_settlement(&mut state, RunOutcome::Silent, &next_batch),
            CursorSettlement::Advance
        );
        assert!(state.unpublished_batch.is_none());
    }
}
