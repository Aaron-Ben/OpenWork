use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgentAssignment, AgentRoster, AgentTokenResponse, DeviceStartResponse, FinishRunRequest,
    HeartbeatRequest, InboxResponse, OpenRunRequest, RunView, TriagePayload, TriageReportRequest,
};

use super::sse::{SseDecoder, SseParseError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct DeviceClient {
    http: reqwest::Client,
    base_url: String,
    device_token: String,
}

impl DeviceClient {
    pub fn new(base_url: String, device_token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            device_token,
        }
    }

    pub async fn start(&self) -> Result<i64, RuntimeClientError> {
        self.http
            .post(format!("{}/api/computers/me/start", self.base_url))
            .bearer_auth(&self.device_token)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json::<DeviceStartResponse>()
            .await
            .map(|response| response.generation)
            .map_err(Into::into)
    }

    pub async fn heartbeat(&self, request: &HeartbeatRequest) -> Result<(), RuntimeClientError> {
        self.http
            .post(format!("{}/api/computers/me/heartbeat", self.base_url))
            .bearer_auth(&self.device_token)
            .json(request)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    pub async fn roster(
        &self,
        generation: i64,
    ) -> Result<Vec<AgentAssignment>, RuntimeClientError> {
        self.http
            .get(format!(
                "{}/api/computers/me/agents?generation={generation}",
                self.base_url
            ))
            .bearer_auth(&self.device_token)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json::<AgentRoster>()
            .await
            .map(|response| response.agents)
            .map_err(Into::into)
    }

    pub async fn mint_agent_token(
        &self,
        agent_id: &str,
        generation: i64,
    ) -> Result<AgentTokenResponse, RuntimeClientError> {
        self.http
            .post(format!(
                "{}/api/computers/me/agents/{agent_id}/token",
                self.base_url
            ))
            .bearer_auth(&self.device_token)
            .json(&serde_json::json!({ "generation": generation }))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    pub fn agent(&self, token: String) -> AgentClient {
        AgentClient {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            token: Arc::new(RwLock::new(token)),
        }
    }
}

#[derive(Clone)]
pub struct AgentClient {
    http: reqwest::Client,
    base_url: String,
    token: Arc<RwLock<String>>,
}

impl AgentClient {
    pub fn replace_token(&self, token: String) {
        *self.token.write().expect("Agent token lock poisoned") = token;
    }

    pub async fn inbox(&self) -> Result<InboxResponse, RuntimeClientError> {
        self.http
            .get(format!("{}/runtime/inbox", self.base_url))
            .bearer_auth(self.token())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    pub async fn open_run(&self, request: &OpenRunRequest) -> Result<RunView, RuntimeClientError> {
        self.http
            .post(format!("{}/runtime/runs", self.base_url))
            .bearer_auth(self.token())
            .json(request)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    pub async fn triage_payload(&self, run_id: &str) -> Result<TriagePayload, RuntimeClientError> {
        self.http
            .get(format!(
                "{}/runtime/inbox-triage/payload?run_id={run_id}",
                self.base_url
            ))
            .bearer_auth(self.token())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    pub async fn report_triage(
        &self,
        request: &TriageReportRequest,
    ) -> Result<(), RuntimeClientError> {
        self.http
            .post(format!("{}/runtime/triage", self.base_url))
            .bearer_auth(self.token())
            .json(request)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    pub async fn finish_run(
        &self,
        run_id: &str,
        request: &FinishRunRequest,
    ) -> Result<RunView, RuntimeClientError> {
        let mut delay = Duration::from_millis(250);
        for attempt in 0..5 {
            let response = self
                .http
                .post(format!("{}/runtime/runs/{run_id}/finish", self.base_url))
                .bearer_auth(self.token())
                .json(request)
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await;
            let error = match response {
                Ok(response) if response.status().is_success() => {
                    return response.json().await.map_err(Into::into);
                }
                Ok(response) => response.error_for_status().unwrap_err(),
                Err(error) => error,
            };
            if attempt == 4 || !retryable_finish_error(&error) {
                return Err(error.into());
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(2));
        }
        unreachable!("finish retry loop returns on every terminal branch")
    }

    pub async fn heartbeat_run(&self, run_id: &str) -> Result<(), RuntimeClientError> {
        self.http
            .post(format!("{}/runtime/runs/{run_id}/heartbeat", self.base_url))
            .bearer_auth(self.token())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    pub async fn wake_loop(&self, wakes: mpsc::Sender<()>, shutdown: CancellationToken) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.is_cancelled() || wakes.is_closed() {
                return;
            }
            let connected_at = Instant::now();
            if let Err(error) = self.wake_stream_once(&wakes, &shutdown).await {
                tracing::warn!(%error, "Agent wake SSE disconnected");
            }
            if shutdown.is_cancelled() || wakes.is_closed() {
                return;
            }
            if connected_at.elapsed() >= Duration::from_secs(60) {
                backoff = Duration::from_secs(1);
            }
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }

    async fn wake_stream_once(
        &self,
        wakes: &mpsc::Sender<()>,
        shutdown: &CancellationToken,
    ) -> Result<(), RuntimeClientError> {
        let mut response = self
            .http
            .get(format!("{}/runtime/wake-stream", self.base_url))
            .bearer_auth(self.token())
            .send()
            .await?
            .error_for_status()?;
        let _ = wakes.try_send(());
        let mut decoder = SseDecoder::default();
        loop {
            let chunk = tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                chunk = response.chunk() => chunk?,
            };
            let Some(chunk) = chunk else {
                return Ok(());
            };
            for event in decoder.push(&chunk)? {
                if event.event.as_deref() != Some("wake") {
                    continue;
                }
                serde_json::from_str::<crate::protocol::WakeEvent>(&event.data)?;
                let _ = wakes.try_send(());
            }
        }
    }

    fn token(&self) -> String {
        self.token
            .read()
            .expect("Agent token lock poisoned")
            .clone()
    }
}

fn retryable_finish_error(error: &reqwest::Error) -> bool {
    error.status().is_none_or(|status| {
        status.is_server_error()
            || status == reqwest::StatusCode::REQUEST_TIMEOUT
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    })
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeClientError {
    #[error("collaboration Runtime request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("collaboration wake stream was invalid: {0}")]
    Sse(#[from] SseParseError),
    #[error("collaboration wake payload was invalid: {0}")]
    Json(#[from] serde_json::Error),
}

impl RuntimeClientError {
    pub fn is_terminal_identity_error(&self) -> bool {
        matches!(
            self,
            Self::Http(error)
                if matches!(
                    error.status(),
                    Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::CONFLICT)
                )
        )
    }

    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(error) => error.status().is_none_or(|status| {
                status.is_server_error()
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }),
            Self::Sse(_) | Self::Json(_) => false,
        }
    }
}
