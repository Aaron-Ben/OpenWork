#![cfg(target_os = "macos")]

use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use openwork_collab::protocol::{DesktopCommand, DesktopCommandResult, RuntimeStatusView};
use openwork_desktop_lib::CollabDaemonClient;
use redis::AsyncCommands;
use sqlx::{Executor, PgPool};
use uuid::Uuid;

#[tokio::test]
async fn desktop_supervises_and_replaces_the_complete_runtime_process_group() {
    let Ok(admin_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let database = format!("collab_supervisor_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = admin_url.rsplit_once('/').unwrap();
    let database_url = format!("{prefix}/{database}");
    let directory = tempfile::tempdir().unwrap();
    let state_root = directory.path().join(".openwork");
    let fake_opencode = install_fake_opencode(directory.path()).await;

    // This integration-test binary contains one test, so no other thread can
    // observe these process-launch variables while the supervisor inherits them.
    unsafe {
        std::env::set_var("DATABASE_URL", &database_url);
        std::env::set_var("REDIS_URL", &redis_url);
        std::env::set_var("OPENCODE_BIN", &fake_opencode);
    }

    let client = CollabDaemonClient::start(
        state_root.clone(),
        PathBuf::from(env!("CARGO_BIN_EXE_openwork-desktop")),
    )
    .await
    .unwrap();
    let initial = status(&client).await;
    let initial_processes = wait_for_process_group().await;

    let DesktopCommandResult::Agent(agent) = client
        .call(DesktopCommand::CreateAgent {
            display_name: "Runtime Helper".to_string(),
            role: Some("Supervisor verifier".to_string()),
            persona: "Reply clearly through the provided collaboration command.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/test".to_string(),
            triage_model_id: "opencode/test".to_string(),
        })
        .await
        .unwrap()
    else {
        panic!("Agent creation returned the wrong result")
    };
    let DesktopCommandResult::Room(room) = client
        .call(DesktopCommand::CreateDirectRoom {
            agent_id: agent.id.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("Direct Room creation returned the wrong result")
    };
    send_and_wait_for_reply(&client, &room.id, 2).await;
    wait_for_completed_runs(&client, 1).await;

    kill_role(&initial_processes, "server");
    let after_server_crash = wait_for_new_session(&client, &initial.runtime_session_id).await;
    let after_server_processes = wait_for_replaced_process_group(&initial_processes).await;
    assert_process_group_gone(&initial_processes).await;
    send_and_wait_for_reply(&client, &room.id, 4).await;
    wait_for_completed_runs(&client, 2).await;

    kill_role(&after_server_processes, "computer");
    let after_computer_crash =
        wait_for_new_session(&client, &after_server_crash.runtime_session_id).await;
    assert_ne!(
        after_computer_crash.runtime_session_id,
        initial.runtime_session_id
    );
    let after_computer_processes = wait_for_replaced_process_group(&after_server_processes).await;
    assert_process_group_gone(&after_server_processes).await;
    send_and_wait_for_reply(&client, &room.id, 6).await;
    wait_for_completed_runs(&client, 3).await;

    client.shutdown().await;

    assert_process_group_gone(&after_computer_processes).await;
    let runtime_entries = std::fs::read_dir(state_root.join("runtime"))
        .unwrap()
        .count();
    assert_eq!(runtime_entries, 0, "normal exit left a RuntimeSession home");
    let database_pool = PgPool::connect(&database_url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&database_pool)
            .await
            .unwrap(),
        1
    );
    database_pool.close().await;
    let redis = redis::Client::open(redis_url).unwrap();
    let mut redis_connection = redis.get_multiplexed_async_connection().await.unwrap();
    let pong: String = redis_connection.ping().await.unwrap();
    assert_eq!(pong, "PONG");

    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}

async fn install_fake_opencode(directory: &Path) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/openwork-collab/tests/fixtures/fake-opencode.zsh");
    let executable = directory.join("fake-opencode");
    tokio::fs::copy(source, &executable).await.unwrap();
    tokio::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .await
        .unwrap();
    executable
}

async fn status(client: &CollabDaemonClient) -> RuntimeStatusView {
    let DesktopCommandResult::Status(status) = client.call(DesktopCommand::Status).await.unwrap()
    else {
        panic!("status command returned the wrong result")
    };
    status
}

async fn wait_for_new_session(
    client: &CollabDaemonClient,
    previous_session_id: &str,
) -> RuntimeStatusView {
    tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            if let Ok(DesktopCommandResult::Status(status)) =
                client.call(DesktopCommand::Status).await
            {
                if status.runtime_session_id != previous_session_id
                    && status.last_computer_heartbeat.is_some()
                {
                    return status;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("supervisor did not replace the failed RuntimeSession")
}

async fn send_and_wait_for_reply(client: &CollabDaemonClient, room_id: &str, count: usize) {
    client
        .call(DesktopCommand::SendMessage {
            room_id: room_id.to_string(),
            body: format!("Reply through OpenCode for message {count}."),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(DesktopCommandResult::Messages { messages }) = client
                .call(DesktopCommand::ListMessages {
                    room_id: room_id.to_string(),
                })
                .await
            {
                if messages.len() == count {
                    assert!(
                        messages.last().unwrap().body.contains("Agent says"),
                        "unexpected message sequence: {messages:#?}"
                    );
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("fake OpenCode did not settle the durable message");
}

async fn wait_for_completed_runs(client: &CollabDaemonClient, count: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(DesktopCommandResult::Runs { runs }) =
                client.call(DesktopCommand::ListRuns { limit: 100 }).await
            {
                let completed = runs.iter().filter(|run| run.status == "completed").count();
                if completed == count {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("Agent reply was visible before its Run finished");
}

async fn wait_for_process_group() -> BTreeMap<String, i32> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let roles = child_roles();
            if roles.contains_key("server") && roles.contains_key("computer") {
                return roles;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Server and Computer child processes were not visible")
}

async fn wait_for_replaced_process_group(
    previous: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let roles = child_roles();
            if ["server", "computer"].iter().all(|role| {
                roles
                    .get(*role)
                    .zip(previous.get(*role))
                    .is_some_and(|(current, old)| current != old)
            }) {
                return roles;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("supervisor did not replace both child processes")
}

fn child_roles() -> BTreeMap<String, i32> {
    let output = Command::new("/usr/bin/pgrep")
        .args(["-P", &std::process::id().to_string()])
        .output()
        .unwrap();
    if !output.status.success() {
        return BTreeMap::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.parse::<i32>().ok())
        .filter_map(|pid| {
            let output = Command::new("/bin/ps")
                .args(["-p", &pid.to_string(), "-o", "command="])
                .output()
                .ok()?;
            let command = String::from_utf8_lossy(&output.stdout);
            let role = if command.contains("--openwork-collab-server") {
                "server"
            } else if command.contains("--openwork-collab-computer") {
                "computer"
            } else {
                return None;
            };
            Some((role.to_string(), pid))
        })
        .collect()
}

fn kill_role(processes: &BTreeMap<String, i32>, role: &str) {
    let pid = processes[role];
    let result = unsafe { libc::kill(pid, libc::SIGKILL) };
    assert_eq!(result, 0, "failed to kill {role} process {pid}");
}

async fn assert_process_group_gone(processes: &BTreeMap<String, i32>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if processes.values().all(|pid| !process_exists(*pid)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("a retired collaboration child process survived");
}

fn process_exists(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
