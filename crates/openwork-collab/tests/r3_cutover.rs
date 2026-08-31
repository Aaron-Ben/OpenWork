#![cfg(unix)]

use openwork_collab::{
    protocol::{
        AgentCommand, AgentCommandRequest, AgentCommandResponse, AgentCommandResult,
        AgentTokenResponse, BoardView, DesiredAgents, DesktopCommand, DesktopCommandRequest,
        DesktopCommandResult, FinishRunRequest, InboxResponse, OpenRunRequest, RunView, request_id,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sqlx::{Executor, PgPool};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    admin: PgPool,
    database: String,
    database_url: String,
    redis_url: String,
    pool: PgPool,
    server: openwork_collab::server::ServerHandle,
    http: reqwest::Client,
    base_url: String,
    runtime_session_id: String,
    desktop_secret: String,
    computer_secret: String,
}

impl Fixture {
    async fn start() -> Option<Self> {
        let guard = FIXTURE_LOCK.lock().await;
        let base = std::env::var("TEST_DATABASE_URL").ok()?;
        let redis_url = std::env::var("TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
        let admin = PgPool::connect(&base).await.unwrap();
        let database = format!("collab_r3_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE {database}").as_str())
            .await
            .unwrap();
        let (prefix, _) = base.rsplit_once('/').unwrap();
        let database_url = format!("{prefix}/{database}");
        let legacy = PgPool::connect(&database_url).await.unwrap();
        legacy
            .execute(
                "CREATE TABLE collab_schema_migrations (
                    version BIGINT PRIMARY KEY,
                    description TEXT NOT NULL,
                    installed_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
                 )",
            )
            .await
            .unwrap();
        legacy
            .execute(
                "INSERT INTO collab_schema_migrations (version, description)
                 VALUES (202608300001, 'legacy fixture')",
            )
            .await
            .unwrap();
        legacy
            .execute("CREATE TABLE collab_computers (id TEXT PRIMARY KEY)")
            .await
            .unwrap();
        legacy
            .execute("CREATE TABLE collab_events (id TEXT PRIMARY KEY)")
            .await
            .unwrap();
        legacy.close().await;
        let credentials = RuntimeCredentials::generate();
        let runtime_session_id = credentials.runtime_session_id.clone();
        let desktop_secret = credentials.desktop_secret.clone();
        let computer_secret = credentials.computer_secret.clone();
        let shutdown = CancellationToken::new();
        let server = CollaborationServer::start(
            ServerOptions {
                database_url: database_url.clone(),
                redis_url: redis_url.clone(),
                runtime_bind: "127.0.0.1:0".parse().unwrap(),
                credentials,
            },
            shutdown,
        )
        .await
        .unwrap();
        let base_url = format!("http://{}", server.runtime_addr());
        let pool = PgPool::connect(&database_url).await.unwrap();
        Some(Self {
            _guard: guard,
            admin,
            database,
            database_url,
            redis_url,
            pool,
            server,
            http: reqwest::Client::new(),
            base_url,
            runtime_session_id,
            desktop_secret,
            computer_secret,
        })
    }

    async fn desktop(&self, request: &DesktopCommandRequest) -> reqwest::Response {
        self.http
            .post(format!("{}/desktop/commands", self.base_url))
            .bearer_auth(&self.desktop_secret)
            .json(request)
            .send()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn r3_scopes_credentials_runs_typed_commands_and_rejects_old_sessions() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };

    let create_request = DesktopCommandRequest {
        request_id: Some(request_id()),
        command: DesktopCommand::CreateAgent {
            display_name: "Équipe Démo".to_string(),
            role: Some("Researcher".to_string()),
            persona: "Investigate carefully.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "local/main".to_string(),
            triage_model_id: "local/triage".to_string(),
        },
    };
    let created = fixture
        .desktop(&create_request)
        .await
        .error_for_status()
        .unwrap()
        .json::<DesktopCommandResult>()
        .await
        .unwrap();
    let DesktopCommandResult::Agent(agent) = created else {
        panic!("Agent command returned the wrong result")
    };
    assert_eq!(agent.id, "equipe-demo");
    assert_eq!(agent.config_revision, 1);
    assert!(agent.archived_at.is_none());

    let malformed_request = fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(format!("req-{}", "A".repeat(32))),
            command: DesktopCommand::CreateBoard {
                title: "Malformed request must not run".to_string(),
                description: None,
            },
        })
        .await;
    assert_eq!(malformed_request.status(), reqwest::StatusCode::BAD_REQUEST);

    let replayed = fixture
        .desktop(&create_request)
        .await
        .error_for_status()
        .unwrap()
        .json::<DesktopCommandResult>()
        .await
        .unwrap();
    assert_eq!(replayed, DesktopCommandResult::Agent(agent.clone()));
    let conflicting_replay = fixture
        .desktop(&DesktopCommandRequest {
            request_id: create_request.request_id.clone(),
            command: DesktopCommand::CreateBoard {
                title: "Same key, different command".to_string(),
                description: None,
            },
        })
        .await;
    assert_eq!(conflicting_replay.status(), reqwest::StatusCode::CONFLICT);
    let agent_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM collab_agent_profiles WHERE agent_id = $1")
            .bind(&agent.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(agent_count, 1);

    let wrong_scope = fixture
        .http
        .get(format!("{}/computer/agents", fixture.base_url))
        .bearer_auth(&fixture.desktop_secret)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_scope.status(), reqwest::StatusCode::UNAUTHORIZED);

    fixture
        .http
        .post(format!("{}/computer/heartbeat", fixture.base_url))
        .bearer_auth(&fixture.computer_secret)
        .json(&serde_json::json!({ "activeAgentIds": [] }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let desired = fixture
        .http
        .get(format!("{}/computer/agents", fixture.base_url))
        .bearer_auth(&fixture.computer_secret)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<DesiredAgents>()
        .await
        .unwrap();
    assert_eq!(desired.runtime_session_id, fixture.runtime_session_id);
    assert_eq!(desired.agents.len(), 1);
    assert_eq!(desired.agents[0].main_model_id, "local/main");

    let room = fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(request_id()),
            command: DesktopCommand::CreateDirectRoom {
                agent_id: agent.id.clone(),
            },
        })
        .await
        .error_for_status()
        .unwrap()
        .json::<DesktopCommandResult>()
        .await
        .unwrap();
    let DesktopCommandResult::Room(room) = room else {
        panic!("Room command returned the wrong result")
    };
    assert!(room.id.starts_with("room-"));
    let empty_message = fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(request_id()),
            command: DesktopCommand::SendMessage {
                room_id: room.id.clone(),
                body: "   ".to_string(),
            },
        })
        .await;
    assert_eq!(empty_message.status(), reqwest::StatusCode::BAD_REQUEST);
    fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(request_id()),
            command: DesktopCommand::SendMessage {
                room_id: room.id.clone(),
                body: "Please investigate this.".to_string(),
            },
        })
        .await
        .error_for_status()
        .unwrap();

    let token = fixture
        .http
        .post(format!(
            "{}/computer/agents/{}/token",
            fixture.base_url, agent.id
        ))
        .bearer_auth(&fixture.computer_secret)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentTokenResponse>()
        .await
        .unwrap();
    let inbox = fixture
        .http
        .get(format!("{}/agent/inbox", fixture.base_url))
        .bearer_auth(&token.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    let trigger = inbox.trigger.expect("human message creates a trigger");
    assert_eq!(trigger.runtime_session_id, fixture.runtime_session_id);
    let run = fixture
        .http
        .post(format!("{}/agent/runs", fixture.base_url))
        .bearer_auth(&token.token)
        .json(&OpenRunRequest { trigger })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<RunView>()
        .await
        .unwrap();
    assert!(run.id.starts_with("run-"));

    let command_request = AgentCommandRequest {
        request_id: request_id(),
        command: AgentCommand::Reply {
            room_id: room.id.clone(),
            body: "I am on it.".to_string(),
            held_token: None,
        },
    };
    let first_result = fixture
        .http
        .post(format!("{}/agent/commands", fixture.base_url))
        .bearer_auth(&token.token)
        .json(&command_request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentCommandResponse>()
        .await
        .unwrap();
    assert!(matches!(
        &first_result.result,
        AgentCommandResult::MessagePublished { .. }
    ));
    let replayed_result = fixture
        .http
        .post(format!("{}/agent/commands", fixture.base_url))
        .bearer_auth(&token.token)
        .json(&command_request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentCommandResponse>()
        .await
        .unwrap();
    assert_eq!(first_result, replayed_result);

    fixture
        .http
        .post(format!("{}/agent/runs/{}/finish", fixture.base_url, run.id))
        .bearer_auth(&token.token)
        .json(&FinishRunRequest {
            status: "completed".to_string(),
            input_tokens: Some(10),
            cached_input_tokens: Some(0),
            output_tokens: Some(5),
            error_code: None,
            error_message: None,
            assistant_text: Some("I am on it.".to_string()),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let board = fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(request_id()),
            command: DesktopCommand::CreateBoard {
                title: "Workspace".to_string(),
                description: Some("Shared work".to_string()),
            },
        })
        .await
        .error_for_status()
        .unwrap()
        .json::<DesktopCommandResult>()
        .await
        .unwrap();
    let DesktopCommandResult::Board(BoardView { columns, .. }) = board else {
        panic!("Board command returned the wrong result")
    };
    assert_eq!(columns.len(), 3);
    assert_eq!(
        columns.iter().filter(|column| column.is_terminal).count(),
        1
    );

    let local_user_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM collab_participants
         WHERE id = 'local-user' AND kind = 'user' AND display_name = 'User'",
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(local_user_count, 1);
    assert!(
        sqlx::query(
            "UPDATE collab_participants SET display_name = 'Changed' WHERE id = 'local-user'"
        )
        .execute(&fixture.pool)
        .await
        .is_err()
    );
    for removed in [
        "collab_computers",
        "collab_reactions",
        "collab_events",
        "collab_cli_requests",
    ] {
        let present: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(removed)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
        assert!(!present, "obsolete table {removed} survived reset");
    }

    let archived = fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(request_id()),
            command: DesktopCommand::ArchiveAgent {
                agent_id: agent.id.clone(),
            },
        })
        .await
        .error_for_status()
        .unwrap()
        .json::<DesktopCommandResult>()
        .await
        .unwrap();
    let DesktopCommandResult::Agent(archived) = archived else {
        panic!("archive returned the wrong result")
    };
    assert!(archived.archived_at.is_some());
    let archived_token = fixture
        .http
        .get(format!("{}/agent/inbox", fixture.base_url))
        .bearer_auth(&token.token)
        .send()
        .await
        .unwrap();
    assert_eq!(archived_token.status(), reqwest::StatusCode::UNAUTHORIZED);
    fixture
        .desktop(&DesktopCommandRequest {
            request_id: Some(request_id()),
            command: DesktopCommand::RestoreAgent {
                agent_id: agent.id.clone(),
            },
        })
        .await
        .error_for_status()
        .unwrap();

    let database_url = fixture.database_url.clone();
    let redis_url = fixture.redis_url.clone();
    let old_token = token.token;
    let desktop_secret = fixture.desktop_secret.clone();
    let computer_secret = fixture.computer_secret.clone();
    fixture.server.shutdown().await.unwrap();

    let replacement_credentials = RuntimeCredentials::generate();
    assert_ne!(
        replacement_credentials.runtime_session_id,
        fixture.runtime_session_id
    );
    assert_ne!(replacement_credentials.desktop_secret, desktop_secret);
    assert_ne!(replacement_credentials.computer_secret, computer_secret);
    let replacement = CollaborationServer::start(
        ServerOptions {
            database_url,
            redis_url,
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
            credentials: replacement_credentials,
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let replacement_base = format!("http://{}", replacement.runtime_addr());
    let rejected = fixture
        .http
        .get(format!("{replacement_base}/agent/inbox"))
        .bearer_auth(old_token)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);
    replacement.shutdown().await.unwrap();

    fixture.pool.close().await;
    fixture
        .admin
        .execute(format!("DROP DATABASE {} WITH (FORCE)", fixture.database).as_str())
        .await
        .unwrap();
    fixture.admin.close().await;
}
