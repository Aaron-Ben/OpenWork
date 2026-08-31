use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgendaDecisionRequest, AgendaDecisionResponse, AgendaPayload, AgentTokenResponse,
    ComputerHeartbeatRequest, DesiredAgents, EngineInventoryReport, FinishRunRequest,
    InboxResponse, InvalidationEvent, OpenRunRequest, RunView, TriagePayload, TriageReportRequest,
};

use super::sse::{SseDecoder, SseParseError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct ComputerClient {
    http: reqwest::Client,
    base_url: String,
    runtime_session_id: String,
    computer_secret: String,
}

impl ComputerClient {
    pub fn new(base_url: String, runtime_session_id: String, computer_secret: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            runtime_session_id,
            computer_secret,
        }
    }

    pub async fn heartbeat(
        &self,
        state: &ComputerHeartbeatRequest,
    ) -> Result<(), RuntimeClientError> {
        self.http
            .post(format!("{}/computer/heartbeat", self.base_url))
            .bearer_auth(&self.computer_secret)
            .json(state)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    pub async fn desired_agents(&self) -> Result<DesiredAgents, RuntimeClientError> {
        let snapshot = self
            .http
            .get(format!("{}/computer/agents", self.base_url))
            .bearer_auth(&self.computer_secret)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json::<DesiredAgents>()
            .await?;
        if snapshot.runtime_session_id != self.runtime_session_id {
            return Err(RuntimeClientError::SessionMismatch);
        }
        Ok(snapshot)
    }

    pub async fn report_inventory(
        &self,
        report: &EngineInventoryReport,
    ) -> Result<(), RuntimeClientError> {
        self.http
            .post(format!("{}/computer/inventory", self.base_url))
            .bearer_auth(&self.computer_secret)
            .json(report)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    pub async fn mint_agent_token(
        &self,
        agent_id: &str,
    ) -> Result<AgentTokenResponse, RuntimeClientError> {
        self.http
            .post(format!(
                "{}/computer/agents/{agent_id}/token",
                self.base_url
            ))
            .bearer_auth(&self.computer_secret)
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

    pub async fn management_loop(
        &self,
        invalidations: mpsc::Sender<()>,
        shutdown: CancellationToken,
    ) {
        reconnecting_sse_loop(
            self.http.clone(),
            format!("{}/computer/events", self.base_url),
            SseCredential::Static(self.computer_secret.clone()),
            "management",
            invalidations,
            shutdown,
        )
        .await;
    }
}

#[derive(Clone)]
pub struct AgentClient {
    http: reqwest::Client,
    base_url: String,
    token: Arc<RwLock<String>>,
}

#[derive(Clone)]
enum SseCredential {
    Static(String),
    Refreshing(Arc<RwLock<String>>),
}

impl SseCredential {
    fn current(&self) -> String {
        match self {
            Self::Static(token) => token.clone(),
            Self::Refreshing(token) => token.read().expect("Agent token lock poisoned").clone(),
        }
    }
}

impl AgentClient {
    pub fn replace_token(&self, token: String) {
        *self.token.write().expect("Agent token lock poisoned") = token;
    }

    pub async fn inbox(&self) -> Result<InboxResponse, RuntimeClientError> {
        self.get_json("/agent/inbox").await
    }

    pub async fn open_run(&self, request: &OpenRunRequest) -> Result<RunView, RuntimeClientError> {
        self.post_json("/agent/runs", request).await
    }

    pub async fn triage_payload(&self, run_id: &str) -> Result<TriagePayload, RuntimeClientError> {
        self.get_json(&format!("/agent/inbox-triage/payload?run_id={run_id}"))
            .await
    }

    pub async fn report_triage(
        &self,
        request: &TriageReportRequest,
    ) -> Result<(), RuntimeClientError> {
        self.post_empty("/agent/triage", request).await
    }

    pub async fn agenda_payload(&self) -> Result<AgendaPayload, RuntimeClientError> {
        self.get_json("/agent/agenda/payload").await
    }

    pub async fn decide_agenda(
        &self,
        request: &AgendaDecisionRequest,
    ) -> Result<AgendaDecisionResponse, RuntimeClientError> {
        self.post_json("/agent/agenda/decision", request).await
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
                .post(format!("{}/agent/runs/{run_id}/finish", self.base_url))
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
            .post(format!("{}/agent/runs/{run_id}/heartbeat", self.base_url))
            .bearer_auth(self.token())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    pub async fn wake_loop(&self, wakes: mpsc::Sender<()>, shutdown: CancellationToken) {
        reconnecting_sse_loop(
            self.http.clone(),
            format!("{}/agent/events", self.base_url),
            SseCredential::Refreshing(self.token.clone()),
            "agent",
            wakes,
            shutdown,
        )
        .await;
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, RuntimeClientError> {
        self.http
            .get(format!("{}{}", self.base_url, path))
            .bearer_auth(self.token())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        request: &impl serde::Serialize,
    ) -> Result<T, RuntimeClientError> {
        self.http
            .post(format!("{}{}", self.base_url, path))
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

    async fn post_empty(
        &self,
        path: &str,
        request: &impl serde::Serialize,
    ) -> Result<(), RuntimeClientError> {
        self.http
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(self.token())
            .json(request)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?;
        Ok(())
    }

    fn token(&self) -> String {
        self.token
            .read()
            .expect("Agent token lock poisoned")
            .clone()
    }
}

async fn reconnecting_sse_loop(
    http: reqwest::Client,
    url: String,
    credential: SseCredential,
    event_name: &'static str,
    invalidations: mpsc::Sender<()>,
    shutdown: CancellationToken,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if shutdown.is_cancelled() || invalidations.is_closed() {
            return;
        }
        let connected_at = Instant::now();
        if let Err(error) = sse_once(
            &http,
            &url,
            &credential.current(),
            event_name,
            &invalidations,
            &shutdown,
        )
        .await
        {
            tracing::warn!(%error, %event_name, "Collaboration SSE disconnected");
        }
        if shutdown.is_cancelled() || invalidations.is_closed() {
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

async fn sse_once(
    http: &reqwest::Client,
    url: &str,
    token: &str,
    event_name: &str,
    invalidations: &mpsc::Sender<()>,
    shutdown: &CancellationToken,
) -> Result<(), RuntimeClientError> {
    let mut response = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?;
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
            if event.event.as_deref() != Some(event_name) {
                continue;
            }
            serde_json::from_str::<InvalidationEvent>(&event.data)?;
            let _ = invalidations.try_send(());
        }
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
    #[error("Collaboration Runtime request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Collaboration SSE was invalid: {0}")]
    Sse(#[from] SseParseError),
    #[error("Collaboration payload was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Server returned a different RuntimeSession")]
    SessionMismatch,
}

impl RuntimeClientError {
    pub fn is_terminal_identity_error(&self) -> bool {
        match self {
            Self::SessionMismatch => true,
            Self::Http(error) => matches!(
                error.status(),
                Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::CONFLICT)
            ),
            Self::Sse(_) | Self::Json(_) => false,
        }
    }

    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(error) => error.status().is_none_or(|status| {
                status.is_server_error()
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }),
            Self::Sse(_) | Self::Json(_) | Self::SessionMismatch => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use super::SseCredential;

    #[test]
    fn refreshing_sse_credential_reads_the_latest_agent_token() {
        let token = Arc::new(RwLock::new("first".to_string()));
        let credential = SseCredential::Refreshing(token.clone());
        assert_eq!(credential.current(), "first");

        *token.write().unwrap() = "second".to_string();
        assert_eq!(credential.current(), "second");
    }
}
