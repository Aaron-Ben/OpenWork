#![cfg(unix)]

use std::{path::Path, time::Duration};

use openwork_collab::{
    protocol::{
        COLLAB_PROTOCOL_VERSION, ComputerStatus, ControlRequest, ControlResponse,
        DeviceStartResponse, EngineProbeView, EngineStatus, HeartbeatRequest,
    },
    server::{CollaborationServer, ServerOptions, control::request},
};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn create_database() -> Option<(PgPool, String, String)> {
    let base = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&base).await.unwrap();
    let database = format!("collab_p1_liveness_{}", Uuid::new_v4().simple());
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
async fn heartbeat_lease_marks_an_abnormally_stopped_computer_offline() {
    let Some((admin, database, database_url)) = create_database().await else {
        return;
    };
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let state = tempfile::tempdir().unwrap();
    let socket = state.path().join("control.sock");
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url: database_url.clone(),
            redis_url,
            state_root: state.path().to_path_buf(),
            control_socket: socket.clone(),
            runtime_bind: "127.0.0.1:0".parse().unwrap(),
            computer_lease: Duration::from_millis(150),
            offline_sweep_interval: Duration::from_millis(25),
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
    let token = registration.device_token.unwrap();
    let base = format!("http://{}", server.runtime_addr());
    let client = reqwest::Client::new();
    let started = client
        .post(format!("{base}/api/computers/me/start"))
        .bearer_auth(&token)
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
        .bearer_auth(&token)
        .json(&HeartbeatRequest {
            protocol_version: COLLAB_PROTOCOL_VERSION,
            generation: started.generation,
            daemon_version: "test".to_string(),
            supervised: false,
            status: ComputerStatus::Online,
            engine: EngineProbeView {
                engine_id: "opencode".to_string(),
                status: EngineStatus::Ready,
                version: Some("test".to_string()),
            },
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let pool = PgPool::connect(&database_url).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status: String =
                sqlx::query_scalar("SELECT status FROM collab_computers WHERE id = 'local'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            if status == "offline" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    pool.close().await;

    shutdown.cancel();
    server.shutdown().await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}
