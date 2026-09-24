//! 协作 Server 的集成测试夹具：每个测试一个隔离数据库，经 HTTP 以 Desktop 与 Agent 身份发命令。
//! `messaging.rs` 与 `posting.rs` 以 `#[path]` 引入。

use openwork_collab::{
    protocol::{
        AgentCommand, AgentCommandRequest, AgentCommandResponse, AgentTokenResponse, AgentView,
        DesktopCommand, DesktopCommandRequest, DesktopCommandResult, FinishRunRequest,
        InboxResponse, OpenRunRequest, RoomView, RunView, TriagePayload, TriageReportRequest,
        request_id,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sqlx::{Executor, PgPool};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

pub struct Fixture {
    _guard: MutexGuard<'static, ()>,
    pub admin: PgPool,
    pub database: String,
    pub pool: PgPool,
    pub server: openwork_collab::server::ServerHandle,
    pub http: reqwest::Client,
    pub base_url: String,
    pub desktop_secret: String,
    pub computer_secret: String,
}

impl Fixture {
    pub async fn start() -> Option<Self> {
        let guard = FIXTURE_LOCK.lock().await;
        let base = std::env::var("TEST_DATABASE_URL").ok()?;
        let redis_url = std::env::var("TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
        let admin = PgPool::connect(&base).await.unwrap();
        let database = format!("collab_messaging_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE {database}").as_str())
            .await
            .unwrap();
        let (prefix, _) = base.rsplit_once('/').unwrap();
        let database_url = format!("{prefix}/{database}");
        let credentials = RuntimeCredentials::generate();
        let desktop_secret = credentials.desktop_secret.clone();
        let computer_secret = credentials.computer_secret.clone();
        let server = CollaborationServer::start(
            ServerOptions {
                database_url: database_url.clone(),
                redis_url,
                runtime_bind: "127.0.0.1:0".parse().unwrap(),
                credentials,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let base_url = format!("http://{}", server.runtime_addr());
        let pool = PgPool::connect(&database_url).await.unwrap();
        Some(Self {
            _guard: guard,
            admin,
            database,
            pool,
            server,
            http: reqwest::Client::new(),
            base_url,
            desktop_secret,
            computer_secret,
        })
    }

    pub async fn desktop(&self, command: DesktopCommand) -> DesktopCommandResult {
        let response = self
            .http
            .post(format!("{}/desktop/commands", self.base_url))
            .bearer_auth(&self.desktop_secret)
            .json(&DesktopCommandRequest {
                request_id: command.is_mutating().then(request_id),
                command: command.clone(),
            })
            .send()
            .await
            .unwrap();
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap();
            panic!("desktop command {command:?} failed with {status}: {body}");
        }
        response.json().await.unwrap()
    }

    pub async fn create_agent(&self, name: &str) -> AgentView {
        let result = self
            .desktop(DesktopCommand::CreateAgent {
                display_name: name.to_string(),
                role: Some("Collaborator".to_string()),
                persona: format!("You are {name}."),
                engine_id: "opencode".to_string(),
                main_model_id: "local/main".to_string(),
                triage_model_id: "local/triage".to_string(),
            })
            .await;
        let DesktopCommandResult::Agent(agent) = result else {
            panic!("create Agent returned the wrong result")
        };
        agent
    }

    pub async fn create_direct(&self, agent_id: &str) -> RoomView {
        let result = self
            .desktop(DesktopCommand::CreateDirectRoom {
                agent_id: agent_id.to_string(),
            })
            .await;
        let DesktopCommandResult::Room(room) = result else {
            panic!("create Direct Room returned the wrong result")
        };
        room
    }

    pub async fn create_group(&self, agent_ids: Vec<String>) -> RoomView {
        let result = self
            .desktop(DesktopCommand::CreateGroupRoom {
                title: "R5 coordination".to_string(),
                agent_ids,
            })
            .await;
        let DesktopCommandResult::Room(room) = result else {
            panic!("create Group Room returned the wrong result")
        };
        room
    }

    pub async fn send_user(&self, room_id: &str, body: &str) {
        let result = self
            .desktop(DesktopCommand::SendMessage {
                room_id: room_id.to_string(),
                body: body.to_string(),
                quoted_message_id: None,
            })
            .await;
        assert!(matches!(result, DesktopCommandResult::Message(_)));
    }

    pub async fn token(&self, agent_id: &str) -> String {
        self.http
            .post(format!(
                "{}/computer/agents/{agent_id}/token",
                self.base_url
            ))
            .bearer_auth(&self.computer_secret)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<AgentTokenResponse>()
            .await
            .unwrap()
            .token
    }

    pub async fn inbox(&self, token: &str) -> InboxResponse {
        let response = self
            .http
            .get(format!("{}/agent/inbox", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap();
            panic!("inbox failed with {status}: {body}");
        }
        response.json().await.unwrap()
    }

    pub async fn agent_events(&self, token: &str) -> reqwest::Response {
        self.http
            .get(format!("{}/agent/events", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
    }

    pub async fn open_run(&self, token: &str, inbox: &InboxResponse) -> RunView {
        self.http
            .post(format!("{}/agent/runs", self.base_url))
            .bearer_auth(token)
            .json(&OpenRunRequest {
                trigger: inbox.trigger.clone().expect("inbox has a trigger"),
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn triage_routed(&self, token: &str, run_id: &str, routed: &str) -> TriagePayload {
        self.http
            .get(format!(
                "{}/agent/inbox-triage/payload?run_id={run_id}&routed={routed}",
                self.base_url
            ))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn report_triage(&self, token: &str, request: &TriageReportRequest) {
        self.http
            .post(format!("{}/agent/triage", self.base_url))
            .bearer_auth(token)
            .json(request)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }

    pub async fn triage(&self, token: &str, run_id: &str) -> TriagePayload {
        let response = self
            .http
            .get(format!(
                "{}/agent/inbox-triage/payload?run_id={run_id}",
                self.base_url
            ))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap();
            panic!("triage failed with {status}: {body}");
        }
        response.json().await.unwrap()
    }

    pub async fn command(&self, token: &str, command: AgentCommand) -> AgentCommandResponse {
        self.command_with_request_id(token, request_id(), command)
            .await
    }

    pub async fn command_with_request_id(
        &self,
        token: &str,
        request_id: String,
        command: AgentCommand,
    ) -> AgentCommandResponse {
        self.http
            .post(format!("{}/agent/commands", self.base_url))
            .bearer_auth(token)
            .json(&AgentCommandRequest {
                request_id,
                command,
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn finish(&self, token: &str, run_id: &str, status: &str) -> RunView {
        self.http
            .post(format!("{}/agent/runs/{run_id}/finish", self.base_url))
            .bearer_auth(token)
            .json(&FinishRunRequest {
                status: status.to_string(),
                input_tokens: Some(1),
                cached_input_tokens: Some(0),
                output_tokens: Some(1),
                error_code: (status != "completed").then(|| "TEST_FAILURE".to_string()),
                error_message: (status != "completed").then(|| "intentional failure".to_string()),
                assistant_text: None,
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn stop(self) {
        self.server.shutdown().await.unwrap();
        self.pool.close().await;
        self.admin
            .execute(format!("DROP DATABASE {} WITH (FORCE)", self.database).as_str())
            .await
            .unwrap();
        self.admin.close().await;
    }
}
