use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgendaDecisionRequest, AgendaDecisionResponse, AgendaPayload, AgentTokenResponse,
    ComputerHeartbeatRequest, DesiredAgents, EngineInventoryReport, FinishRunRequest,
    InboxResponse, OpenRunRequest, RunView, TriagePayload, TriageReportRequest,
    sse::reconnecting_invalidation_loop,
};

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
        let credential = SseCredential::Static(self.computer_secret.clone());
        reconnecting_invalidation_loop(
            self.http.clone(),
            format!("{}/computer/events", self.base_url),
            move || credential.current(),
            "management",
            shutdown,
            move |_| {
                request_rerun(&invalidations);
                !invalidations.is_closed()
            },
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

    pub async fn wake_loop(&self, rerun_requested: mpsc::Sender<()>, shutdown: CancellationToken) {
        let credential = SseCredential::Refreshing(self.token.clone());
        reconnecting_invalidation_loop(
            self.http.clone(),
            format!("{}/agent/events", self.base_url),
            move || credential.current(),
            "agent",
            shutdown,
            move |_| {
                request_rerun(&rerun_requested);
                !rerun_requested.is_closed()
            },
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

fn request_rerun(invalidations: &mpsc::Sender<()>) {
    let _ = invalidations.try_send(());
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
        }
    }

    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(error) => error.status().is_none_or(|status| {
                status.is_server_error()
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }),
            Self::SessionMismatch => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Router,
        body::Body,
        extract::State,
        http::{Response, header},
        routing::get,
    };
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use crate::protocol::sse::reconnecting_invalidation_loop;

    use super::{SseCredential, request_rerun};

    #[test]
    fn refreshing_sse_credential_reads_the_latest_agent_token() {
        let token = Arc::new(RwLock::new("first".to_string()));
        let credential = SseCredential::Refreshing(token.clone());
        assert_eq!(credential.current(), "first");

        *token.write().unwrap() = "second".to_string();
        assert_eq!(credential.current(), "second");
    }

    #[tokio::test]
    async fn busy_wakes_coalesce_into_one_rerun_request() {
        let (rerun_requested, mut receiver) = mpsc::channel(1);
        for _ in 0..100 {
            request_rerun(&rerun_requested);
        }

        assert_eq!(receiver.recv().await, Some(()));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn management_and_agent_sse_reconnect_independently_after_disconnects() {
        #[derive(Clone, Default)]
        struct Requests {
            management: Arc<AtomicUsize>,
            agent: Arc<AtomicUsize>,
        }

        async fn finite_event(
            State(counter): State<Arc<AtomicUsize>>,
            event_name: &'static str,
        ) -> Response<Body> {
            let sequence = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let body = format!(
                "event: {event_name}\ndata: {{\"id\":\"event-{sequence}\",\"kind\":\"message\",\"subjectId\":null,\"revision\":null,\"publishedAt\":1}}\n\n"
            );
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(body))
                .unwrap()
        }

        async fn management(State(requests): State<Requests>) -> Response<Body> {
            finite_event(State(requests.management), "management").await
        }

        async fn agent(State(requests): State<Requests>) -> Response<Body> {
            finite_event(State(requests.agent), "agent").await
        }

        let requests = Requests::default();
        let app = Router::new()
            .route("/management", get(management))
            .route("/agent", get(agent))
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let shutdown = CancellationToken::new();
        let (management_tx, mut management_rx) = mpsc::channel(4);
        let (agent_tx, mut agent_rx) = mpsc::channel(4);
        let management_shutdown = shutdown.clone();
        let agent_shutdown = shutdown.clone();
        let management_loop = tokio::spawn(reconnecting_invalidation_loop(
            reqwest::Client::new(),
            format!("http://{address}/management"),
            || "management-secret".to_string(),
            "management",
            management_shutdown,
            move |_| {
                request_rerun(&management_tx);
                !management_tx.is_closed()
            },
        ));
        let agent_loop = tokio::spawn(reconnecting_invalidation_loop(
            reqwest::Client::new(),
            format!("http://{address}/agent"),
            || "agent-token".to_string(),
            "agent",
            agent_shutdown,
            move |_| {
                request_rerun(&agent_tx);
                !agent_tx.is_closed()
            },
        ));

        tokio::time::timeout(std::time::Duration::from_secs(4), async {
            assert_eq!(management_rx.recv().await, Some(()));
            assert_eq!(agent_rx.recv().await, Some(()));
            assert_eq!(management_rx.recv().await, Some(()));
            assert_eq!(agent_rx.recv().await, Some(()));
        })
        .await
        .expect("SSE clients did not reconnect after finite responses closed");

        assert!(requests.management.load(Ordering::SeqCst) >= 2);
        assert!(requests.agent.load(Ordering::SeqCst) >= 2);
        shutdown.cancel();
        management_loop.await.unwrap();
        agent_loop.await.unwrap();
        server.abort();
        let _ = server.await;
    }
}
