#![cfg(target_os = "macos")]

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        engine::{
            AgentEngineRuntime, ClassifyRequest, ClassifyResult, EngineAdapter, EngineAvailability,
            EngineError, EngineId, EngineInventory, EngineRegistry, EngineRuntimeConfig,
            TurnRequest, TurnResult,
        },
    },
    protocol::{DesktopCommand, DesktopCommandRequest, DesktopCommandResult, request_id},
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
struct CrashOnceAdapter {
    crashed: Arc<AtomicBool>,
    runtimes_created: Arc<AtomicUsize>,
}

struct CrashOnceRuntime {
    crashed: Arc<AtomicBool>,
}

#[async_trait]
impl EngineAdapter for CrashOnceAdapter {
    fn id(&self) -> EngineId {
        EngineId::new("crash-once").unwrap()
    }

    async fn probe(&self) -> Result<EngineInventory, EngineError> {
        Ok(EngineInventory {
            availability: EngineAvailability::Available,
        })
    }

    async fn classify(&self, _request: ClassifyRequest) -> Result<ClassifyResult, EngineError> {
        panic!("human messages must bypass local triage")
    }

    async fn create_agent_runtime(
        &self,
        _config: EngineRuntimeConfig,
    ) -> Result<Box<dyn AgentEngineRuntime>, EngineError> {
        self.runtimes_created.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(CrashOnceRuntime {
            crashed: self.crashed.clone(),
        }))
    }
}

#[async_trait]
impl AgentEngineRuntime for CrashOnceRuntime {
    async fn run_turn(&mut self, _request: TurnRequest) -> Result<TurnResult, EngineError> {
        if !self.crashed.swap(true, Ordering::SeqCst) {
            panic!("intentional Runner crash")
        }
        Ok(TurnResult::default())
    }

    async fn shutdown(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

#[tokio::test]
async fn a_failed_runner_is_rebuilt_without_waiting_for_the_roster_poll() {
    let admin_url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL is required");
    let redis_url = std::env::var("TEST_REDIS_URL")
        .or_else(|_| std::env::var("REDIS_URL"))
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let database = format!("collab_runner_recovery_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = admin_url.rsplit_once('/').unwrap();
    let database_url = format!("{prefix}/{database}");
    let credentials = RuntimeCredentials::generate();
    let desktop_secret = credentials.desktop_secret.clone();
    let computer_secret = credentials.computer_secret.clone();
    let runtime_session_id = credentials.runtime_session_id.clone();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url,
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
            display_name: "Crash Once".to_string(),
            role: None,
            persona: "Recover after a Runner crash.".to_string(),
            engine_id: "crash-once".to_string(),
            main_model_id: "crash/main".to_string(),
            triage_model_id: "crash/triage".to_string(),
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
        DesktopCommand::CreateDirectRoom { agent_id: agent.id },
    )
    .await
    else {
        panic!("Room creation returned the wrong result")
    };

    let runtimes_created = Arc::new(AtomicUsize::new(0));
    let daemon_shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            openwork_root: tempfile::tempdir().unwrap().keep().join(".openwork"),
            runtime_session_id,
            runtime_base_url: base_url.clone(),
            computer_secret,
            shim_executable: std::path::PathBuf::from(env!("CARGO_BIN_EXE_openwork")),
            poll_interval: Duration::from_millis(50),
            roster_interval: Duration::from_secs(60),
            heartbeat_interval: Duration::from_millis(100),
            engine_rescan_interval: Duration::from_secs(60),
        },
        EngineRegistry::single(CrashOnceAdapter {
            crashed: Arc::new(AtomicBool::new(false)),
            runtimes_created: runtimes_created.clone(),
        }),
    );
    let task_shutdown = daemon_shutdown.clone();
    let daemon_task = tokio::spawn(async move { daemon.run(task_shutdown).await });
    wait_for_count(&runtimes_created, 1).await;

    desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SendMessage {
            room_id: room.id,
            body: "Trigger the intentional crash.".to_string(),
        },
    )
    .await;

    wait_for_count(&runtimes_created, 2).await;

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

async fn wait_for_count(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Runner runtime count never reached {expected}"));
}

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
