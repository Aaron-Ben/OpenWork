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
            database_url: database_url.clone(),
            redis_url: std::env::var("TEST_REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string()),
            state_root: state.path().join("server"),
            control_socket: socket.clone(),
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
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
            model: "opencode/mimo-v2.5-free".to_string(),
        },
    )
    .await
    .unwrap();
    request(
        &socket,
        &ControlRequest::CreateAgent {
            id: "helper_two".to_string(),
            display_name: "Helper Two".to_string(),
            system_prompt: "Reply as the second Agent.".to_string(),
            model: "opencode/mimo-v2.5-free".to_string(),
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
    let ControlResponse::Room(room_two) = request(
        &socket,
        &ControlRequest::CreateDirectRoom {
            agent_id: "helper_two".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("second room creation failed")
    };
    let fake_opencode = state.path().join("fake-opencode");
    tokio::fs::write(
        &fake_opencode,
        r#"#!/bin/zsh
prompt="$(cat)"
if [[ " $* " == *" --agent openwork-triage "* ]]; then
  if [[ "$prompt" == *"FYI only."* ]]; then
    print -r -- '{"type":"text","part":{"text":"{\"actionable\":false,\"reason\":\"informational only\",\"promptNote\":\"\"}"}}'
  else
    print -r -- '{"type":"text","part":{"text":"{\"actionable\":true,\"reason\":\"direct human request\",\"promptNote\":\"answer the request\"}"}}'
  fi
  print -r -- '{"type":"step_finish","part":{"tokens":{"input":4,"output":2,"cache":{"read":0,"write":0}}}}'
  exit 0
fi
if [[ "$prompt" == *"Resume please."* && " $* " == *" --session "* ]]; then
  print -r -- '{"type":"error","error":{"message":"session not found"}}'
  exit 1
fi
room_id="$(print -r -- "$prompt" | sed -n 's/^room_id: //p' | head -n 1)"
printf '%s\n%s' 'Agent says `code` $(literal) --as=admin' 'second line' | openwork reply "$room_id" --stdin >/dev/null || exit $?
session_id="ses_helper"
if [[ "$prompt" == *"Resume please."* ]]; then
  session_id="ses_rebuilt"
fi
print -r -- "{\"type\":\"text\",\"sessionID\":\"$session_id\",\"part\":{\"text\":\"published\"}}"
print -r -- "{\"type\":\"step_finish\",\"sessionID\":\"$session_id\",\"part\":{\"tokens\":{\"input\":8,\"output\":3,\"cache\":{\"read\":1,\"write\":0}}}}"
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
            poll_interval: Duration::from_secs(30),
            roster_interval: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(100),
            engine_rescan_interval: Duration::from_millis(250),
        },
        Arc::new(OpenCodeAdapter::with_executable(fake_opencode)),
    );
    let daemon_shutdown_for_task = daemon_shutdown.clone();
    let daemon_task = tokio::spawn(async move { daemon.run(daemon_shutdown_for_task).await });

    let token_file = state
        .path()
        .join("computer/agents/helper/bin/.runtime-token");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !token_file.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let second_token_file = state
        .path()
        .join("computer/agents/helper_two/bin/.runtime-token");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !second_token_file.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id.clone(),
            body: "Please answer through OpenCode.".to_string(),
        },
    )
    .await
    .unwrap();
    request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room_two.id.clone(),
            body: "Second Agent, please answer.".to_string(),
        },
    )
    .await
    .unwrap();

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

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let ControlResponse::Messages { messages } = request(
                &socket,
                &ControlRequest::ListMessages {
                    room_id: room_two.id.clone(),
                },
            )
            .await
            .unwrap() else {
                panic!("second room message listing failed")
            };
            if messages.len() == 2 {
                assert_eq!(messages[1].author_id, "helper_two");
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();

    let roster_pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::query(
        "UPDATE collab_agents
         SET system_prompt = 'Updated second Agent persona.',
             config_version = config_version + 1
         WHERE id = 'helper_two'",
    )
    .execute(&roster_pool)
    .await
    .unwrap();
    let second_prompt_file = state.path().join("computer/agents/helper_two/AGENTS.md");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let prompt = tokio::fs::read_to_string(&second_prompt_file)
                .await
                .unwrap();
            if prompt.contains("Updated second Agent persona.") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    roster_pool.close().await;

    request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id.clone(),
            body: "FYI only.".to_string(),
        },
    )
    .await
    .unwrap();
    let test_pool = PgPool::connect(&database_url).await.unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let last_read_seq: i64 = sqlx::query_scalar(
                "SELECT last_read_seq FROM collab_room_members
                 WHERE room_id = $1 AND participant_id = 'helper'",
            )
            .bind(&room.id)
            .fetch_one(&test_pool)
            .await
            .unwrap();
            if last_read_seq == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
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
    assert_eq!(messages.len(), 3, "triage=false must skip the main turn");
    let false_triages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM collab_triages
         WHERE agent_id = 'helper' AND actionable = FALSE AND source = 'local_model'",
    )
    .fetch_one(&test_pool)
    .await
    .unwrap();
    assert_eq!(false_triages, 1);
    test_pool.close().await;

    request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id.clone(),
            body: "Resume please.".to_string(),
        },
    )
    .await
    .unwrap();
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
            if messages.len() == 5 {
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
    let session_file = state.path().join("computer/sessions/helper.session");
    let session: serde_json::Value = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match tokio::fs::read_to_string(&session_file).await {
                Ok(session) => {
                    let session: serde_json::Value = serde_json::from_str(&session).unwrap();
                    if session["session_id"] == "ses_rebuilt" {
                        break session;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("session could not be read: {error}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(session["engine_id"], "opencode");
    assert_eq!(session["model"], "opencode/mimo-v2.5-free");
    assert_eq!(session["session_id"], "ses_rebuilt");
    assert!(
        session["persona_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );

    daemon_shutdown.cancel();
    daemon_task.await.unwrap().unwrap();
    let test_pool = PgPool::connect(&database_url).await.unwrap();
    let computer_status: String =
        sqlx::query_scalar("SELECT status FROM collab_computers WHERE id = 'local'")
            .fetch_one(&test_pool)
            .await
            .unwrap();
    assert_eq!(computer_status, "offline");
    test_pool.close().await;
    server_shutdown.cancel();
    server.shutdown().await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}
