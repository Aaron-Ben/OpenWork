use openwork_collab::{
    protocol::{
        AgentTokenResponse, DesktopCommand, DesktopCommandRequest, DesktopCommandResult,
        InboxResponse, request_id, sse::SseDecoder,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sqlx::{Executor, PgPool};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires explicit TEST_DATABASE_URL service"]
async fn redis_disconnect_cannot_erase_a_durable_message_or_start_agenda() {
    let admin_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let database = format!("collab_redis_resilience_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = admin_url.rsplit_once('/').unwrap();
    let database_url = format!("{prefix}/{database}");
    let credentials = RuntimeCredentials::generate();
    let desktop_secret = credentials.desktop_secret.clone();
    let computer_secret = credentials.computer_secret.clone();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url: database_url.clone(),
            redis_url: "redis://127.0.0.1:1".to_string(),
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
            display_name: "Redis Proof".to_string(),
            role: None,
            persona: "Read durable work before acting.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/test".to_string(),
            triage_model_id: "opencode/test".to_string(),
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
        panic!("Direct Room creation returned the wrong result")
    };
    let DesktopCommandResult::Message(message) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SendMessage {
            room_id: room.id,
            body: "Redis is unavailable; keep this durable.".to_string(),
        },
    )
    .await
    else {
        panic!("message send returned the wrong result")
    };
    desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SetAgentAgenda {
            agent_id: agent.id.clone(),
            enabled: true,
        },
    )
    .await;

    let token = http
        .post(format!("{base_url}/computer/agents/{}/token", agent.id))
        .bearer_auth(&computer_secret)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentTokenResponse>()
        .await
        .unwrap()
        .token;
    let inbox = http
        .get(format!("{base_url}/agent/inbox"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert!(
        inbox
            .messages
            .iter()
            .any(|candidate| candidate.id == message.id),
        "Redis wake failure hid a PostgreSQL message from the durable inbox"
    );

    let agenda = http
        .get(format!("{base_url}/agent/agenda/payload"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert!(
        !agenda.status().is_success(),
        "Agenda must fail closed when Redis cooldown coordination is unavailable"
    );

    server.shutdown().await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
#[ignore = "requires explicit TEST_DATABASE_URL and TEST_REDIS_URL services"]
async fn redis_subscriber_recovers_after_a_live_connection_is_cut() {
    let admin_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let redis_url = std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL is required");
    let redis_client = redis::Client::open(redis_url.as_str()).unwrap();
    let connection_info = redis_client.get_connection_info();
    let (host, port) = match connection_info.addr() {
        redis::ConnectionAddr::Tcp(host, port) => (host.clone(), *port),
        address => panic!("Redis resilience proxy requires plain TCP, got {address}"),
    };
    assert!(
        connection_info.redis_settings().username().is_none()
            && connection_info.redis_settings().password().is_none(),
        "Redis resilience proxy currently requires an unauthenticated test service"
    );
    let target = tokio::net::lookup_host((host.as_str(), port))
        .await
        .unwrap()
        .next()
        .expect("Redis address resolved to no endpoints");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = listener.local_addr().unwrap();
    let (mut proxy_shutdown, mut proxy_task) = spawn_tcp_proxy(listener, target);
    let proxy_url = format!(
        "redis://{proxy_address}/{}",
        connection_info.redis_settings().db()
    );

    let admin = PgPool::connect(&admin_url).await.unwrap();
    let database = format!("collab_redis_reconnect_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE DATABASE {database}").as_str())
        .await
        .unwrap();
    let (prefix, _) = admin_url.rsplit_once('/').unwrap();
    let database_url = format!("{prefix}/{database}");
    let credentials = RuntimeCredentials::generate();
    let desktop_secret = credentials.desktop_secret.clone();
    let computer_secret = credentials.computer_secret.clone();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url,
            redis_url: proxy_url,
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
            display_name: "Redis Reconnect".to_string(),
            role: None,
            persona: "Observe durable messages.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/test".to_string(),
            triage_model_id: "opencode/test".to_string(),
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
        panic!("Direct Room creation returned the wrong result")
    };
    let token = http
        .post(format!("{base_url}/computer/agents/{}/token", agent.id))
        .bearer_auth(&computer_secret)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentTokenResponse>()
        .await
        .unwrap()
        .token;
    let mut events = http
        .get(format!("{base_url}/agent/events"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut decoder = SseDecoder::default();
    next_sse_subject(&mut events, &mut decoder).await;

    let DesktopCommandResult::Message(before_cut) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SendMessage {
            room_id: room.id.clone(),
            body: "before Redis cut".to_string(),
        },
    )
    .await
    else {
        panic!("message send returned the wrong result")
    };
    assert_eq!(
        next_sse_subject(&mut events, &mut decoder).await,
        Some(before_cut.id)
    );

    proxy_shutdown.cancel();
    proxy_task.await.unwrap();
    let DesktopCommandResult::Message(during_cut) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SendMessage {
            room_id: room.id.clone(),
            body: "persist while Redis is cut".to_string(),
        },
    )
    .await
    else {
        panic!("message send returned the wrong result")
    };

    let listener = TcpListener::bind(proxy_address).await.unwrap();
    (proxy_shutdown, proxy_task) = spawn_tcp_proxy(listener, target);
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let DesktopCommandResult::Message(after_reconnect) = desktop_command(
        &http,
        &base_url,
        &desktop_secret,
        DesktopCommand::SendMessage {
            room_id: room.id,
            body: "after Redis reconnect".to_string(),
        },
    )
    .await
    else {
        panic!("message send returned the wrong result")
    };
    assert_eq!(
        next_sse_subject(&mut events, &mut decoder).await,
        Some(after_reconnect.id)
    );
    let inbox = http
        .get(format!("{base_url}/agent/inbox"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert!(
        inbox
            .messages
            .iter()
            .any(|message| message.id == during_cut.id),
        "message committed during the Redis outage disappeared"
    );

    drop(events);
    server.shutdown().await.unwrap();
    proxy_shutdown.cancel();
    proxy_task.await.unwrap();
    admin
        .execute(format!("DROP DATABASE {database} WITH (FORCE)").as_str())
        .await
        .unwrap();
    admin.close().await;
}

fn spawn_tcp_proxy(
    listener: TcpListener,
    target: std::net::SocketAddr,
) -> (CancellationToken, JoinHandle<()>) {
    let shutdown = CancellationToken::new();
    let task_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            let incoming = tokio::select! {
                _ = task_shutdown.cancelled() => return,
                incoming = listener.accept() => incoming,
            };
            let Ok((mut downstream, _)) = incoming else {
                return;
            };
            connections.spawn(async move {
                let Ok(mut upstream) = TcpStream::connect(target).await else {
                    return;
                };
                let _ = copy_bidirectional(&mut downstream, &mut upstream).await;
            });
        }
    });
    (shutdown, task)
}

async fn next_sse_subject(
    response: &mut reqwest::Response,
    decoder: &mut SseDecoder,
) -> Option<String> {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let chunk = response
                .chunk()
                .await
                .unwrap()
                .expect("Agent SSE ended unexpectedly");
            for event in decoder.push(&chunk).unwrap() {
                if event.event.as_deref() == Some("agent") {
                    let event =
                        serde_json::from_str::<openwork_collab::protocol::InvalidationEvent>(
                            &event.data,
                        )
                        .unwrap();
                    return event.subject_id;
                }
            }
        }
    })
    .await
    .expect("Agent wake did not arrive after Redis recovered")
}

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
