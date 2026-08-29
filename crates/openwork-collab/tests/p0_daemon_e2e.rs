#![cfg(target_os = "macos")]

use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};

use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        opencode::OpenCodeAdapter,
    },
    protocol::{ControlRequest, ControlResponse},
    server::{CollaborationServer, ServerOptions, control::request},
};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn create_database() -> Option<(PgPool, String, String)> {
    let base = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&base).await.unwrap();
    let database = format!("collab_p0_daemon_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = base.rsplit_once('/').unwrap();
    Some((admin, database.clone(), format!("{prefix}/{database}")))
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
async fn computer_daemon_drives_opencode_through_the_shim_to_a_settled_reply() {
    let Some((admin, database, database_url)) = create_database().await else {
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let socket = state.path().join("control.sock");
    let server_shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url,
            state_root: state.path().join("server"),
            control_socket: socket.clone(),
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
        },
        server_shutdown.clone(),
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
    request(
        &socket,
        &ControlRequest::CreateAgent {
            id: "helper".to_string(),
            display_name: "Helper".to_string(),
            system_prompt: "Reply clearly.".to_string(),
        },
    )
    .await
    .unwrap();
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
    request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id.clone(),
            body: "Please answer through OpenCode.".to_string(),
        },
    )
    .await
    .unwrap();

    let fake_opencode = state.path().join("fake-opencode");
    tokio::fs::write(
        &fake_opencode,
        r#"#!/bin/zsh
if [[ "$1" == "--version" ]]; then
  print -r -- "opencode test"
  exit 0
fi
if [[ "$1 $2" == "run --help" ]]; then
  print -r -- "--pure --format --auto --model --session"
  exit 0
fi
if [[ "$1 $2" == "auth list" ]]; then
  print -r -- "1 credentials"
  exit 0
fi
prompt="$(cat)"
room_id="$(print -r -- "$prompt" | sed -n 's/^room_id: //p' | head -n 1)"
printf '%s\n%s' 'Agent says `code` $(literal) --as=admin' 'second line' | openwork reply "$room_id" --stdin >/dev/null || exit $?
print -r -- '{"type":"text","sessionID":"ses_helper","part":{"text":"published"}}'
print -r -- '{"type":"step_finish","sessionID":"ses_helper","part":{"tokens":{"input":8,"output":3,"cache":{"read":1,"write":0}}}}'
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&fake_opencode)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&fake_opencode, permissions)
        .await
        .unwrap();

    let daemon_shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            state_root: state.path().join("computer"),
            runtime_base_url: format!("http://{}", server.runtime_addr()),
            device_token: registration.device_token.unwrap(),
            shim_executable: Path::new(env!("CARGO_BIN_EXE_openwork")).to_path_buf(),
            supervised: false,
            poll_interval: Duration::from_millis(25),
            roster_interval: Duration::from_millis(100),
        },
        Arc::new(OpenCodeAdapter::with_executable(fake_opencode)),
    );
    let daemon_shutdown_for_task = daemon_shutdown.clone();
    let daemon_task = tokio::spawn(async move { daemon.run(daemon_shutdown_for_task).await });

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
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
            if messages.len() == 2 {
                assert_eq!(messages[1].author_id, "helper");
                assert_eq!(
                    messages[1].body,
                    "Agent says `code` $(literal) --as=admin\nsecond line"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();

    let agent_home = state.path().join("computer/agents/helper");
    let standing_prompt = tokio::fs::read_to_string(agent_home.join("AGENTS.md"))
        .await
        .unwrap();
    assert!(standing_prompt.contains("Reply clearly."));
    assert!(standing_prompt.contains("`openwork` CLI"));
    assert!(!standing_prompt.to_ascii_lowercase().contains("mcp"));
    let token_metadata = tokio::fs::metadata(agent_home.join("bin/.runtime-token"))
        .await
        .unwrap();
    assert_eq!(token_metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        tokio::fs::read_to_string(state.path().join("computer/sessions/helper.session"))
            .await
            .unwrap(),
        "ses_helper"
    );

    daemon_shutdown.cancel();
    daemon_task.await.unwrap().unwrap();
    server_shutdown.cancel();
    server.shutdown().await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}
