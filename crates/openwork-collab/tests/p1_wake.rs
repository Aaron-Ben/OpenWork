#![cfg(unix)]

use std::{path::Path, time::Duration};

use openwork_collab::{
    protocol::{
        AgentTokenResponse, COLLAB_PROTOCOL_VERSION, ControlRequest, ControlResponse,
        DeviceStartResponse, EngineProbeView, EngineStatus, HeartbeatRequest,
    },
    server::{CollaborationServer, ServerOptions, control::request},
};
use serde_json::Value;
use sqlx::{Executor, PgPool};
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn create_database() -> Option<(PgPool, String, String)> {
    let base = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&base).await.unwrap();
    let database = format!("collab_p1_wake_{}", Uuid::new_v4().simple());
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

fn start_wake_stream(base: String, token: String) -> (oneshot::Receiver<()>, JoinHandle<Value>) {
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut response = client
            .get(format!("{base}/runtime/wake-stream"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        ready_tx.send(()).unwrap();
        let mut buffer = String::new();
        loop {
            let chunk = response.chunk().await.unwrap().expect("wake stream ended");
            buffer.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = buffer.find("\n\n") {
                let event = buffer[..end].to_string();
                buffer.drain(..end + 2);
                if !event.contains("event: wake") {
                    continue;
                }
                let data = event
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .expect("wake event has data");
                return serde_json::from_str(data).unwrap();
            }
        }
    });
    (ready_rx, task)
}

#[tokio::test]
async fn committed_message_is_delivered_as_an_agent_wake_over_redis_and_sse() {
    let Some((admin, database, database_url)) = create_database().await else {
        return;
    };
    let Some(redis_url) = std::env::var("TEST_REDIS_URL").ok() else {
        admin
            .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
            .await
            .unwrap();
        admin.close().await;
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let socket = state.path().join("control.sock");
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url,
            redis_url,
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
    request(
        &socket,
        &ControlRequest::CreateAgent {
            id: "helper".to_string(),
            display_name: "Helper".to_string(),
            system_prompt: "Help the user.".to_string(),
            model: "opencode/hy3-free".to_string(),
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

    let client = reqwest::Client::new();
    let base = format!("http://{}", server.runtime_addr());
    let device_token = registration.device_token.unwrap();
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
    let token = client
        .post(format!("{base}/api/computers/me/agents/helper/token"))
        .bearer_auth(&device_token)
        .json(&serde_json::json!({ "generation": started.generation }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentTokenResponse>()
        .await
        .unwrap();

    let (stream_ready, wake_task) = start_wake_stream(base, token.token);
    tokio::time::timeout(Duration::from_secs(2), stream_ready)
        .await
        .unwrap()
        .unwrap();
    let ControlResponse::Message(message) = request(
        &socket,
        &ControlRequest::SendMessage {
            room_id: room.id,
            body: "wake up".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("message creation failed")
    };
    let wake = tokio::time::timeout(Duration::from_secs(5), wake_task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wake["agentId"], "helper");
    assert_eq!(wake["messageId"], message.id);

    shutdown.cancel();
    server.shutdown().await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}
