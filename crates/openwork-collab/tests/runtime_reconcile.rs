#![cfg(target_os = "macos")]

mod support;

use std::time::Duration;

use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        engine::EngineRegistry,
        opencode::OpenCodeAdapter,
    },
    protocol::{
        DesktopCommand, DesktopCommandRequest, DesktopCommandResult, RunnerState,
        RuntimeStatusView, request_id,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sha2::{Digest, Sha256};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn desktop_command(
    http: &reqwest::Client,
    base_url: &str,
    desktop_secret: &str,
    command: DesktopCommand,
) -> DesktopCommandResult {
    http.post(format!("{base_url}/desktop/commands"))
        .bearer_auth(desktop_secret)
        .json(&DesktopCommandRequest {
            request_id: command.is_mutating().then(request_id),
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

async fn wait_for_status(
    http: &reqwest::Client,
    base_url: &str,
    desktop_secret: &str,
    predicate: impl Fn(&RuntimeStatusView) -> bool,
) -> RuntimeStatusView {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let DesktopCommandResult::Status(status) =
                desktop_command(http, base_url, desktop_secret, DesktopCommand::Status).await
            else {
                panic!("status command returned the wrong result")
            };
            if predicate(&status) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("R4 actual state did not converge")
}

async fn wait_for_file(path: &std::path::Path, predicate: impl Fn(&str) -> bool) -> String {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(content) = tokio::fs::read_to_string(path).await
                && predicate(&content)
            {
                return content;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("R4 filesystem state did not converge")
}

async fn seed_compatible_session(
    state_file: &std::path::Path,
    managed_context_file: &std::path::Path,
    model: &str,
) {
    let managed_context = tokio::fs::read(managed_context_file).await.unwrap();
    let context_fingerprint = format!("sha256:{:x}", Sha256::digest(&managed_context));
    tokio::fs::write(
        state_file,
        serde_json::to_vec(&serde_json::json!({
            "engine_id": "opencode",
            "model": model,
            "context_fingerprint": context_fingerprint,
            "session_id": "ses_before_context_change",
            "updated_at": "2026-08-31T00:00:00Z",
        }))
        .unwrap(),
    )
    .await
    .unwrap();
}

async fn wait_for_missing(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if !tokio::fs::try_exists(path).await.unwrap() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("incompatible Engine session metadata was not removed");
}

#[tokio::test]
async fn desired_state_reconciles_without_restart_and_keeps_persistent_home() {
    let Ok(base) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
    let admin = PgPool::connect(&base).await.unwrap();
    let database = format!("collab_reconcile_{}", Uuid::new_v4().simple());
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
    let pool = PgPool::connect(&database_url).await.unwrap();

    let DesktopCommandResult::Agent(helper) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::CreateAgent {
            display_name: "Helper".to_string(),
            role: Some("Researcher".to_string()),
            persona: "Initial persona.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/main-v1".to_string(),
            triage_model_id: "opencode/triage-v1".to_string(),
        },
    )
    .await
    else {
        panic!("Agent creation returned the wrong result")
    };

    sqlx::query(
        "INSERT INTO collab_engine_inventory (
            engine_id, status, version, checked_at, last_error, observed_session_id
         ) VALUES ('opencode', 'ready', NULL, CURRENT_TIMESTAMP, NULL, 'runtime-stale')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let DesktopCommandResult::Status(stale_status) =
        desktop_command(&http, &base_url, &desktop_secret, DesktopCommand::Status).await
    else {
        panic!("status command returned the wrong result")
    };
    assert_eq!(stale_status.engines[0].observed_session_id, "runtime-stale");
    assert!(stale_status.engine_readiness.is_empty());
    assert!(stale_status.runners.is_empty());
    let DesktopCommandResult::Agent(broken) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::CreateAgent {
            display_name: "Broken".to_string(),
            role: None,
            persona: "Recover after home repair.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/main-v1".to_string(),
            triage_model_id: "opencode/triage-v1".to_string(),
        },
    )
    .await
    else {
        panic!("Agent creation returned the wrong result")
    };

    let directory = tempfile::tempdir().unwrap();
    let openwork_root = directory.path().join(".openwork");
    tokio::fs::create_dir_all(openwork_root.join("agents"))
        .await
        .unwrap();
    tokio::fs::write(openwork_root.join("agents").join(&broken.id), b"blocked")
        .await
        .unwrap();
    let executable = support::fake_opencode(&directory).await;
    let daemon_shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            openwork_root: openwork_root.clone(),
            runtime_session_id: runtime_session_id.clone(),
            runtime_base_url: base_url.clone(),
            computer_secret,
            shim_executable: std::path::PathBuf::from(env!("CARGO_BIN_EXE_openwork")),
            poll_interval: Duration::from_millis(100),
            roster_interval: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(100),
            engine_rescan_interval: Duration::from_millis(250),
        },
        EngineRegistry::single(OpenCodeAdapter::with_executable(executable)),
    );
    let task_shutdown = daemon_shutdown.clone();
    let daemon_task = tokio::spawn(async move { daemon.run(task_shutdown).await });

    let initial = wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status
            .runners
            .iter()
            .any(|runner| runner.agent_id == helper.id && runner.state == RunnerState::Running)
            && status
                .runners
                .iter()
                .any(|runner| runner.agent_id == broken.id && runner.state == RunnerState::Error)
    })
    .await;
    assert_eq!(initial.engine_readiness.len(), 1);
    assert_eq!(
        initial.engine_readiness[0].status,
        openwork_collab::protocol::EngineStatus::Ready
    );

    let helper_home = openwork_root.join("agents").join(&helper.id);
    let helper_context = helper_home.join("AGENTS.md");
    let helper_session = helper_home.join("engines/opencode/session.json");
    let runtime_root = openwork_root.join("runtime").join(&runtime_session_id);
    assert!(helper_home.join("AGENTS.md").is_file());
    assert!(helper_home.join("work").is_dir());
    assert!(helper_home.join("engines/opencode").is_dir());
    assert!(runtime_root.join("bin/openwork").exists());
    assert!(runtime_root.join("agents/helper/runtime-token").is_file());
    assert!(runtime_root.join("derived/helper/opencode").is_dir());
    for obsolete in ["bin", "memory", "notes", "skills", "workspace"] {
        assert!(!helper_home.join(obsolete).exists(), "created {obsolete}");
    }
    tokio::fs::write(helper_home.join("work/keep.txt"), b"persistent")
        .await
        .unwrap();

    tokio::fs::remove_file(openwork_root.join("agents").join(&broken.id))
        .await
        .unwrap();
    wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status
            .runners
            .iter()
            .any(|runner| runner.agent_id == broken.id && runner.state == RunnerState::Running)
    })
    .await;

    seed_compatible_session(&helper_session, &helper_context, "opencode/main-v1").await;

    let DesktopCommandResult::Agent(updated) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::UpdateAgent {
            agent_id: helper.id.clone(),
            display_name: "Helper".to_string(),
            role: Some("Investigator".to_string()),
            persona: "Updated through management SSE.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/main-v2".to_string(),
            triage_model_id: "opencode/triage-v2".to_string(),
        },
    )
    .await
    else {
        panic!("Agent update returned the wrong result")
    };
    assert_eq!(updated.id, helper.id);
    assert_eq!(updated.config_revision, 2);
    wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status.runners.iter().any(|runner| {
            runner.agent_id == helper.id
                && runner.config_revision == updated.config_revision
                && runner.state == RunnerState::Running
        })
    })
    .await;
    wait_for_missing(&helper_session).await;
    wait_for_file(&helper_home.join("AGENTS.md"), |content| {
        content.contains("Updated through management SSE.")
    })
    .await;

    seed_compatible_session(&helper_session, &helper_context, "opencode/main-v2").await;

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE collab_agent_profiles SET persona = 'Recovered by full snapshot' WHERE agent_id = $1",
    )
    .bind(&helper.id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE collab_agent_runtime_configs SET config_revision = config_revision + 1 WHERE agent_id = $1",
    )
    .bind(&helper.id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status.runners.iter().any(|runner| {
            runner.agent_id == helper.id
                && runner.config_revision == 3
                && runner.state == RunnerState::Running
        })
    })
    .await;
    wait_for_missing(&helper_session).await;
    wait_for_file(&helper_home.join("AGENTS.md"), |content| {
        content.contains("Recovered by full snapshot")
    })
    .await;

    let DesktopCommandResult::Agent(archived) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::ArchiveAgent {
            agent_id: helper.id.clone(),
        },
    )
    .await
    else {
        panic!("archive returned the wrong result")
    };
    assert!(archived.archived_at.is_some());
    wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status
            .runners
            .iter()
            .all(|runner| runner.agent_id != helper.id)
    })
    .await;
    assert_eq!(
        tokio::fs::read(helper_home.join("work/keep.txt"))
            .await
            .unwrap(),
        b"persistent"
    );

    let DesktopCommandResult::Agent(restored) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::RestoreAgent {
            agent_id: helper.id.clone(),
        },
    )
    .await
    else {
        panic!("restore returned the wrong result")
    };
    wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status.runners.iter().any(|runner| {
            runner.agent_id == helper.id
                && runner.config_revision == restored.config_revision
                && runner.state == RunnerState::Running
        })
    })
    .await;

    let DesktopCommandResult::Agent(newcomer) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::CreateAgent {
            display_name: "Newcomer".to_string(),
            role: None,
            persona: "Start without restarting Desktop.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/main-v2".to_string(),
            triage_model_id: "opencode/triage-v2".to_string(),
        },
    )
    .await
    else {
        panic!("new Agent creation returned the wrong result")
    };
    wait_for_status(&http, &base_url, &desktop_secret, |status| {
        status
            .runners
            .iter()
            .any(|runner| runner.agent_id == newcomer.id && runner.state == RunnerState::Running)
    })
    .await;

    daemon_shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(20), daemon_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    server.shutdown().await.unwrap();
    pool.close().await;
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}
