use crate::protocol::{
    AgentAssignment, AgentRoster, AgentTokenResponse, CliRequest, CliResult, DeviceStartResponse,
    FinishRunRequest, HeartbeatRequest, InboxResponse, OpenRunRequest, RunView,
};

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
            token,
        }
    }
}

#[derive(Clone)]
pub struct AgentClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl AgentClient {
    pub fn replace_token(&mut self, token: String) {
        self.token = token;
    }

    pub async fn inbox(&self) -> Result<InboxResponse, RuntimeClientError> {
        self.http
            .get(format!("{}/runtime/inbox", self.base_url))
            .bearer_auth(&self.token)
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
            .bearer_auth(&self.token)
            .json(request)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    #[allow(dead_code)]
    pub async fn cli(&self, argv: Vec<String>) -> Result<CliResult, RuntimeClientError> {
        self.http
            .post(format!("{}/runtime/cli", self.base_url))
            .bearer_auth(&self.token)
            .json(&CliRequest { argv })
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }

    pub async fn finish_run(
        &self,
        run_id: &str,
        request: &FinishRunRequest,
    ) -> Result<RunView, RuntimeClientError> {
        self.http
            .post(format!("{}/runtime/runs/{run_id}/finish", self.base_url))
            .bearer_auth(&self.token)
            .json(request)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)?
            .json()
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeClientError {
    #[error("collaboration Runtime request failed: {0}")]
    Http(#[from] reqwest::Error),
}
