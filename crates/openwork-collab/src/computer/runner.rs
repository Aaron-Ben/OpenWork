use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgentAssignment, FinishRunRequest, MessageView, OpenRunRequest, TriageReportRequest,
};

use super::{
    client::{AgentClient, DeviceClient, RuntimeClientError},
    engine::{ClassifyRequest, EngineAdapter, EngineError, EngineUsage, TurnRequest, TurnResult},
    home::{AgentHome, HomeError},
    scheduling::{RunnerResources, is_rate_limited},
    triage::parse_triage,
};

pub struct AgentRunner<A: EngineAdapter> {
    assignment: AgentAssignment,
    device: DeviceClient,
    generation: i64,
    client: AgentClient,
    adapter: Arc<A>,
    home: AgentHome,
    poll_interval: Duration,
    token_expires_at: i64,
    resources: RunnerResources,
    triage_trouble_streak: u32,
    triage_backoff_until: Option<Instant>,
    engine_backoff_until: Option<Instant>,
    pending_finish: Option<(String, FinishRunRequest)>,
}

pub(super) struct RunnerIdentity {
    pub device: DeviceClient,
    pub generation: i64,
    pub token_expires_at: i64,
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

impl<A: EngineAdapter> AgentRunner<A> {
    pub fn new(
        assignment: AgentAssignment,
        identity: RunnerIdentity,
        client: AgentClient,
        adapter: Arc<A>,
        home: AgentHome,
        poll_interval: Duration,
        resources: RunnerResources,
    ) -> Self {
        Self {
            assignment,
            device: identity.device,
            generation: identity.generation,
            client,
            adapter,
            home,
            poll_interval,
            token_expires_at: identity.token_expires_at,
            resources,
            triage_trouble_streak: 0,
            triage_backoff_until: None,
            engine_backoff_until: None,
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
        result
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
            return Ok(());
        };
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
                self.adapter
                    .classify(ClassifyRequest {
                        cwd: self.home.triage_root.clone(),
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
                        self.finish_triage_failure(&run.id, error.to_string(), false)
                            .await?;
                        return Ok(());
                    }
                },
                Err(error) => {
                    let interrupted = matches!(error, EngineError::Cancelled);
                    if !interrupted {
                        self.note_triage_failure();
                    }
                    self.finish_triage_failure(&run.id, error.to_string(), interrupted)
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
        let prompt = build_prompt(&self.assignment, &inbox.messages, &verdict.prompt_note);
        let session = self.home.load_session().await?;
        let result = self
            .run_main_turn(prompt.clone(), session.clone(), cancellation.clone())
            .await;
        let result = if session.is_some() && matches!(&result, Err(EngineError::SessionInvalid(_)))
        {
            self.home.clear_session().await?;
            self.run_main_turn(prompt, None, cancellation).await
        } else {
            result
        };
        if let Err(error) = &result
            && is_rate_limited(error)
        {
            self.engine_backoff_until = Some(Instant::now() + Duration::from_secs(60));
        }
        match result {
            Ok(result) => {
                if let Some(session_id) = &result.session_id {
                    self.home.save_session(session_id).await?;
                }
                self.finish_or_queue(
                    run.id.clone(),
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
                let cancelled = matches!(error, super::engine::EngineError::Cancelled);
                self.finish_or_queue(
                    run.id.clone(),
                    FinishRunRequest {
                        status: if cancelled { "interrupted" } else { "failed" }.to_string(),
                        input_tokens: None,
                        cached_input_tokens: None,
                        output_tokens: None,
                        error_code: Some("ENGINE_ERROR".to_string()),
                        error_message: Some(error.to_string()),
                        assistant_text: None,
                    },
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn run_main_turn(
        &self,
        prompt: String,
        resume_session_id: Option<String>,
        cancellation: CancellationToken,
    ) -> Result<TurnResult, EngineError> {
        let _permit = self.resources.main_permit(&cancellation).await?;
        self.resources.gate(&cancellation).await?;
        let result = self
            .adapter
            .run_turn(TurnRequest {
                home: self.home.root.clone(),
                prompt,
                model: Some(self.assignment.model.clone()),
                resume_session_id,
                environment: self.home.environment.clone(),
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
    ) -> Result<(), RunnerError> {
        self.finish_or_queue(
            run_id.to_string(),
            FinishRunRequest {
                status: if interrupted { "interrupted" } else { "failed" }.to_string(),
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                error_code: Some("TRIAGE_ERROR".to_string()),
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

    async fn refresh_token_if_needed(&mut self) -> Result<(), RunnerError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if !token_needs_refresh(self.token_expires_at, now) {
            return Ok(());
        }
        let response = self
            .device
            .mint_agent_token(&self.assignment.id, self.generation)
            .await?;
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

fn build_prompt(
    assignment: &AgentAssignment,
    messages: &[MessageView],
    triage_note: &str,
) -> String {
    let mut prompt = format!(
        "You are {}. {}\nHandle the following durable collaboration delivery.\n",
        assignment.display_name, assignment.system_prompt
    );
    if !triage_note.trim().is_empty() {
        prompt.push_str(&format!("Triage focus: {triage_note}\n"));
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

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(transparent)]
    Runtime(#[from] RuntimeClientError),
    #[error(transparent)]
    Home(#[from] HomeError),
    #[error("Agent wake loop stopped unexpectedly")]
    WakeLoopStopped,
}

impl RunnerError {
    pub(super) fn is_fenced(&self) -> bool {
        self.to_string().contains("409 Conflict")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::{next_trigger, token_needs_refresh};

    #[test]
    fn refreshes_agent_token_with_five_minutes_remaining() {
        assert!(!token_needs_refresh(1_301, 1_000));
        assert!(token_needs_refresh(1_300, 1_000));
        assert!(token_needs_refresh(999, 1_000));
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
