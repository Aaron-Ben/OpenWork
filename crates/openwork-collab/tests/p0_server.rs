#![cfg(unix)]

use std::{path::Path, time::Duration};

use openwork_collab::{
    protocol::{
        AgentRoster, AgentTokenResponse, COLLAB_PROTOCOL_VERSION, CliRequest, CliResult,
        ControlRequest, ControlResponse, DeviceStartResponse, EngineInventoryView, EngineStatus,
        FinishRunRequest, HeartbeatRequest, InboxResponse, OpenRunRequest, RunView,
    },
    server::{CollaborationServer, ServerError, ServerOptions, control::request},
};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn create_database() -> Option<(PgPool, String, String)> {
    let base = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&base).await.unwrap();
    let database = format!("collab_p0_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = base.rsplit_once('/').unwrap();
    Some((admin, database.clone(), format!("{prefix}/{database}")))
}

#[tokio::test]
async fn runtime_opens_a_delivery_publishes_a_reply_and_settles_the_message() {
    let Some((admin, database, database_url)) = create_database().await else {
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let socket = state.path().join("control.sock");
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url: database_url.clone(),
            redis_url: "redis://127.0.0.1:6379".to_string(),
            state_root: state.path().to_path_buf(),
            control_socket: socket.clone(),
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
        },
        shutdown.clone(),
    )
    .await
    .unwrap();
    wait_for_socket(&socket).await;

    let ControlResponse::LocalComputer(registration) =
        request(&socket, &ControlRequest::EnsureLocalComputer)
            .await
            .unwrap()
    else {
        panic!("local computer registration failed")
    };
    let device_token = registration.device_token.unwrap();
    let ControlResponse::Agent(agent) = request(
        &socket,
        &ControlRequest::CreateAgent {
            id: "helper".to_string(),
            display_name: "Helper".to_string(),
            system_prompt: "Help the user.".to_string(),
            model: "opencode/mimo-v2.5-free".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("agent creation failed")
    };
    assert_eq!(agent.model, "opencode/mimo-v2.5-free");
    let ControlResponse::Room(room) = request(
        &socket,
        &ControlRequest::CreateDirectRoom {
            agent_id: "helper".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("room creation failed")
    };
    let ControlResponse::Message(_) = request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id.clone(),
            body: "Can you help?".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("message creation failed")
    };

    let client = reqwest::Client::new();
    let base = format!("http://{}", server.runtime_addr());
    let started = client
        .post(format!("{base}/api/computers/me/start"))
        .bearer_auth(&device_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<DeviceStartResponse>()
        .await
        .unwrap();
    client
        .post(format!("{base}/api/computers/me/heartbeat"))
        .bearer_auth(&device_token)
        .json(&HeartbeatRequest {
            protocol_version: COLLAB_PROTOCOL_VERSION,
            generation: started.generation,
            daemon_version: "test".to_string(),
            supervised: false,
            status: openwork_collab::protocol::ComputerStatus::Online,
            engine: EngineInventoryView {
                engine_id: "opencode".to_string(),
                status: EngineStatus::Ready,
            },
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let roster = client
        .get(format!(
            "{base}/api/computers/me/agents?generation={}",
            started.generation
        ))
        .bearer_auth(&device_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentRoster>()
        .await
        .unwrap();
    assert_eq!(roster.agents.len(), 1);
    assert_eq!(roster.agents[0].model, "opencode/mimo-v2.5-free");
    let token = client
        .post(format!("{base}/api/computers/me/agents/helper/token"))
        .bearer_auth(&device_token)
        .json(&serde_json::json!({"generation": started.generation}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentTokenResponse>()
        .await
        .unwrap();
    let inbox = client
        .get(format!("{base}/runtime/inbox"))
        .bearer_auth(&token.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert_eq!(inbox.messages[0].body, "Can you help?");
    let trigger = inbox.trigger.unwrap();
    let run = client
        .post(format!("{base}/runtime/runs"))
        .bearer_auth(&token.token)
        .json(&OpenRunRequest {
            trigger: trigger.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<RunView>()
        .await
        .unwrap();
    assert_eq!(run.id, trigger.dispatch_id);
    let cli = client
        .post(format!("{base}/runtime/cli"))
        .bearer_auth(&token.token)
        .json(&CliRequest {
            request_id: "cli_p0_reply".to_string(),
            argv: vec![
                "reply".to_string(),
                room.id.clone(),
                "--".to_string(),
                "Of course.".to_string(),
            ],
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<CliResult>()
        .await
        .unwrap();
    assert_eq!(cli.exit_code, 0);
    let finish_request = FinishRunRequest {
        status: "completed".to_string(),
        input_tokens: Some(10),
        cached_input_tokens: Some(2),
        output_tokens: Some(4),
        error_code: None,
        error_message: None,
        assistant_text: Some("done".to_string()),
    };
    client
        .post(format!("{base}/runtime/runs/{}/finish", run.id))
        .bearer_auth(&token.token)
        .json(&finish_request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let retried_finish = client
        .post(format!("{base}/runtime/runs/{}/finish", run.id))
        .bearer_auth(&token.token)
        .json(&finish_request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<RunView>()
        .await
        .unwrap();
    assert_eq!(retried_finish.status, "completed");

    let ControlResponse::Messages { messages } = request(
        &socket,
        &ControlRequest::ListMessages {
            room_id: room.id.clone(),
        },
    )
    .await
    .unwrap() else {
        panic!("message listing failed")
    };
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].author_id, "helper");
    assert_eq!(messages[1].body, "Of course.");
    let test_pool = PgPool::connect(&database_url).await.unwrap();
    let run_model: Option<String> =
        sqlx::query_scalar("SELECT model FROM collab_runs WHERE id = $1")
            .bind(&run.id)
            .fetch_one(&test_pool)
            .await
            .unwrap();
    assert_eq!(run_model.as_deref(), Some("opencode/mimo-v2.5-free"));
    let last_read_seq: i64 = sqlx::query_scalar(
        "SELECT last_read_seq FROM collab_room_members
         WHERE room_id = $1 AND participant_id = 'helper'",
    )
    .bind(&room.id)
    .fetch_one(&test_pool)
    .await
    .unwrap();
    assert_eq!(last_read_seq, 1);

    request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id.clone(),
            body: "One more question.".to_string(),
        },
    )
    .await
    .unwrap();
    let next_inbox = client
        .get(format!("{base}/runtime/inbox"))
        .bearer_auth(&token.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    let stale_trigger = next_inbox.trigger.unwrap();
    let stale_dispatch_id = stale_trigger.dispatch_id.clone();
    let mut fence = test_pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM collab_computers WHERE id = 'local' FOR UPDATE")
        .execute(&mut *fence)
        .await
        .unwrap();
    let stale_client = client.clone();
    let stale_base = base.clone();
    let stale_token = token.token.clone();
    let stale_request = tokio::spawn(async move {
        stale_client
            .post(format!("{stale_base}/runtime/runs"))
            .bearer_auth(stale_token)
            .json(&OpenRunRequest {
                trigger: stale_trigger,
            })
            .send()
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    sqlx::query(
        "UPDATE collab_computers
         SET daemon_generation = daemon_generation + 1
         WHERE id = 'local'",
    )
    .execute(&mut *fence)
    .await
    .unwrap();
    fence.commit().await.unwrap();
    let stale_response = stale_request.await.unwrap();
    assert_eq!(stale_response.status(), reqwest::StatusCode::CONFLICT);
    let stale_run_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collab_runs WHERE id = $1)")
            .bind(stale_dispatch_id)
            .fetch_one(&test_pool)
            .await
            .unwrap();
    assert!(!stale_run_exists);
    test_pool.close().await;

    shutdown.cancel();
    server.shutdown().await.unwrap();
    drop_database(admin, &database).await;
}

async fn drop_database(admin: PgPool, database: &str) {
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}

async fn wait_for_socket(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn runtime_rejects_a_non_loopback_bind_before_opening_external_resources() {
    let state = tempfile::tempdir().unwrap();
    let result = CollaborationServer::start(
        ServerOptions {
            database_url: "postgres://unused".to_string(),
            redis_url: "redis://127.0.0.1:6379".to_string(),
            state_root: state.path().to_path_buf(),
            control_socket: state.path().join("control.sock"),
            runtime_bind: "0.0.0.0:0".parse().unwrap(),
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
        },
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(result, Err(ServerError::NonLoopbackBind(_))));
}

#[tokio::test]
async fn owner_control_stops_the_server_for_a_launchd_handoff() {
    let Some((admin, database, database_url)) = create_database().await else {
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let socket = state.path().join("control.sock");
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url,
            redis_url: "redis://127.0.0.1:6379".to_string(),
            state_root: state.path().to_path_buf(),
            control_socket: socket.clone(),
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
        },
        shutdown,
    )
    .await
    .unwrap();
    wait_for_socket(&socket).await;

    let response = request(&socket, &ControlRequest::ShutdownServer)
        .await
        .unwrap();
    assert_eq!(response, ControlResponse::Acknowledged);
    tokio::time::timeout(Duration::from_secs(5), server.shutdown())
        .await
        .expect("Server stopped after owner handoff")
        .unwrap();
    assert!(!socket.exists());

    drop_database(admin, &database).await;
}

#[tokio::test]
async fn control_ensures_the_single_local_computer_and_returns_its_secret_once() {
    let Some((admin, database, database_url)) = create_database().await else {
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let socket = state.path().join("control.sock");
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url,
            redis_url: "redis://127.0.0.1:6379".to_string(),
            state_root: state.path().to_path_buf(),
            control_socket: socket.clone(),
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
        },
        shutdown.clone(),
    )
    .await
    .unwrap();
    wait_for_socket(&socket).await;
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        tokio::fs::metadata(&socket)
            .await
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
    );

    let first = request(&socket, &ControlRequest::EnsureLocalComputer)
        .await
        .unwrap();
    let second = request(&socket, &ControlRequest::EnsureLocalComputer)
        .await
        .unwrap();

    let ControlResponse::LocalComputer(first) = first else {
        panic!("unexpected first response");
    };
    let ControlResponse::LocalComputer(second) = second else {
        panic!("unexpected second response");
    };
    assert_eq!(first.computer.id, "local");
    assert_eq!(first.computer.engine_id, "opencode");
    assert!(first.device_token.is_some());
    assert!(second.device_token.is_none());

    shutdown.cancel();
    server.shutdown().await.unwrap();
    drop_database(admin, &database).await;
}
