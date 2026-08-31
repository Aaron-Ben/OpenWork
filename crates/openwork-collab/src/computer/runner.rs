use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgendaDecisionRequest, AgentAssignment, ClimateView, FinishRunRequest, MessageView,
    OpenRunRequest, TriageReportRequest,
};

use super::{
    agenda::parse_agenda_decision,
    client::{AgentClient, ComputerClient, RuntimeClientError},
    engine::{
        AgentEngineRuntime, ClassifyRequest, EngineAdapter, EngineError, EngineUsage, TurnRequest,
        TurnResult,
    },
    home::{AgentHome, HomeError},
    scheduling::RunnerResources,
    triage::parse_triage,
};

const AGENDA_QUIET_WINDOW: Duration = Duration::from_secs(90);
const AGENDA_CHECK_INTERVAL: Duration = Duration::from_secs(60);

pub struct AgentRunner {
    assignment: AgentAssignment,
    computer: ComputerClient,
    client: AgentClient,
    engine: RunnerEngine,
    home: AgentHome,
    poll_interval: Duration,
    token_expires_at: i64,
    resources: RunnerResources,
    triage_trouble_streak: u32,
    triage_backoff_until: Option<Instant>,
    engine_backoff_until: Option<Instant>,
    agenda_backoff_until: Option<Instant>,
    agenda_failure_streak: u32,
    quiet_since: Instant,
    last_agenda_check: Option<Instant>,
    pending_finish: Option<(String, FinishRunRequest)>,
}

pub(super) struct RunnerIdentity {
    pub computer: ComputerClient,
    pub token_expires_at: i64,
}

pub(super) struct RunnerEngine {
    pub adapter: Arc<dyn EngineAdapter>,
    pub runtime: Box<dyn AgentEngineRuntime>,
}

struct RunHeartbeat {
    shutdown: CancellationToken,
    _task: tokio::task::JoinHandle<()>,
}

impl RunHeartbeat {
    fn start(client: AgentClient, run_id: String) -> Self {
        let shutdown = CancellationToken::new();
        let task_shutdown = shutdown.clone();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => return,
                    _ = interval.tick() => {
                        if let Err(error) = client.heartbeat_run(&run_id).await {
                            tracing::warn!(%run_id, %error, "run heartbeat failed");
                        }
                    }
                }
            }
        });
        Self {
            shutdown,
            _task: task,
        }
    }
}

impl Drop for RunHeartbeat {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl AgentRunner {
    pub fn new(
        assignment: AgentAssignment,
        identity: RunnerIdentity,
        client: AgentClient,
        engine: RunnerEngine,
        home: AgentHome,
        poll_interval: Duration,
        resources: RunnerResources,
    ) -> Self {
        Self {
            assignment,
            computer: identity.computer,
            client,
            engine,
            home,
            poll_interval,
            token_expires_at: identity.token_expires_at,
            resources,
            triage_trouble_streak: 0,
            triage_backoff_until: None,
            engine_backoff_until: None,
            agenda_backoff_until: None,
            agenda_failure_streak: 0,
            quiet_since: Instant::now(),
            last_agenda_check: None,
            pending_finish: None,
        }
    }

    pub async fn run(mut self, shutdown: CancellationToken) -> Result<(), RunnerError> {
        let (wake_tx, mut wake_rx) = mpsc::channel(1);
        let wake_shutdown = shutdown.child_token();
        let wake_client = self.client.clone();
        let wake_task_shutdown = wake_shutdown.clone();
        let wake_task = tokio::spawn(async move {
            wake_client.wake_loop(wake_tx, wake_task_shutdown).await;
        });
        let mut interval = tokio::time::interval(self.poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let result = 'runner: loop {
            let Some(debounce) = next_trigger(&mut wake_rx, &mut interval, &shutdown).await? else {
                break 'runner Ok(());
            };
            if debounce && !debounce_wakes(&mut wake_rx, &shutdown).await {
                break 'runner Ok(());
            }
            loop {
                match self.drive_once(shutdown.clone()).await {
                    Ok(()) => {}
                    Err(error) if error.is_fenced() => break 'runner Err(error),
                    Err(error) => tracing::warn!(
                        agent_id = self.assignment.id,
                        %error,
                        "Agent turn attempt failed; unread inbox remains durable"
                    ),
                }
                if shutdown.is_cancelled() {
                    break 'runner Ok(());
                }
                if wake_rx.try_recv().is_err() {
                    break;
                }
            }
        };
        wake_shutdown.cancel();
        drop(wake_rx);
        let _ = wake_task.await;
        let engine_shutdown = self.engine.runtime.shutdown().await;
        result.and(engine_shutdown.map_err(RunnerError::Engine))
    }

    async fn drive_once(&mut self, cancellation: CancellationToken) -> Result<(), RunnerError> {
        if !self.retry_pending_finish().await? {
            return Ok(());
        }
        let now = Instant::now();
        if self
            .triage_backoff_until
            .is_some_and(|deadline| deadline > now)
            || self
                .engine_backoff_until
                .is_some_and(|deadline| deadline > now)
        {
            return Ok(());
        }
        self.refresh_token_if_needed().await?;
        let inbox = self.client.inbox().await?;
        let Some(trigger) = inbox.trigger else {
            return self.maybe_agenda(cancellation).await;
        };
        self.quiet_since = Instant::now();
        let run = self.client.open_run(&OpenRunRequest { trigger }).await?;
        let _run_heartbeat = RunHeartbeat::start(self.client.clone(), run.id.clone());
        let payload = match self.client.triage_payload(&run.id).await {
            Ok(payload) => payload,
            Err(error) => {
                let message = error.to_string();
                self.finish_or_queue(
                    run.id.clone(),
                    FinishRunRequest {
                        status: "failed".to_string(),
                        input_tokens: None,
                        cached_input_tokens: None,
                        output_tokens: None,
                        error_code: Some("TRIAGE_PAYLOAD_ERROR".to_string()),
                        error_message: Some(message),
                        assistant_text: None,
                    },
                )
                .await?;
                return Ok(());
            }
        };
        let triage_started = Instant::now();
        let (verdict, triage_usage, triage_model) = if let Some(verdict) = payload.verdict {
            (verdict, EngineUsage::default(), payload.model)
        } else {
            let prompt = format!(
                "{}\n\n{}",
                payload.instructions.unwrap_or_default(),
                payload.input.unwrap_or_default()
            );
            let result = async {
                let _permit = self.resources.triage_permit(&cancellation).await?;
                self.resources.gate(&cancellation).await?;
                self.engine
                    .adapter
                    .classify(ClassifyRequest {
                        cwd: self.home.work_root.clone(),
                        config_root: self.home.config_root.clone(),
                        prompt,
                        model: Some(payload.model.clone()),
                        environment: self.home.environment.clone(),
                        cancellation: cancellation.clone(),
                    })
                    .await
            }
            .await;
            self.resources.observe_result(&result).await;
            match result {
                Ok(result) => match parse_triage(&result.text) {
                    Ok(verdict) => (verdict, result.usage, result.model.unwrap_or(payload.model)),
                    Err(error) => {
                        self.note_triage_failure();
                        self.finish_triage_failure(&run.id, error.to_string(), false, false)
                            .await?;
                        return Ok(());
                    }
                },
                Err(error) => {
                    let interrupted = matches!(error, EngineError::Cancelled);
                    let rate_limited = error.is_rate_limited();
                    if !interrupted {
                        self.note_triage_failure();
                    }
                    self.finish_triage_failure(
                        &run.id,
                        error.to_string(),
                        interrupted,
                        rate_limited,
                    )
                    .await?;
                    return Ok(());
                }
            }
        };
        self.triage_trouble_streak = 0;
        self.triage_backoff_until = None;
        let report = TriageReportRequest {
            run_id: run.id.clone(),
            verdict: verdict.clone(),
            model: triage_model,
            input_tokens: Some(triage_usage.input_tokens as i64),
            output_tokens: Some(triage_usage.output_tokens as i64),
            latency_ms: Some(triage_started.elapsed().as_millis() as i64),
        };
        if let Err(error) = self.client.report_triage(&report).await {
            self.finish_or_queue(
                run.id.clone(),
                FinishRunRequest {
                    status: "failed".to_string(),
                    input_tokens: None,
                    cached_input_tokens: None,
                    output_tokens: None,
                    error_code: Some("TRIAGE_REPORT_ERROR".to_string()),
                    error_message: Some(error.to_string()),
                    assistant_text: None,
                },
            )
            .await?;
            return Ok(());
        }
        if !verdict.actionable {
            self.finish_or_queue(
                run.id.clone(),
                FinishRunRequest {
                    status: "completed".to_string(),
                    input_tokens: Some(triage_usage.input_tokens as i64),
                    cached_input_tokens: Some(triage_usage.cached_input_tokens as i64),
                    output_tokens: Some(triage_usage.output_tokens as i64),
                    error_code: None,
                    error_message: None,
                    assistant_text: None,
                },
            )
            .await?;
            return Ok(());
        }
        let prompt = build_prompt(
            &self.assignment,
            &inbox.messages,
            &inbox.climates,
            &verdict.prompt_note,
            inbox.carried_over,
        );
        self.execute_main_run(run.id, prompt, cancellation).await?;
        self.quiet_since = Instant::now();
        Ok(())
    }

    async fn execute_main_run(
        &mut self,
        run_id: String,
        prompt: String,
        cancellation: CancellationToken,
    ) -> Result<(), RunnerError> {
        let result = self.run_main_turn(prompt, cancellation).await;
        if let Err(error) = &result
            && error.is_rate_limited()
        {
            self.engine_backoff_until = Some(
                Instant::now()
                    + error
                        .retry_after()
                        .unwrap_or_else(|| Duration::from_secs(60)),
            );
        }
        match result {
            Ok(result) => {
                self.finish_or_queue(
                    run_id.clone(),
                    FinishRunRequest {
                        status: "completed".to_string(),
                        input_tokens: Some(result.usage.input_tokens as i64),
                        cached_input_tokens: Some(result.usage.cached_input_tokens as i64),
                        output_tokens: Some(result.usage.output_tokens as i64),
                        error_code: None,
                        error_message: None,
                        assistant_text: Some(result.text),
                    },
                )
                .await?;
            }
            Err(error) => {
                let cancelled = matches!(error, EngineError::Cancelled);
                let rate_limited = error.is_rate_limited();
                self.finish_or_queue(
                    run_id,
                    FinishRunRequest {
                        status: if cancelled { "interrupted" } else { "failed" }.to_string(),
                        input_tokens: None,
                        cached_input_tokens: None,
                        output_tokens: None,
                        error_code: Some(
                            if rate_limited {
                                "ENGINE_RATE_LIMITED"
                            } else {
                                "ENGINE_ERROR"
                            }
                            .to_string(),
                        ),
                        error_message: Some(error.to_string()),
                        assistant_text: None,
                    },
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn maybe_agenda(&mut self, cancellation: CancellationToken) -> Result<(), RunnerError> {
        let now = Instant::now();
        if !agenda_due(
            self.assignment.agenda_enabled,
            self.quiet_since.elapsed(),
            self.last_agenda_check.map(|last| last.elapsed()),
            self.agenda_backoff_until
                .is_some_and(|deadline| deadline > now),
        ) {
            return Ok(());
        }
        self.last_agenda_check = Some(Instant::now());
        let payload = match self.client.agenda_payload().await {
            Ok(payload) => payload,
            Err(error) if error.is_terminal_identity_error() => return Err(error.into()),
            Err(error) => {
                self.note_agenda_failure();
                tracing::warn!(agent_id = self.assignment.id, %error, "Agenda payload failed closed");
                return Ok(());
            }
        };
        if payload.candidate_set.candidates.is_empty() {
            self.agenda_failure_streak = 0;
            self.agenda_backoff_until = None;
            return Ok(());
        }
        let classify_started = Instant::now();
        let result = async {
            let _permit = self.resources.triage_permit(&cancellation).await?;
            self.resources.gate(&cancellation).await?;
            self.engine
                .adapter
                .classify(ClassifyRequest {
                    cwd: self.home.work_root.clone(),
                    config_root: self.home.config_root.clone(),
                    prompt: payload.classify_prompt,
                    model: Some(self.assignment.triage_model_id.clone()),
                    environment: self.home.environment.clone(),
                    cancellation: cancellation.clone(),
                })
                .await
        }
        .await;
        self.resources.observe_result(&result).await;
        let result = match result {
            Ok(result) => result,
            Err(EngineError::Cancelled) => return Ok(()),
            Err(error) => {
                self.note_agenda_failure();
                tracing::warn!(agent_id = self.assignment.id, %error, "local Agenda classification failed closed");
                return Ok(());
            }
        };
        let decision = match parse_agenda_decision(&result.text) {
            Ok(decision) => decision,
            Err(error) => {
                self.note_agenda_failure();
                tracing::warn!(agent_id = self.assignment.id, %error, "local Agenda decision was rejected");
                return Ok(());
            }
        };
        let response = match self
            .client
            .decide_agenda(&AgendaDecisionRequest {
                candidate_set: payload.candidate_set,
                decision,
                model: result
                    .model
                    .unwrap_or_else(|| self.assignment.triage_model_id.clone()),
                input_tokens: result.usage.input_tokens as i64,
                output_tokens: result.usage.output_tokens as i64,
                latency_ms: classify_started.elapsed().as_millis() as i64,
            })
            .await
        {
            Ok(response) => response,
            Err(error) if error.is_terminal_identity_error() => return Err(error.into()),
            Err(error) => {
                self.note_agenda_failure();
                tracing::warn!(agent_id = self.assignment.id, %error, "Agenda decision failed closed");
                return Ok(());
            }
        };
        self.agenda_failure_streak = 0;
        self.agenda_backoff_until = None;
        let Some(trigger) = response.trigger else {
            return Ok(());
        };
        let brief = response.focused_brief.ok_or_else(|| {
            RunnerError::AgendaProtocol("trigger has no focused brief".to_string())
        })?;
        let run = self.client.open_run(&OpenRunRequest { trigger }).await?;
        let _run_heartbeat = RunHeartbeat::start(self.client.clone(), run.id.clone());
        let prompt = build_agenda_prompt(&self.assignment, &brief);
        self.execute_main_run(run.id, prompt, cancellation).await?;
        self.quiet_since = Instant::now();
        Ok(())
    }

    async fn run_main_turn(
        &mut self,
        prompt: String,
        cancellation: CancellationToken,
    ) -> Result<TurnResult, EngineError> {
        let _permit = self.resources.main_permit(&cancellation).await?;
        self.resources.gate(&cancellation).await?;
        let result = self
            .engine
            .runtime
            .run_turn(TurnRequest {
                prompt,
                cancellation,
            })
            .await;
        self.resources.observe_result(&result).await;
        result
    }

    async fn finish_triage_failure(
        &mut self,
        run_id: &str,
        message: String,
        interrupted: bool,
        rate_limited: bool,
    ) -> Result<(), RunnerError> {
        self.finish_or_queue(
            run_id.to_string(),
            FinishRunRequest {
                status: if interrupted { "interrupted" } else { "failed" }.to_string(),
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                error_code: Some(
                    if rate_limited {
                        "TRIAGE_RATE_LIMITED"
                    } else {
                        "TRIAGE_ERROR"
                    }
                    .to_string(),
                ),
                error_message: Some(message),
                assistant_text: None,
            },
        )
        .await?;
        Ok(())
    }

    async fn retry_pending_finish(&mut self) -> Result<bool, RunnerError> {
        let Some((run_id, request)) = self.pending_finish.take() else {
            return Ok(true);
        };
        self.finish_or_queue(run_id, request).await
    }

    async fn finish_or_queue(
        &mut self,
        run_id: String,
        request: FinishRunRequest,
    ) -> Result<bool, RunnerError> {
        match self.client.finish_run(&run_id, &request).await {
            Ok(_) => Ok(true),
            Err(error) if error.is_transient() => {
                tracing::warn!(agent_id = self.assignment.id, %run_id, %error, "run finish remains pending");
                self.pending_finish = Some((run_id, request));
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn note_triage_failure(&mut self) {
        self.triage_trouble_streak = self.triage_trouble_streak.saturating_add(1);
        let exponent = self.triage_trouble_streak.saturating_sub(1).min(4);
        let seconds = 30_u64.saturating_mul(1_u64 << exponent).min(10 * 60);
        self.triage_backoff_until = Some(Instant::now() + Duration::from_secs(seconds));
    }

    fn note_agenda_failure(&mut self) {
        self.agenda_failure_streak = self.agenda_failure_streak.saturating_add(1);
        let exponent = self.agenda_failure_streak.saturating_sub(1).min(4);
        let seconds = 60_u64.saturating_mul(1_u64 << exponent).min(15 * 60);
        self.agenda_backoff_until = Some(Instant::now() + Duration::from_secs(seconds));
    }

    async fn refresh_token_if_needed(&mut self) -> Result<(), RunnerError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if !token_needs_refresh(self.token_expires_at, now) {
            return Ok(());
        }
        let response = self.computer.mint_agent_token(&self.assignment.id).await?;
        self.home.save_runtime_token(&response.token).await?;
        self.client.replace_token(response.token);
        self.token_expires_at = response.expires_at;
        Ok(())
    }
}

async fn next_trigger(
    wakes: &mut mpsc::Receiver<()>,
    interval: &mut tokio::time::Interval,
    shutdown: &CancellationToken,
) -> Result<Option<bool>, RunnerError> {
    tokio::select! {
        biased;
        wake = wakes.recv() => match wake {
            Some(()) => Ok(Some(true)),
            None if shutdown.is_cancelled() => Ok(None),
            None => Err(RunnerError::WakeLoopStopped),
        },
        _ = shutdown.cancelled() => Ok(None),
        _ = interval.tick() => Ok(Some(false)),
    }
}

async fn debounce_wakes(wakes: &mut mpsc::Receiver<()>, shutdown: &CancellationToken) -> bool {
    let delay = tokio::time::sleep(Duration::from_millis(2_500));
    tokio::pin!(delay);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return false,
            _ = &mut delay => return true,
            wake = wakes.recv() => {
                if wake.is_none() {
                    return false;
                }
            }
        }
    }
}

fn token_needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at <= now + 5 * 60
}

fn agenda_due(
    enabled: bool,
    quiet_for: Duration,
    since_last_check: Option<Duration>,
    backoff_active: bool,
) -> bool {
    enabled
        && quiet_for >= AGENDA_QUIET_WINDOW
        && since_last_check.is_none_or(|elapsed| elapsed >= AGENDA_CHECK_INTERVAL)
        && !backoff_active
}

fn build_prompt(
    assignment: &AgentAssignment,
    messages: &[MessageView],
    climates: &[ClimateView],
    triage_note: &str,
    carried_over: bool,
) -> String {
    let mut prompt = format!(
        "You are {}. {}\nHandle the following durable collaboration delivery.\n",
        assignment.display_name, assignment.persona
    );
    if !triage_note.trim().is_empty() {
        prompt.push_str(&format!("Triage focus: {triage_note}\n"));
    }
    if carried_over {
        prompt.push_str(
            "This is the oldest bounded inbox batch; more unread messages remain for a later run.\n",
        );
    }
    if !climates.is_empty() {
        prompt.push_str(
            "Private Climate context follows. These are your subjective current impressions, not objective facts.\n",
        );
        for climate in climates {
            prompt.push_str(&format!(
                "about {}: affinity={}, trust={}, note={}\n",
                climate.about_participant_id,
                climate.affinity,
                climate.trust,
                climate.last_note.as_deref().unwrap_or("none"),
            ));
        }
    }
    for message in messages {
        prompt.push_str(&format!(
            "room_id: {}\n[{}] {}: {}\n",
            message.room_id, message.sequence, message.author_id, message.body
        ));
    }
    prompt.push_str(
        "Publish collaboration actions with the openwork CLI. Assistant text alone is not sent.\n",
    );
    prompt
}

fn build_agenda_prompt(assignment: &AgentAssignment, focused_brief: &str) -> String {
    format!(
        "You are {}. {}\nHandle this proactive collaboration turn.\n{}\n",
        assignment.display_name, assignment.persona, focused_brief
    )
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(transparent)]
    Runtime(#[from] RuntimeClientError),
    #[error(transparent)]
    Home(#[from] HomeError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("Agent wake loop stopped unexpectedly")]
    WakeLoopStopped,
    #[error("Agenda protocol failed: {0}")]
    AgendaProtocol(String),
}

impl RunnerError {
    pub(super) fn is_fenced(&self) -> bool {
        matches!(
            self,
            Self::Runtime(error) if error.is_terminal_identity_error()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::protocol::{AgentAssignment, ClimateView, MessageView};
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::{agenda_due, build_prompt, next_trigger, token_needs_refresh};

    #[test]
    fn refreshes_agent_token_with_five_minutes_remaining() {
        assert!(!token_needs_refresh(1_301, 1_000));
        assert!(token_needs_refresh(1_300, 1_000));
        assert!(token_needs_refresh(999, 1_000));
    }

    #[test]
    fn agenda_requires_enabled_quiet_interval_and_no_backoff() {
        assert!(!agenda_due(false, Duration::from_secs(120), None, false));
        assert!(!agenda_due(true, Duration::from_secs(89), None, false));
        assert!(!agenda_due(
            true,
            Duration::from_secs(90),
            Some(Duration::from_secs(59)),
            false
        ));
        assert!(!agenda_due(
            true,
            Duration::from_secs(90),
            Some(Duration::from_secs(60)),
            true
        ));
        assert!(agenda_due(
            true,
            Duration::from_secs(90),
            Some(Duration::from_secs(60)),
            false
        ));
    }

    #[test]
    fn main_prompt_projects_only_the_current_private_climate_snapshot() {
        let assignment = AgentAssignment {
            id: "alpha".to_string(),
            display_name: "Alpha".to_string(),
            role: None,
            persona: "Investigate carefully.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "local/main".to_string(),
            triage_model_id: "local/triage".to_string(),
            config_revision: 1,
            agenda_enabled: false,
        };
        let prompt = build_prompt(
            &assignment,
            &[MessageView {
                id: "msg-1".to_string(),
                room_id: "room-1".to_string(),
                sequence: 1,
                author_id: "beta".to_string(),
                body: "Please review this.".to_string(),
            }],
            &[ClimateView {
                agent_id: "alpha".to_string(),
                about_participant_id: "beta".to_string(),
                affinity: 0.75,
                trust: 0.5,
                last_note: Some("Strong technically; verify estimates.".to_string()),
                updated_at: "2026-08-31T20:00:00+08:00".to_string(),
            }],
            "",
            false,
        );

        assert!(prompt.contains("subjective current impressions"));
        assert!(prompt.contains("about beta: affinity=0.75, trust=0.5"));
        assert!(prompt.contains("Strong technically; verify estimates."));
        assert!(prompt.contains("Please review this."));
    }

    #[tokio::test]
    async fn closed_wake_channel_is_graceful_during_shutdown() {
        let (sender, mut wakes) = mpsc::channel(1);
        drop(sender);
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let mut interval = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(60),
            Duration::from_secs(60),
        );

        let trigger = next_trigger(&mut wakes, &mut interval, &shutdown).await;

        assert!(matches!(trigger, Ok(None)));
    }
}
