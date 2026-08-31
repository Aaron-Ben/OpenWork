#![cfg(target_os = "macos")]

mod support;

use std::{path::PathBuf, time::Duration};

use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        engine::EngineRegistry,
        opencode::OpenCodeAdapter,
    },
    protocol::{DesktopCommand, DesktopCommandRequest, DesktopCommandResult, request_id},
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn desktop_command(
    http: &reqwest::Client,
    base_url: &str,
    desktop_secret: &str,
    command: DesktopCommand,
) -> DesktopCommandResult {
    let request = DesktopCommandRequest {
        request_id: command.is_mutating().then(request_id),
        command,
    };
    http.post(format!("{base_url}/desktop/commands"))
        .bearer_auth(desktop_secret)
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn run_smoke(
    engine_executable: Option<PathBuf>,
    model: &str,
    prompt: &str,
    expected_body: &str,
    exact_body: bool,
    reply_timeout: Duration,
) {
    let Ok(base) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
    let admin = PgPool::connect(&base).await.unwrap();
    let database = format!("collab_runtime_e2e_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = base.rsplit_once('/').unwrap();
    let database_url = format!("{prefix}/{database}");

    let credentials = RuntimeCredentials::generate();
    let runtime_session_id = credentials.runtime_session_id.clone();
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
    let http = reqwest::Client::new();

    let DesktopCommandResult::Agent(agent) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::CreateAgent {
            display_name: "Helper".to_string(),
            role: Some("Assistant".to_string()),
            persona: "Reply clearly.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: model.to_string(),
            triage_model_id: model.to_string(),
        },
    )
    .await
    else {
        panic!("Agent creation returned the wrong result")
    };
    let DesktopCommandResult::Room(room) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::CreateDirectRoom {
            agent_id: agent.id.clone(),
        },
    )
    .await
    else {
        panic!("Room creation returned the wrong result")
    };

    let state = tempfile::tempdir().unwrap();
    let engine_executable = match engine_executable {
        Some(executable) => executable,
        None => support::fake_opencode(&state).await,
    };
    let daemon_shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            openwork_root: state.path().join(".openwork"),
            runtime_session_id,
            runtime_base_url: base_url.clone(),
            computer_secret,
            shim_executable: std::path::PathBuf::from(env!("CARGO_BIN_EXE_openwork")),
            poll_interval: Duration::from_millis(100),
            roster_interval: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(100),
            engine_rescan_interval: Duration::from_millis(250),
        },
        EngineRegistry::single(OpenCodeAdapter::with_executable(engine_executable)),
    );
    let daemon_task_shutdown = daemon_shutdown.clone();
    let daemon_task = tokio::spawn(async move { daemon.run(daemon_task_shutdown).await });

    desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SendMessage {
            room_id: room.id.clone(),
            body: prompt.to_string(),
        },
    )
    .await;

    tokio::time::timeout(reply_timeout, async {
        loop {
            let DesktopCommandResult::Messages { messages } = desktop_command(
                &http,
                &base_url,
                &desktop_secret,
                DesktopCommand::ListMessages {
                    room_id: room.id.clone(),
                },
            )
            .await
            else {
                panic!("Message listing returned the wrong result")
            };
            if messages.len() == 2 {
                assert_eq!(messages[1].author_id, agent.id);
                if exact_body {
                    assert_eq!(messages[1].body, expected_body);
                } else {
                    assert!(messages[1].body.contains(expected_body));
                }
                break;
            }
            assert!(messages.len() < 2, "Agent published more than one reply");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();

    let pool = PgPool::connect(&database_url).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let completed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM collab_runs
                 WHERE agent_id = $1 AND status = 'completed'",
            )
            .bind(&agent.id)
            .fetch_one(&pool)
            .await
            .unwrap();
            if completed == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    pool.close().await;

    daemon_shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(20), daemon_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    server.shutdown().await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn desktop_server_computer_and_fake_opencode_settle_a_reply() {
    run_smoke(
        None,
        "opencode/test",
        "Please answer through OpenCode.",
        "Agent says `code` $(literal) --as=admin\nsecond line",
        true,
        Duration::from_secs(12),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires explicit authorization for an external OpenCode model request"]
async fn desktop_server_computer_and_real_opencode_smoke() {
    assert_eq!(
        std::env::var("OPENWORK_REAL_OPENCODE_SMOKE").as_deref(),
        Ok("1"),
        "set OPENWORK_REAL_OPENCODE_SMOKE=1 to confirm the external model request"
    );
    let executable = std::env::var_os("OPENCODE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("opencode"));
    let model = std::env::var("OPENWORK_REAL_OPENCODE_MODEL")
        .unwrap_or_else(|_| "deepseek/deepseek-v4-flash".to_string());
    run_smoke(
        Some(executable),
        &model,
        "Use the openwork reply command to reply with exactly: OpenWork real smoke OK",
        "OpenWork real smoke OK",
        false,
        Duration::from_secs(180),
    )
    .await;
}
