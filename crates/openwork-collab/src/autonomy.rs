//! Periodic idle, agenda, and scanner work that runs before main-Agent dispatch.

use std::time::{Duration, Instant};

use serde_json::Value;
use thiserror::Error;
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{MissedTickBehavior, interval_at},
};
use tokio_util::sync::CancellationToken;

use crate::{
    model::{AgendaCandidate, Agent, TriageRecordInput},
    observation::{ObservationSink, record_triage},
    proactivity::{
        NudgeClaim, ProactivityError, ProactivityHub, ScannerDecision, scanner_fingerprint,
    },
    storage::{CollabStorage, StorageError},
    triage::{AgendaTriageContext, TriageClient, TriageSource, resolve_agenda_failure},
};

pub const AGENDA_CHECK: Duration = Duration::from_secs(60);
pub const AGENDA_QUIET: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq)]
pub struct AutonomousWake {
    pub agent_id: String,
    pub room_id: String,
    pub trigger: &'static str,
    pub reason: String,
    pub prompt_note: String,
    pub context: Value,
    pub up_to_sequence: i64,
    pub stalled_claim: bool,
}

#[derive(Clone)]
pub struct AutonomyEngine {
    storage: CollabStorage,
    triage: TriageClient,
    state: ProactivityHub,
    observations: ObservationSink,
}

impl AutonomyEngine {
    pub fn new(
        storage: CollabStorage,
        state: ProactivityHub,
        observations: ObservationSink,
    ) -> Self {
        Self {
            triage: TriageClient::new(storage.pool().clone()),
            storage,
            state,
            observations,
        }
    }

    pub async fn sweep_agenda(&self) -> Result<Option<AutonomousWake>, AutonomyError> {
        let mut available = Vec::new();
        for agent in self.storage.agents().await? {
            if agent.enabled && self.storage.agent_is_quiet(&agent.id, AGENDA_QUIET).await? {
                available.push(agent.id);
            }
        }
        let Some(agent_id) = self.state.choose_idle(&available)? else {
            return Ok(None);
        };
        let Some(agent) = self.storage.agent(&agent_id).await? else {
            return Ok(None);
        };
        let candidates = self.storage.agenda_candidates(&agent.id).await?;
        if candidates.is_empty() {
            self.record_empty_agenda(&agent).await?;
            return Ok(None);
        }
        for candidate in candidates {
            if let Some(wake) = self.evaluate_agenda_candidate(&agent, candidate).await? {
                return Ok(Some(wake));
            }
        }
        Ok(None)
    }

    pub async fn sweep_scanner(&self) -> Result<Vec<AutonomousWake>, AutonomyError> {
        let mut wakes = Vec::new();
        for agent in self.storage.scanner_agents().await? {
            if !self.storage.agent_is_quiet(&agent.id, AGENDA_QUIET).await? {
                continue;
            }
            let snapshot = self.storage.scanner_snapshot(&agent.id).await?;
            if snapshot.rooms.is_empty() {
                continue;
            }
            let rooms = snapshot
                .rooms
                .iter()
                .map(|room| (room.room_id.clone(), room.highest_sequence))
                .collect::<Vec<_>>();
            let fingerprint = scanner_fingerprint(&rooms);
            if self
                .state
                .observe_scanner(&agent.id, &fingerprint, Instant::now())?
                != ScannerDecision::Changed
            {
                continue;
            }
            let room_id = snapshot.rooms[0].room_id.clone();
            let highest_sequence = snapshot.rooms[0].highest_sequence;
            wakes.push(AutonomousWake {
                agent_id: agent.id,
                room_id,
                trigger: "scanner",
                reason: format!("cross-room activity changed in {} active room(s)", rooms.len()),
                prompt_note: "Review the changed cross-room snapshot. Act only when you can advance shared work; publish through collaboration tools.".to_string(),
                context: serde_json::to_value(&snapshot)?,
                up_to_sequence: highest_sequence,
                stalled_claim: false,
            });
        }
        Ok(wakes)
    }

    async fn evaluate_agenda_candidate(
        &self,
        agent: &Agent,
        candidate: AgendaCandidate,
    ) -> Result<Option<AutonomousWake>, AutonomyError> {
        let stall_claimed = if candidate.stalled && candidate.cards.is_empty() {
            match self
                .state
                .try_claim_stall(&candidate.room_id, Instant::now())?
            {
                NudgeClaim::Claimed => true,
                NudgeClaim::CoolingDown | NudgeClaim::ClaimedByPeer => {
                    self.record_short_circuit(
                        agent,
                        &candidate,
                        TriageSource::RateLimited,
                        "stalled room is already claimed or cooling down",
                    )
                    .await?;
                    return Ok(None);
                }
                NudgeClaim::DeclineCap => {
                    self.record_short_circuit(
                        agent,
                        &candidate,
                        TriageSource::LoopCap,
                        "stalled room reached the three-decline cap",
                    )
                    .await?;
                    return Ok(None);
                }
            }
        } else {
            false
        };
        let (settings, settings_error) = match self.storage.triage_settings().await {
            Ok(settings) => (settings, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let started = Instant::now();
        let evaluated = match settings.as_ref() {
            Some(settings) => match self
                .triage
                .decide_agenda(
                    settings,
                    AgendaTriageContext {
                        agent,
                        candidate: &candidate,
                    },
                )
                .await
            {
                Ok(result) => EvaluatedAgenda {
                    actionable: result.decision.actionable,
                    response_mode: Some(result.decision.response_mode.as_database_str()),
                    source: TriageSource::SupportModel,
                    reason: result.decision.reason,
                    prompt_note: result.decision.prompt_note,
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                },
                Err(error) => {
                    let fallback = resolve_agenda_failure(error.to_string());
                    EvaluatedAgenda::fallback(fallback.source, fallback.reason)
                }
            },
            None => {
                let fallback = resolve_agenda_failure(
                    settings_error
                        .as_deref()
                        .unwrap_or("agenda model is not configured"),
                );
                EvaluatedAgenda::fallback(fallback.source, fallback.reason)
            }
        };
        let latency_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
        if let Err(error) = record_triage(
            &self.storage,
            &self.observations,
            TriageRecordInput {
                agent_id: &agent.id,
                room_id: &candidate.room_id,
                up_to_sequence: candidate.highest_sequence,
                actionable: evaluated.actionable,
                response_mode: evaluated.response_mode,
                source: evaluated.source.as_str(),
                reason: Some(&evaluated.reason),
                prompt_note: Some(&evaluated.prompt_note),
                provider_id: settings.as_ref().map(|value| value.provider_id.as_str()),
                model_id: settings.as_ref().map(|value| value.model_id.as_str()),
                input_tokens: evaluated.input_tokens,
                output_tokens: evaluated.output_tokens,
                latency_ms,
            },
        )
        .await
        {
            if stall_claimed {
                self.state.cancel_stall(&candidate.room_id)?;
            }
            return Err(error.into());
        }
        if stall_claimed {
            let current_sequence = match self.storage.room(&candidate.room_id).await {
                Ok(room) => room.map(|room| room.next_sequence),
                Err(error) => {
                    self.state.cancel_stall(&candidate.room_id)?;
                    return Err(error.into());
                }
            };
            if current_sequence != Some(candidate.highest_sequence) {
                self.state.cancel_stall(&candidate.room_id)?;
                self.record_short_circuit(
                    agent,
                    &candidate,
                    TriageSource::RateLimited,
                    "stalled room changed while the agenda gate was running",
                )
                .await?;
                return Ok(None);
            }
            if !evaluated.actionable {
                self.state
                    .finish_stall(&candidate.room_id, Instant::now(), false)?;
            }
        }
        if !evaluated.actionable {
            return Ok(None);
        }
        Ok(Some(AutonomousWake {
            agent_id: agent.id.clone(),
            room_id: candidate.room_id.clone(),
            trigger: "agenda",
            reason: evaluated.reason,
            prompt_note: evaluated.prompt_note,
            context: serde_json::to_value(&candidate)?,
            up_to_sequence: candidate.highest_sequence,
            stalled_claim: stall_claimed,
        }))
    }

    async fn record_empty_agenda(&self, agent: &Agent) -> Result<(), AutonomyError> {
        let Some((room_id, up_to_sequence)) = self.storage.first_agent_room(&agent.id).await?
        else {
            return Ok(());
        };
        record_triage(
            &self.storage,
            &self.observations,
            TriageRecordInput {
                agent_id: &agent.id,
                room_id: &room_id,
                up_to_sequence,
                actionable: false,
                response_mode: None,
                source: TriageSource::EmptyInbox.as_str(),
                reason: Some(
                    "agenda found no assigned or mentioned incomplete card and no stalled room",
                ),
                prompt_note: None,
                provider_id: None,
                model_id: None,
                input_tokens: None,
                output_tokens: None,
                latency_ms: 0,
            },
        )
        .await?;
        Ok(())
    }

    async fn record_short_circuit(
        &self,
        agent: &Agent,
        candidate: &AgendaCandidate,
        source: TriageSource,
        reason: &str,
    ) -> Result<(), AutonomyError> {
        record_triage(
            &self.storage,
            &self.observations,
            TriageRecordInput {
                agent_id: &agent.id,
                room_id: &candidate.room_id,
                up_to_sequence: candidate.highest_sequence,
                actionable: false,
                response_mode: None,
                source: source.as_str(),
                reason: Some(reason),
                prompt_note: None,
                provider_id: None,
                model_id: None,
                input_tokens: None,
                output_tokens: None,
                latency_ms: 0,
            },
        )
        .await?;
        Ok(())
    }
}

struct EvaluatedAgenda {
    actionable: bool,
    response_mode: Option<&'static str>,
    source: TriageSource,
    reason: String,
    prompt_note: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

impl EvaluatedAgenda {
    fn fallback(source: TriageSource, reason: String) -> Self {
        Self {
            actionable: true,
            response_mode: None,
            source,
            prompt_note: "The agenda gate failed; inspect the focused candidate and act only on real shared work.".to_string(),
            reason,
            input_tokens: None,
            output_tokens: None,
        }
    }
}

pub struct AutonomyHandle(JoinHandle<()>);

impl AutonomyHandle {
    pub async fn shutdown(self) {
        let _ = self.0.await;
    }
}

pub fn start(
    storage: CollabStorage,
    state: ProactivityHub,
    wakes: mpsc::UnboundedSender<AutonomousWake>,
    observations: ObservationSink,
    cancel: CancellationToken,
) -> AutonomyHandle {
    let task = tokio::spawn(async move {
        let engine = AutonomyEngine::new(storage, state, observations);
        let mut ticker = interval_at(tokio::time::Instant::now() + AGENDA_CHECK, AGENDA_CHECK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = ticker.tick() => {
                    match engine.sweep_agenda().await {
                        Ok(Some(wake)) => {
                            if wakes.send(wake).is_err() { return; }
                        }
                        Ok(None) => {}
                        Err(error) => eprintln!("agenda sweep failed: {error}"),
                    }
                    match engine.sweep_scanner().await {
                        Ok(found) => {
                            for wake in found {
                                if wakes.send(wake).is_err() { return; }
                            }
                        }
                        Err(error) => eprintln!("scanner sweep failed: {error}"),
                    }
                }
            }
        }
    });
    AutonomyHandle(task)
}

pub async fn record_dispatch_short_circuit(
    storage: &CollabStorage,
    observations: &ObservationSink,
    wake: &AutonomousWake,
    source: TriageSource,
    reason: &str,
) {
    if let Err(error) = record_triage(
        storage,
        observations,
        TriageRecordInput {
            agent_id: &wake.agent_id,
            room_id: &wake.room_id,
            up_to_sequence: wake.up_to_sequence,
            actionable: false,
            response_mode: None,
            source: source.as_str(),
            reason: Some(reason),
            prompt_note: None,
            provider_id: None,
            model_id: None,
            input_tokens: None,
            output_tokens: None,
            latency_ms: 0,
        },
    )
    .await
    {
        eprintln!("failed to record autonomous short circuit: {error}");
    }
}

#[derive(Debug, Error)]
pub enum AutonomyError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Proactivity(#[from] ProactivityError),
    #[error("autonomy JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}
