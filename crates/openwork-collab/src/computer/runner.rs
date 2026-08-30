use std::{sync::Arc, time::Duration};

use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use crate::protocol::{AgentAssignment, FinishRunRequest, MessageView, OpenRunRequest};

use super::{
    client::{AgentClient, DeviceClient, RuntimeClientError},
    engine::{EngineAdapter, TurnRequest},
    home::{AgentHome, HomeError},
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
}

pub(super) struct RunnerIdentity {
    pub device: DeviceClient,
    pub generation: i64,
    pub token_expires_at: i64,
}

impl<A: EngineAdapter> AgentRunner<A> {
    pub fn new(
        assignment: AgentAssignment,
        identity: RunnerIdentity,
        client: AgentClient,
        adapter: Arc<A>,
        home: AgentHome,
        poll_interval: Duration,
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
        }
    }

    pub async fn run(mut self, shutdown: CancellationToken) -> Result<(), RunnerError> {
        self.drive_once(shutdown.clone()).await?;
        let mut interval = tokio::time::interval(self.poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                _ = interval.tick() => {
                    if let Err(error) = self.drive_once(shutdown.clone()).await
                        && error.is_fenced()
                    {
                        return Err(error);
                    }
                }
            }
        }
    }

    async fn drive_once(&mut self, cancellation: CancellationToken) -> Result<(), RunnerError> {
        self.refresh_token_if_needed().await?;
        let inbox = self.client.inbox().await?;
        let Some(trigger) = inbox.trigger else {
            return Ok(());
        };
        let run = self.client.open_run(&OpenRunRequest { trigger }).await?;
        let session = self.home.load_session().await?;
        let result = self
            .adapter
            .run_turn(TurnRequest {
                home: self.home.root.clone(),
                prompt: build_prompt(&self.assignment, &inbox.messages),
                model: Some(self.assignment.model.clone()),
                resume_session_id: session,
                environment: self.home.environment.clone(),
                cancellation,
            })
            .await;
        match result {
            Ok(result) => {
                if let Some(session_id) = &result.session_id {
                    self.home.save_session(session_id).await?;
                }
                self.client
                    .finish_run(
                        &run.id,
                        &FinishRunRequest {
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
                self.client
                    .finish_run(
                        &run.id,
                        &FinishRunRequest {
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

fn token_needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at <= now + 5 * 60
}

fn build_prompt(assignment: &AgentAssignment, messages: &[MessageView]) -> String {
    let mut prompt = format!(
        "You are {}. {}\nHandle the following durable collaboration delivery.\n",
        assignment.display_name, assignment.system_prompt
    );
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
}

impl RunnerError {
    fn is_fenced(&self) -> bool {
        self.to_string().contains("409 Conflict")
    }
}

#[cfg(test)]
mod tests {
    use super::token_needs_refresh;

    #[test]
    fn refreshes_agent_token_with_five_minutes_remaining() {
        assert!(!token_needs_refresh(1_301, 1_000));
        assert!(token_needs_refresh(1_300, 1_000));
        assert!(token_needs_refresh(999, 1_000));
    }
}
