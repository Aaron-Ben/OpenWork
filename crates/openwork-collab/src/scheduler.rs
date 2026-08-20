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

fn arm_debounce(deadlines: &mut HashMap<String, Instant>, agent_id: &str, now: Instant) {
    deadlines
        .entry(agent_id.to_string())
        .or_insert(now + DEBOUNCE);
}

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
    proactive: Option<ProactivePrompt>,
    needs_full_context: bool,
    /// Highest sequence handed to the engine this run, per room. Consumed once
    /// at settlement; see `settle_read_cursor`.
    delivered_sequences: HashMap<String, i64>,
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
            needs_full_context: true,
            delivered_sequences: HashMap::new(),
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
        // Keyed by Agent, not by (Agent, room): one wake covers the whole inbox.
        // The window is armed by the FIRST pending message and is NOT extended by
        // later ones — otherwise a steadily busy room would push the deadline
        // forward forever and the Agent would never wake at all.
        let mut deadlines = HashMap::<String, Instant>::new();
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
                                arm_debounce(&mut deadlines, &agent.id, Instant::now());
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
                    for agent_id in due {
                        deadlines.remove(&agent_id);
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
                        ).await;
                        if retry {
                            deadlines.insert(agent_id, Instant::now() + DEBOUNCE);
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

/// One wake per Agent, covering every room with pending messages.
///
/// Triage judges the whole inbox at once rather than one room at a time: an
/// Agent has a single engine session, so a per-room wake still hands it the
/// cross-room inbox, and settling that inbox per wake is what keeps the read
/// cursor honest (see `settle_read_cursor`).
async fn handle_due(
    services: &DispatchServices<'_>,
    states: &mut HashMap<String, RuntimeState>,
    sessions: &mut HashMap<String, String>,
    triage: &TriageClient,
    agent_id: &str,
) -> bool {
    let Some(batches) = states.get_mut(agent_id).map(|state| {
        state
            .pending
            .drain()
            .filter(|(_, notices)| !notices.is_empty())
            .collect::<HashMap<String, Vec<MessageNotice>>>()
    }) else {
        return false;
    };
    if batches.is_empty() {
        return false;
    }
    let active_teammates = active_teammates(states, agent_id, &batches);
    let triage = wake_triage::evaluate(
        services.storage,
        triage,
        agent_id,
        &batches,
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
    if action == WakeAction::Defer {
        // Injection budget spent. The messages stay unread, and the pending
        // rerun this already armed picks them up from the inbox next turn.
        return false;
    }
    // An inbox wake has no single focused room; the digest carries them all.
    state.room_id = None;
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

/// Teammates already running a turn that covers one of the rooms in this batch.
///
/// Room attribution comes from what each teammate was actually handed this run
/// (`delivered_sequences`), plus the focused room of an autonomous wake — a
/// teammate busy with an unrelated room must not suppress this one. Message
/// authors are excluded: they are awake, but they are the ones speaking.
///
/// The result is an unordered set per room. It never expresses a turn order, so
/// it cannot be read as "I am third in line" (see collaboration.md 9.1).
fn active_teammates(
    states: &HashMap<String, RuntimeState>,
    candidate_id: &str,
    batches: &HashMap<String, Vec<MessageNotice>>,
) -> HashMap<String, HashSet<String>> {
    let mut per_room = HashMap::<String, HashSet<String>>::new();
    for (room_id, notices) in batches {
        let authors = notices
            .iter()
            .map(|notice| notice.author_id.as_str())
            .collect::<HashSet<_>>();
        let busy = states
            .iter()
            .filter(|(teammate_id, state)| {
                teammate_id.as_str() != candidate_id
                    && state.active_run_id.is_some()
                    && !authors.contains(teammate_id.as_str())
                    && (state.delivered_sequences.contains_key(room_id)
                        || state.room_id.as_deref() == Some(room_id.as_str()))
            })
            .map(|(teammate_id, _)| teammate_id.clone())
            .collect::<HashSet<_>>();
        if !busy.is_empty() {
            per_room.insert(room_id.clone(), busy);
        }
    }
    per_room
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
        let evidence = services.observations.take_run_evidence(&run_id);
        let outcome = evidence.outcome();
        let delivered_sequences = std::mem::take(&mut state.delivered_sequences);
        settle_read_cursor(
            services.storage,
            services.observations,
            agent_id,
            &run_id,
            &evidence,
            &delivered_sequences,
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
        state.room_id = None;
        if let Err(error) = dispatch(services, sessions, agent_id, state, "rerun", true).await {
            eprintln!("failed to rerun Agent {agent_id}: {error}");
            state.wake.reset();
        }
    }
}

/// Advance the read cursor for the rooms this run actually finished with.
///
/// A room settles only when the Agent published into it (`reply` / `react` /
/// `card`) or explicitly stood down (`ack`). Everything else it was merely
/// shown stays unread and is redelivered next turn.
///
/// The alternative — advancing every room that appeared in the digest — reads
/// "delivered" as "handled" and silently drops rooms the turn never addressed:
/// a run focused on room A would mark room B read, and B's own follow-up wake
/// would then arrive with an empty inbox and produce nothing at all.
///
/// A room that is never settled simply stays unread. That does not spin: wakes
/// are driven by message arrival, so an unanswered room costs nothing until
/// something new is actually said in it.
async fn settle_read_cursor(
    storage: &CollabStorage,
    observations: &ObservationSink,
    agent_id: &str,
    run_id: &str,
    evidence: &crate::observation::RunEvidence,
    delivered_sequences: &HashMap<String, i64>,
) {
    if delivered_sequences.is_empty() {
        return;
    }
    let settled = evidence.settled_rooms().collect::<HashSet<_>>();
    let mut advanced = Vec::new();
    let mut carried = Vec::new();
    for (room_id, sequence) in delivered_sequences {
        if !settled.contains(room_id.as_str()) {
            carried.push(room_id.clone());
            continue;
        }
        match storage.mark_read(room_id, agent_id, *sequence).await {
            Ok(_) => advanced.push(room_id.clone()),
            Err(error) => eprintln!(
                "failed to advance inbox cursor for Agent {agent_id} in room {room_id}: {error}"
            ),
        }
    }
    observations.record(OwnedObservation::linked(
        Some(run_id),
        Some(agent_id),
        None,
        "inbox.settled",
        json!({
            "advanced": advanced,
            "carriedOver": carried,
            "ackedRooms": evidence.acked_rooms(),
        }),
    ));
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
    let focused_room_id = state.room_id.clone();
    let full_context = state.needs_full_context;

    // Every room with something to deliver, plus the focused room of an
    // autonomous wake even when it has no unread of its own.
    let mut digest_room_ids = inbox
        .messages
        .iter()
        .map(|message| message.room_id.clone())
        .collect::<Vec<_>>();
    if let Some(focused) = focused_room_id.as_deref() {
        digest_room_ids.push(focused.to_string());
    }
    digest_room_ids.sort();
    digest_room_ids.dedup();

    let summaries = services
        .storage
        .room_summaries(agent_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut rooms = Vec::with_capacity(digest_room_ids.len());
    for room_id in &digest_room_ids {
        let summary = summaries.iter().find(|room| &room.id == room_id);
        let glance = if full_context {
            Some(
                services
                    .storage
                    .glance(room_id, 50)
                    .await
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        // The seen cursor is what HELD compares against, so it must reflect the
        // highest sequence this room actually showed the Agent this turn.
        let delivered_highest = inbox
            .messages
            .iter()
            .filter(|message| &message.room_id == room_id)
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
        rooms.push(WakeRoom {
            room_id: room_id.clone(),
            kind: summary.map(|room| room.kind.clone()),
            roster: summary.map(|room| room.members.clone()),
            glance,
        });
    }

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
        focused_room_id: focused_room_id.as_deref(),
        inbox: &inbox,
        rooms: &rooms,
        memory: memory.as_deref(),
        prompt_note: state.prompt_note.as_deref(),
        proactive: state.proactive.take(),
        trigger,
    });
    let run_id = if start_run {
        Some(
            services
                .storage
                .begin_run(&agent, focused_room_id.as_deref(), trigger)
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
            focused_room_id.as_deref(),
            "prompt.started",
            json!({"trigger": trigger, "sessionId": session_id}),
        ));
    } else if let Some(run_id) = state.active_run_id.as_deref() {
        services.observations.record(OwnedObservation::linked(
            Some(run_id),
            Some(&agent.id),
            focused_room_id.as_deref(),
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
                focused_room_id.as_deref(),
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

/// One room's slice of a wake digest.
struct WakeRoom {
    room_id: String,
    kind: Option<String>,
    roster: Option<Vec<RoomMember>>,
    glance: Option<RoomGlance>,
}

struct WakePromptParts<'a> {
    /// Set only for an autonomous wake, which really is about one room. An
    /// inbox wake has no focus — every room in the digest is equally real.
    focused_room_id: Option<&'a str>,
    inbox: &'a Inbox,
    rooms: &'a [WakeRoom],
    memory: Option<&'a str>,
    prompt_note: Option<&'a str>,
    proactive: Option<ProactivePrompt>,
    trigger: &'a str,
}

/// Assemble the JSON prompt injected with `prompt_async`.
///
/// The digest is grouped by room, and each room carries its own roster, because
/// an Agent has one engine session serving every room it belongs to: without
/// the grouping it would have to infer from a flat list which room a message
/// came from and which room to answer in.
///
/// Each roster carries its members' `id`s — the standing prompt tells the Agent
/// to address teammates by that id, so the two must stay in sync.
fn build_wake_prompt(parts: WakePromptParts<'_>) -> String {
    let WakePromptParts {
        focused_room_id,
        inbox,
        rooms,
        memory,
        prompt_note,
        proactive,
        trigger,
    } = parts;
    let (instruction, delivery) = if proactive.is_some() {
        (
            "This is an explicitly autonomous collaboration turn. Execute the focused brief using the published state and collaboration tools. Do not spend the turn merely deciding whether work exists; the cheap agenda gate already did that. Natural-language output without a successful tool call does not publish a response.",
            "Unread is authoritative. Autonomous dispatch and seen coordination never update last_read_seq. Close out every room you were shown: openwork_reply / openwork_react / openwork_card settle a room, and openwork_ack settles one you are deliberately not answering. A room you leave unsettled stays unread and comes back.",
        )
    } else {
        (
            "Triage found your inbox actionable. It may cover more than one room: each entry under rooms carries its own id, roster, and unread messages. Reply into a specific room with openwork_reply(room_id=...), and use openwork_ack(room_id=...) for a room you have read but are deliberately not answering. Natural-language output without a successful tool call does not publish a response.",
            "Unread is authoritative. This prompt and its seen coordination cursor do not clear or update last_read_seq. Close out every room you were shown: openwork_reply / openwork_react / openwork_card settle a room, and openwork_ack settles one you are deliberately not answering. A room you leave unsettled stays unread and comes back.",
        )
    };
    let mut prompt = serde_json::Map::new();
    prompt.insert("instruction".to_string(), json!(instruction));
    if let Some(proactive) = proactive {
        prompt.insert("trigger".to_string(), json!(trigger));
        prompt.insert("reason".to_string(), json!(proactive.reason));
        prompt.insert("focusedContext".to_string(), proactive.context);
    }
    if let Some(focused_room_id) = focused_room_id {
        prompt.insert("focusedRoomId".to_string(), json!(focused_room_id));
    }
    prompt.insert("rooms".to_string(), json!(prompt_rooms(inbox, rooms)));
    prompt.insert("unreadCount".to_string(), json!(inbox.unread_count));
    if let Some(notice) = inbox.omission_notice.as_deref() {
        prompt.insert("unreadOmissionNotice".to_string(), json!(notice));
        prompt.insert("omittedCount".to_string(), json!(inbox.omitted_count));
    }
    prompt.insert("triagePromptNote".to_string(), json!(prompt_note));
    prompt.insert("delivery".to_string(), json!(delivery));
    let full_context = memory.is_some() || rooms.iter().any(|room| room.glance.is_some());
    if full_context {
        prompt.insert("fullContext".to_string(), json!(true));
    }
    if let Some(memory) = memory {
        prompt.insert("memory".to_string(), json!(memory));
    }
    serde_json::Value::Object(prompt).to_string()
}

fn prompt_rooms(inbox: &Inbox, rooms: &[WakeRoom]) -> Vec<serde_json::Value> {
    rooms
        .iter()
        .map(|room| {
            let unread = inbox
                .messages
                .iter()
                .filter(|message| message.room_id == room.room_id)
                .map(|message| {
                    // A message already spelled out under recentPublishedState is
                    // referenced by id instead of repeated: the same body twice in
                    // one prompt is pure token cost.
                    if room.glance.as_ref().is_some_and(|glance| {
                        glance.messages.iter().any(|recent| recent.id == message.id)
                    }) {
                        json!({
                            "id": message.id,
                            "sequence": message.sequence,
                            "includedInRecentPublishedState": true,
                        })
                    } else {
                        json!(message)
                    }
                })
                .collect::<Vec<_>>();
            let mut entry = serde_json::Map::new();
            entry.insert("roomId".to_string(), json!(room.room_id));
            entry.insert("kind".to_string(), json!(room.kind));
            entry.insert("roster".to_string(), json!(room.roster));
            entry.insert("unread".to_string(), json!(unread));
            if let Some(glance) = room.glance.as_ref() {
                entry.insert("recentPublishedState".to_string(), json!(glance));
            }
            serde_json::Value::Object(entry)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::{
        Executor, PgPool,
        postgres::{PgConnectOptions, PgPoolOptions},
    };
    use uuid::Uuid;

    use super::*;
    use crate::model::AgentInput;

    fn member(id: &str, display_name: &str) -> RoomMember {
        RoomMember {
            id: id.to_string(),
            display_name: display_name.to_string(),
            kind: "agent".to_string(),
            enabled: true,
            muted: false,
        }
    }

    fn room(room_id: &str, roster: Vec<RoomMember>, glance: Option<RoomGlance>) -> WakeRoom {
        WakeRoom {
            room_id: room_id.to_string(),
            kind: Some("group".to_string()),
            roster: Some(roster),
            glance,
        }
    }

    fn parts<'a>(rooms: &'a [WakeRoom], inbox: &'a Inbox) -> WakePromptParts<'a> {
        WakePromptParts {
            focused_room_id: None,
            inbox,
            rooms,
            memory: None,
            prompt_note: None,
            proactive: None,
            trigger: "room_message",
        }
    }

    fn message(id: &str, sequence: i64, body: &str) -> crate::model::Message {
        in_room(id, "general", sequence, body)
    }

    fn in_room(id: &str, room_id: &str, sequence: i64, body: &str) -> crate::model::Message {
        crate::model::Message {
            id: id.to_string(),
            room_id: room_id.to_string(),
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

    fn empty_inbox() -> Inbox {
        Inbox {
            messages: Vec::new(),
            unread_count: 0,
            omitted_count: 0,
            omission_notice: None,
        }
    }

    async fn postgres_storage(prefix: &str) -> Option<(PgPool, PgPool, String, CollabStorage)> {
        let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
        let admin = PgPool::connect(&database_url).await.unwrap();
        let schema = format!("{prefix}_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&database_url)
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        let storage = CollabStorage::from_pool(pool.clone());
        storage.migrate().await.unwrap();
        Some((admin, pool, schema, storage))
    }

    async fn drop_postgres_schema(admin: PgPool, pool: PgPool, schema: String) {
        pool.close().await;
        admin
            .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
            .await
            .unwrap();
    }

    async fn two_room_inbox(storage: &CollabStorage) -> HashMap<String, i64> {
        storage
            .create_agent(&AgentInput {
                id: Some("alice".to_string()),
                display_name: "Alice".to_string(),
                role: None,
                bio: None,
                system_prompt: "Handle collaboration rooms".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "main".to_string(),
                enabled: true,
                scanner_enabled: false,
            })
            .await
            .unwrap();
        for room_id in ["alpha", "beta"] {
            storage
                .create_group_room(Some(room_id), room_id)
                .await
                .unwrap();
            storage.add_member(room_id, "user").await.unwrap();
            storage.add_member(room_id, "alice").await.unwrap();
            storage
                .send_message(room_id, "user", &format!("{room_id} first"))
                .await
                .unwrap();
            storage
                .send_message(room_id, "user", &format!("{room_id} second"))
                .await
                .unwrap();
        }

        let inbox = storage.inbox("alice").await.unwrap();
        let mut delivered_sequences = HashMap::new();
        for message in inbox.messages {
            delivered_sequences
                .entry(message.room_id)
                .and_modify(|sequence: &mut i64| *sequence = (*sequence).max(message.sequence))
                .or_insert(message.sequence);
        }
        delivered_sequences
    }

    async fn read_cursors(storage: &CollabStorage) -> HashMap<String, i64> {
        storage
            .room_summaries("alice")
            .await
            .unwrap()
            .into_iter()
            .map(|room| (room.id, room.last_read_sequence))
            .collect()
    }

    #[tokio::test]
    async fn settling_a_reply_in_alpha_keeps_beta_unread_for_the_next_wake() {
        let Some((admin, pool, schema, storage)) =
            postgres_storage("collab_settle_reply_test").await
        else {
            return;
        };
        let delivered_sequences = two_room_inbox(&storage).await;
        let before = read_cursors(&storage).await;
        let first_inbox = storage.inbox("alice").await.unwrap();
        let beta_messages = first_inbox
            .messages
            .iter()
            .filter(|message| message.room_id == "beta")
            .map(|message| (message.sequence, message.body.clone()))
            .collect::<Vec<_>>();

        let observations = ObservationSink::discarding();
        observations.set_active_run("alice", Some("run_reply_alpha"));
        observations.mark_action("alice", "alpha");
        let evidence = observations.take_run_evidence("run_reply_alpha");
        settle_read_cursor(
            &storage,
            &observations,
            "alice",
            "run_reply_alpha",
            &evidence,
            &delivered_sequences,
        )
        .await;

        let after = read_cursors(&storage).await;
        assert_eq!(after["alpha"], delivered_sequences["alpha"]);
        assert_eq!(after["beta"], before["beta"]);
        let next_inbox = storage.inbox("alice").await.unwrap();
        assert_eq!(
            next_inbox
                .messages
                .iter()
                .filter(|message| message.room_id == "beta")
                .map(|message| (message.sequence, message.body.clone()))
                .collect::<Vec<_>>(),
            beta_messages
        );
        assert!(
            next_inbox
                .messages
                .iter()
                .all(|message| message.room_id != "alpha")
        );

        drop_postgres_schema(admin, pool, schema).await;
    }

    #[tokio::test]
    async fn acknowledging_beta_settles_it_without_turning_the_run_into_action() {
        let Some((admin, pool, schema, storage)) = postgres_storage("collab_settle_ack_test").await
        else {
            return;
        };
        let delivered_sequences = two_room_inbox(&storage).await;
        let before = read_cursors(&storage).await;
        let observations = ObservationSink::discarding();
        observations.set_active_run("alice", Some("run_ack_beta"));
        observations.mark_ack("alice", "beta");
        let evidence = observations.take_run_evidence("run_ack_beta");

        assert_eq!(evidence.outcome(), crate::model::RunOutcome::Silent);
        settle_read_cursor(
            &storage,
            &observations,
            "alice",
            "run_ack_beta",
            &evidence,
            &delivered_sequences,
        )
        .await;

        let after = read_cursors(&storage).await;
        assert_eq!(after["alpha"], before["alpha"]);
        assert_eq!(after["beta"], delivered_sequences["beta"]);

        drop_postgres_schema(admin, pool, schema).await;
    }

    #[test]
    fn each_room_carries_its_own_roster_of_member_ids() {
        let rooms = [room(
            "general",
            vec![
                member("user", "你"),
                member("alice", "小艾"),
                member("code_review", "Code Review"),
            ],
            None,
        )];
        let inbox = empty_inbox();
        let prompt: serde_json::Value =
            serde_json::from_str(&build_wake_prompt(parts(&rooms, &inbox))).unwrap();
        let roster = prompt["rooms"][0]["roster"].as_array().unwrap();
        let ids: Vec<&str> = roster
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["user", "alice", "code_review"]);
        assert_eq!(roster[1]["displayName"].as_str().unwrap(), "小艾");
    }

    /// The whole point of grouping: an Agent in two rooms must be able to tell
    /// which message belongs where, and which roster applies to each.
    #[test]
    fn two_rooms_stay_separated_with_their_own_messages_and_rosters() {
        let rooms = [
            room("general", vec![member("alice", "小艾")], None),
            room("design", vec![member("bob", "Bob")], None),
        ];
        let inbox = Inbox {
            messages: vec![
                message("msg_1", 4, "general question"),
                in_room("msg_2", "design", 9, "design question"),
            ],
            unread_count: 2,
            omitted_count: 0,
            omission_notice: None,
        };
        let prompt: serde_json::Value =
            serde_json::from_str(&build_wake_prompt(parts(&rooms, &inbox))).unwrap();

        assert_eq!(prompt["rooms"][0]["roomId"], "general");
        assert_eq!(prompt["rooms"][0]["unread"].as_array().unwrap().len(), 1);
        assert_eq!(prompt["rooms"][0]["unread"][0]["body"], "general question");
        assert_eq!(prompt["rooms"][0]["roster"][0]["id"], "alice");

        assert_eq!(prompt["rooms"][1]["roomId"], "design");
        assert_eq!(prompt["rooms"][1]["unread"].as_array().unwrap().len(), 1);
        assert_eq!(prompt["rooms"][1]["unread"][0]["body"], "design question");
        assert_eq!(prompt["rooms"][1]["roster"][0]["id"], "bob");

        // No single focused room, and the instruction must name the settle tools.
        assert!(prompt.get("focusedRoomId").is_none());
        let instruction = prompt["instruction"].as_str().unwrap();
        assert!(instruction.contains("openwork_ack"));
        assert!(instruction.contains("room_id"));
    }

    #[test]
    fn proactive_wake_names_its_focused_room_and_keeps_the_roster() {
        let rooms = [room("general", vec![member("bob", "Bob")], None)];
        let inbox = empty_inbox();
        let prompt: serde_json::Value = serde_json::from_str(&build_wake_prompt(WakePromptParts {
            focused_room_id: Some("general"),
            proactive: Some(ProactivePrompt {
                reason: "stalled room".to_string(),
                context: serde_json::json!({"roomId": "general"}),
            }),
            ..parts(&rooms, &inbox)
        }))
        .unwrap();
        assert_eq!(prompt["focusedRoomId"], "general");
        assert_eq!(
            prompt["rooms"][0]["roster"][0]["id"].as_str().unwrap(),
            "bob"
        );
        assert_eq!(prompt["trigger"].as_str().unwrap(), "room_message");
    }

    #[test]
    fn regular_wake_is_incremental_and_carries_the_omission_notice_in_prompt_text() {
        let rooms = [room("general", vec![member("alice", "小艾")], None)];
        let inbox = Inbox {
            messages: vec![message("msg_2", 2, "new")],
            unread_count: 12,
            omitted_count: 11,
            omission_notice: Some("11 newer unread".to_string()),
        };
        let prompt = build_wake_prompt(parts(&rooms, &inbox));
        let value: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert!(value["rooms"][0].get("recentPublishedState").is_none());
        assert!(value.get("memory").is_none());
        assert!(value.get("fullContext").is_none());
        assert_eq!(value["unreadOmissionNotice"], "11 newer unread");
        assert!(prompt.contains("11 newer unread"));
    }

    #[test]
    fn full_context_replaces_duplicate_unread_bodies_with_id_references() {
        let duplicate = message("msg_2", 2, "already in recent state");
        let inbox = Inbox {
            messages: vec![duplicate.clone()],
            unread_count: 1,
            omitted_count: 0,
            omission_notice: None,
        };
        let rooms = [room(
            "general",
            vec![member("alice", "小艾")],
            Some(RoomGlance {
                room_id: "general".to_string(),
                highest_sequence: 2,
                messages: vec![duplicate],
            }),
        )];
        let value: serde_json::Value = serde_json::from_str(&build_wake_prompt(WakePromptParts {
            memory: Some("# Memory"),
            ..parts(&rooms, &inbox)
        }))
        .unwrap();
        assert_eq!(value["fullContext"], true);
        assert_eq!(
            value["rooms"][0]["unread"][0]["includedInRecentPublishedState"],
            true
        );
        assert!(value["rooms"][0]["unread"][0].get("body").is_none());
        assert_eq!(
            value["rooms"][0]["recentPublishedState"]["messages"][0]["body"],
            "already in recent state"
        );
    }

    fn notice(room_id: &str, author_id: &str, sequence: i64) -> MessageNotice {
        MessageNotice {
            room_id: room_id.to_string(),
            author_id: author_id.to_string(),
            body: "hi".to_string(),
            sequence,
        }
    }

    #[test]
    fn triage_active_set_is_room_scoped_and_excludes_candidate_and_authors() {
        let states = HashMap::from([
            // alice is mid-run on general, so she counts for general.
            ("alice".to_string(), runtime("alice", "general")),
            // bob is busy, but on another room: he must not suppress general.
            ("bob".to_string(), runtime("bob", "other")),
            // carol is busy on general but wrote the message that woke this batch.
            ("carol".to_string(), runtime("carol", "general")),
            ("dave".to_string(), runtime("dave", "general")),
        ]);
        let batches = HashMap::from([("general".to_string(), vec![notice("general", "carol", 4)])]);

        let per_room = active_teammates(&states, "dave", &batches);
        assert_eq!(
            per_room.get("general"),
            Some(&HashSet::from(["alice".to_string()]))
        );
        assert!(!per_room.contains_key("other"));
    }

    #[test]
    fn a_room_with_no_busy_teammate_carries_no_entry_at_all() {
        let states = HashMap::from([("bob".to_string(), runtime("bob", "other"))]);
        let batches = HashMap::from([("general".to_string(), vec![notice("general", "user", 1)])]);
        assert!(active_teammates(&states, "dave", &batches).is_empty());
    }

    #[test]
    fn later_messages_do_not_extend_an_agents_debounce_window() {
        let mut deadlines = HashMap::new();
        let first = Instant::now();

        arm_debounce(&mut deadlines, "alice", first);
        arm_debounce(&mut deadlines, "alice", first + Duration::from_millis(400));
        arm_debounce(&mut deadlines, "alice", first + Duration::from_millis(900));

        assert_eq!(deadlines["alice"], first + DEBOUNCE);
    }

    #[test]
    fn debounce_windows_are_independent_per_agent() {
        let mut deadlines = HashMap::new();
        let alice_now = Instant::now();
        let bob_now = alice_now + Duration::from_millis(600);

        arm_debounce(&mut deadlines, "alice", alice_now);
        arm_debounce(&mut deadlines, "bob", bob_now);

        assert_eq!(deadlines["alice"], alice_now + DEBOUNCE);
        assert_eq!(deadlines["bob"], bob_now + DEBOUNCE);
    }

    #[test]
    fn removed_debounce_window_rearms_from_the_new_message() {
        let mut deadlines = HashMap::new();
        let first = Instant::now();
        arm_debounce(&mut deadlines, "alice", first);
        assert_eq!(deadlines.remove("alice"), Some(first + DEBOUNCE));

        let next = first + Duration::from_secs(5);
        arm_debounce(&mut deadlines, "alice", next);

        assert_eq!(deadlines["alice"], next + DEBOUNCE);
    }
}
