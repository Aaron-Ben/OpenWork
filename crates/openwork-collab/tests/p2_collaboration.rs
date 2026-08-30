#![cfg(unix)]

use std::{path::Path, time::Duration};

use openwork_collab::{
    protocol::{
        AgentTokenResponse, COLLAB_PROTOCOL_VERSION, CliRequest, CliResult, CliSideEffect,
        ComputerStatus, ControlRequest, ControlResponse, DeviceStartResponse, EngineInventoryView,
        EngineStatus, FinishRunRequest, HeartbeatRequest, InboxResponse, OpenRunRequest, RunView,
        TriagePayload, TriageReportRequest,
    },
    server::{CollaborationServer, ServerHandle, ServerOptions, control::request},
};
use sqlx::{Executor, PgPool};
use tokio::{
    sync::{Mutex, MutexGuard, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

struct Fixture {
    _test_guard: MutexGuard<'static, ()>,
    admin: PgPool,
    database: String,
    socket: std::path::PathBuf,
    server: ServerHandle,
    pool: PgPool,
    client: reqwest::Client,
    base_url: String,
    device_token: String,
    generation: i64,
}

impl Fixture {
    async fn start() -> Option<Self> {
        let redis_url = std::env::var("TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
        Self::start_with_redis(redis_url).await
    }

    async fn start_with_redis(redis_url: String) -> Option<Self> {
        let test_guard = FIXTURE_LOCK.lock().await;
        let base_database_url = std::env::var("TEST_DATABASE_URL").ok()?;
        let admin = PgPool::connect(&base_database_url).await.unwrap();
        let database = format!("collab_p2_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE {database}").as_str())
            .await
            .unwrap();
        let (prefix, _) = base_database_url.rsplit_once('/').unwrap();
        let database_url = format!("{prefix}/{database}");
        let pool = PgPool::connect(&database_url).await.unwrap();
        let state = tempfile::tempdir().unwrap().keep();
        let socket = state.join("control.sock");
        let shutdown = CancellationToken::new();
        let server = CollaborationServer::start(
            ServerOptions {
                database_url,
                redis_url,
                state_root: state,
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
        let ControlResponse::LocalComputer(registration) =
            request(&socket, &ControlRequest::EnsureLocalComputer)
                .await
                .unwrap()
        else {
            panic!("local computer registration failed")
        };
        let device_token = registration.device_token.unwrap();
        let client = reqwest::Client::new();
        let base_url = format!("http://{}", server.runtime_addr());
        let generation = client
            .post(format!("{base_url}/api/computers/me/start"))
            .bearer_auth(&device_token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<DeviceStartResponse>()
            .await
            .unwrap()
            .generation;
        client
            .post(format!("{base_url}/api/computers/me/heartbeat"))
            .bearer_auth(&device_token)
            .json(&HeartbeatRequest {
                protocol_version: COLLAB_PROTOCOL_VERSION,
                generation,
                daemon_version: "test".to_string(),
                supervised: false,
                status: ComputerStatus::Online,
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
        Some(Self {
            _test_guard: test_guard,
            admin,
            database,
            socket,
            server,
            pool,
            client,
            base_url,
            device_token,
            generation,
        })
    }

    async fn create_agent(&self, id: &str) -> String {
        let response = request(
            &self.socket,
            &ControlRequest::CreateAgent {
                id: id.to_string(),
                display_name: id.to_string(),
                system_prompt: "Collaborate carefully.".to_string(),
                model: "opencode/mimo-v2.5-free".to_string(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, ControlResponse::Agent(_)));
        self.client
            .post(format!(
                "{}/api/computers/me/agents/{id}/token",
                self.base_url
            ))
            .bearer_auth(&self.device_token)
            .json(&serde_json::json!({ "generation": self.generation }))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<AgentTokenResponse>()
            .await
            .unwrap()
            .token
    }

    async fn open_direct_run(&self, agent_id: &str, token: &str) -> String {
        let ControlResponse::Room(room) = request(
            &self.socket,
            &ControlRequest::CreateDirectRoom {
                agent_id: agent_id.to_string(),
            },
        )
        .await
        .unwrap() else {
            panic!("direct room creation failed")
        };
        let response = request(
            &self.socket,
            &ControlRequest::SendMessage {
                room_id: room.id,
                body: "Please start a group.".to_string(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, ControlResponse::Message(_)));
        self.open_inbox_run(token).await
    }

    async fn open_inbox_run(&self, token: &str) -> String {
        let inbox = self
            .client
            .get(format!("{}/runtime/inbox", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<InboxResponse>()
            .await
            .unwrap();
        let trigger = inbox.trigger.unwrap();
        self.client
            .post(format!("{}/runtime/runs", self.base_url))
            .bearer_auth(token)
            .json(&OpenRunRequest { trigger })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<RunView>()
            .await
            .unwrap()
            .id
    }

    async fn finish(&self, token: &str, run_id: &str) -> RunView {
        self.finish_with_status(token, run_id, "completed").await
    }

    async fn finish_with_status(&self, token: &str, run_id: &str, status: &str) -> RunView {
        self.client
            .post(format!("{}/runtime/runs/{run_id}/finish", self.base_url))
            .bearer_auth(token)
            .json(&FinishRunRequest {
                status: status.to_string(),
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                error_code: None,
                error_message: None,
                assistant_text: None,
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<RunView>()
            .await
            .unwrap()
    }

    async fn cli(&self, token: &str, request_id: &str, argv: Vec<&str>) -> CliResult {
        let response = self
            .client
            .post(format!("{}/runtime/cli", self.base_url))
            .bearer_auth(token)
            .json(&CliRequest {
                request_id: request_id.to_string(),
                argv: argv.into_iter().map(str::to_string).collect(),
            })
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(status.is_success(), "Runtime CLI failed: {status} {body}");
        serde_json::from_str(&body).unwrap()
    }

    async fn triage_payload(&self, token: &str, run_id: &str) -> TriagePayload {
        self.client
            .get(format!(
                "{}/runtime/inbox-triage/payload?run_id={run_id}",
                self.base_url
            ))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<TriagePayload>()
            .await
            .unwrap()
    }

    async fn insert_message(&self, room_id: &str, author_id: &str, message_kind: &str, body: &str) {
        let mut transaction = self.pool.begin().await.unwrap();
        let sequence: i64 = sqlx::query_scalar(
            "UPDATE collab_rooms
             SET next_seq = next_seq + 1,
                 last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1
             RETURNING next_seq",
        )
        .bind(room_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(format!("msg_{}", Uuid::new_v4().simple()))
        .bind(room_id)
        .bind(sequence)
        .bind(author_id)
        .bind(message_kind)
        .bind(body)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }

    async fn stop(self) {
        self.server.shutdown().await.unwrap();
        self.pool.close().await;
        self.admin
            .execute(format!("DROP DATABASE {} WITH (FORCE)", self.database).as_str())
            .await
            .unwrap();
        self.admin.close().await;
    }
}

fn start_wake_stream(
    base: String,
    token: String,
    expected_room_id: String,
) -> (oneshot::Receiver<()>, JoinHandle<serde_json::Value>) {
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
                let wake: serde_json::Value = serde_json::from_str(data).unwrap();
                if wake["roomId"] == expected_room_id {
                    return wake;
                }
            }
        }
    });
    (ready_rx, task)
}

#[tokio::test]
async fn p2_5_triage_bypasses_the_model_for_human_and_system_messages() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;

    let alpha_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let human = fixture.triage_payload(&alpha_token, &alpha_run).await;
    let human = human.verdict.expect("human message is deterministic");
    assert!(human.actionable);
    assert_eq!(human.source, "deterministic");

    let dm = fixture
        .cli(
            &alpha_token,
            "cli_p25_agent_dm",
            vec!["dm", "beta", "--", "Can you inspect the failure?"],
        )
        .await;
    let (agent_room, _) = direct_effects(&dm);
    fixture.finish(&alpha_token, &alpha_run).await;

    let beta_run = fixture.open_inbox_run(&beta_token).await;
    let agent_only = fixture.triage_payload(&beta_token, &beta_run).await;
    assert!(agent_only.verdict.is_none());
    let input = agent_only
        .input
        .expect("agent-only traffic uses local triage");
    assert!(input.contains("room_kind: direct"));
    assert!(input.contains("message_kind: normal"));
    assert!(input.contains("author_kind: agent"));
    assert!(input.contains("author_name: alpha"));
    fixture
        .cli(
            &beta_token,
            "cli_p25_ack_agent_dm",
            vec!["ack", &agent_room],
        )
        .await;
    fixture.finish(&beta_token, &beta_run).await;

    fixture
        .insert_message(
            &agent_room,
            "alpha",
            "normal",
            "The failure now reproduces twice.",
        )
        .await;
    let follow_up_run = fixture.open_inbox_run(&beta_token).await;
    let follow_up = fixture.triage_payload(&beta_token, &follow_up_run).await;
    let follow_up_input = follow_up.input.expect("agent follow-up uses local triage");
    assert!(follow_up_input.contains("Recent posted context:"));
    assert!(follow_up_input.contains("Can you inspect the failure?"));
    assert!(follow_up_input.contains("The failure now reproduces twice."));
    fixture
        .cli(
            &beta_token,
            "cli_p25_ack_agent_follow_up",
            vec!["ack", &agent_room],
        )
        .await;
    fixture.finish(&beta_token, &follow_up_run).await;

    fixture
        .insert_message(&agent_room, "alpha", "system", "alpha updated membership")
        .await;
    let system_run = fixture.open_inbox_run(&beta_token).await;
    let system = fixture.triage_payload(&beta_token, &system_run).await;
    let system = system
        .verdict
        .expect("system-only delivery is deterministic");
    assert!(!system.actionable);
    assert_eq!(system.source, "system_only");
    fixture
        .client
        .post(format!("{}/runtime/triage", fixture.base_url))
        .bearer_auth(&beta_token)
        .json(&TriageReportRequest {
            run_id: system_run.clone(),
            verdict: system,
            model: "opencode/mimo-v2.5-free".to_string(),
            input_tokens: Some(0),
            output_tokens: Some(0),
            latency_ms: Some(0),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let finished = fixture.finish(&beta_token, &system_run).await;
    assert_eq!(finished.outcome.as_deref(), Some("silent"));

    fixture.stop().await;
}

#[tokio::test]
async fn p2_5_muted_agent_wakes_only_for_an_exact_id_mention_and_receives_context() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_p25_muted_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
            ],
        )
        .await;
    let room_id = group_room(&created);
    fixture.finish(&alpha_token, &bootstrap_run).await;

    let initial_run = fixture.open_inbox_run(&beta_token).await;
    fixture
        .cli(&beta_token, "cli_p25_ack_initial", vec!["ack", &room_id])
        .await;
    fixture.finish(&beta_token, &initial_run).await;
    sqlx::query(
        "UPDATE collab_room_members SET muted = TRUE
         WHERE room_id = $1 AND participant_id = 'beta'",
    )
    .bind(&room_id)
    .execute(&fixture.pool)
    .await
    .unwrap();

    let (ready, mut wake_task) = start_wake_stream(
        fixture.base_url.clone(),
        beta_token.clone(),
        room_id.clone(),
    );
    tokio::time::timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap();
    request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "Could @beta2 inspect this?".to_string(),
        },
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut wake_task)
            .await
            .is_err()
    );

    let ControlResponse::Message(mentioned) = request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "Could @beta inspect this?".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("mentioned message was not created")
    };
    let wake = tokio::time::timeout(Duration::from_secs(5), wake_task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wake["agentId"], "beta");
    assert_eq!(wake["messageId"], mentioned.id);

    let inbox = fixture
        .client
        .get(format!("{}/runtime/inbox", fixture.base_url))
        .bearer_auth(&beta_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert_eq!(inbox.messages.len(), 2);
    assert_eq!(inbox.messages[0].body, "Could @beta2 inspect this?");
    assert_eq!(inbox.messages[1].body, "Could @beta inspect this?");

    fixture.stop().await;
}

#[tokio::test]
async fn stale_group_reply_is_held_until_the_agent_reconsiders_once() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_group_for_held",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
            ],
        )
        .await;
    let room_id = created
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::GroupRoomCreated { room_id } => Some(room_id.clone()),
            _ => None,
        })
        .unwrap();
    fixture.finish(&alpha_token, &bootstrap_run).await;
    let response = request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "What should we do?".to_string(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(response, ControlResponse::Message(_)));
    fixture.open_inbox_run(&alpha_token).await;
    fixture.open_inbox_run(&beta_token).await;

    let alpha = fixture
        .cli(
            &alpha_token,
            "cli_alpha_reply",
            vec!["reply", &room_id, "--", "Alpha answer"],
        )
        .await;
    assert_eq!(alpha.exit_code, 0);
    let held = fixture
        .cli(
            &beta_token,
            "cli_beta_stale_reply",
            vec!["reply", &room_id, "--", "Beta stale answer"],
        )
        .await;
    assert_eq!(held.exit_code, 10);
    assert!(held.text.contains("Alpha answer"));
    let token = held
        .text
        .lines()
        .find_map(|line| line.trim().strip_prefix("--held-token "))
        .expect("HELD response includes a retry token")
        .to_string();

    request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "More context arrived".to_string(),
        },
    )
    .await
    .unwrap();
    let held_again = fixture
        .cli(
            &beta_token,
            "cli_beta_reconsidered",
            vec![
                "reply",
                &room_id,
                "--held-token",
                &token,
                "--",
                "Beta adds a distinct point",
            ],
        )
        .await;
    assert_eq!(held_again.exit_code, 10);
    assert!(held_again.text.contains("More context arrived"));
    let refreshed_token = held_again
        .text
        .lines()
        .find_map(|line| line.trim().strip_prefix("--held-token "))
        .expect("a newer peer message issues a fresh HELD token")
        .to_string();
    let reconsidered = fixture
        .cli(
            &beta_token,
            "cli_beta_reconsidered_again",
            vec![
                "reply",
                &room_id,
                "--held-token",
                &refreshed_token,
                "--",
                "Beta adds a distinct point",
            ],
        )
        .await;
    assert_eq!(reconsidered.exit_code, 0);
    let replayed = fixture
        .cli(
            &beta_token,
            "cli_beta_replayed_hold",
            vec![
                "reply",
                &room_id,
                "--held-token",
                &token,
                "--",
                "Trying to replay the token",
            ],
        )
        .await;
    assert_eq!(replayed.exit_code, 10);
    let ControlResponse::Messages { messages } = request(
        &fixture.socket,
        &ControlRequest::ListMessages {
            room_id: room_id.clone(),
        },
    )
    .await
    .unwrap() else {
        panic!("group message listing failed")
    };
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.author_id == "beta")
            .count(),
        1
    );

    fixture.stop().await;
}

#[tokio::test]
async fn concurrent_group_replies_publish_one_message_and_hold_the_other() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_concurrent_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
            ],
        )
        .await;
    let room_id = group_room(&created);
    fixture.finish(&alpha_token, &bootstrap_run).await;
    request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "Question".to_string(),
        },
    )
    .await
    .unwrap();
    fixture.open_inbox_run(&alpha_token).await;
    fixture.open_inbox_run(&beta_token).await;

    let alpha_reply = fixture.cli(
        &alpha_token,
        "cli_concurrent_alpha",
        vec!["reply", &room_id, "--", "Alpha answer"],
    );
    let beta_reply = fixture.cli(
        &beta_token,
        "cli_concurrent_beta",
        vec!["reply", &room_id, "--", "Beta answer"],
    );
    let (alpha, beta) = tokio::join!(alpha_reply, beta_reply);
    let mut exit_codes = [alpha.exit_code, beta.exit_code];
    exit_codes.sort_unstable();
    assert_eq!(exit_codes, [0, 10]);

    let ControlResponse::Messages { messages } = request(
        &fixture.socket,
        &ControlRequest::ListMessages {
            room_id: room_id.clone(),
        },
    )
    .await
    .unwrap() else {
        panic!("group message listing failed")
    };
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(message.body.as_str(), "Alpha answer" | "Beta answer"))
            .count(),
        1
    );

    fixture.stop().await;
}

#[tokio::test]
async fn held_response_includes_the_peer_sequence_bound_to_its_retry_token() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_bounded_held_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
            ],
        )
        .await;
    let room_id = group_room(&created);
    fixture.finish(&alpha_token, &bootstrap_run).await;
    request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "Question".to_string(),
        },
    )
    .await
    .unwrap();
    fixture.open_inbox_run(&beta_token).await;
    for update in 1..=51 {
        request(
            &fixture.socket,
            &ControlRequest::SendMessage {
                room_id: room_id.clone(),
                body: format!("peer update {update}"),
            },
        )
        .await
        .unwrap();
    }

    let held = fixture
        .cli(
            &beta_token,
            "cli_bounded_held_reply",
            vec!["reply", &room_id, "--", "Stale answer"],
        )
        .await;
    assert_eq!(held.exit_code, 10);
    assert!(held.text.contains("peer update 51"));
    assert!(held.text.contains("--held-token hold_"));

    fixture.stop().await;
}

#[tokio::test]
async fn redis_outage_keeps_fresh_reply_available_but_fails_closed_when_held_is_required() {
    let Some(fixture) = Fixture::start_with_redis("redis://127.0.0.1:1".to_string()).await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_redis_outage_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
            ],
        )
        .await;
    let room_id = group_room(&created);
    fixture.finish(&alpha_token, &bootstrap_run).await;
    request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "Question".to_string(),
        },
    )
    .await
    .unwrap();
    fixture.open_inbox_run(&alpha_token).await;
    fixture.open_inbox_run(&beta_token).await;

    let fresh = fixture
        .cli(
            &alpha_token,
            "cli_redis_outage_fresh",
            vec!["reply", &room_id, "--", "Fresh answer"],
        )
        .await;
    assert_eq!(fresh.exit_code, 0);
    let stale = fixture
        .cli(
            &beta_token,
            "cli_redis_outage_stale",
            vec!["reply", &room_id, "--", "Stale answer"],
        )
        .await;
    assert_eq!(stale.exit_code, 11);
    assert!(stale.side_effects.is_empty());

    fixture.stop().await;
}

#[tokio::test]
async fn direct_messages_reuse_one_room_and_reactions_are_idempotent_sets() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let ControlResponse::Room(first_control_room) = request(
        &fixture.socket,
        &ControlRequest::CreateDirectRoom {
            agent_id: "alpha".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("direct room creation failed")
    };
    let ControlResponse::Room(retried_control_room) = request(
        &fixture.socket,
        &ControlRequest::CreateDirectRoom {
            agent_id: "alpha".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("direct room retry failed")
    };
    assert_eq!(first_control_room.id, retried_control_room.id);
    let first_run = fixture.open_direct_run("alpha", &alpha_token).await;

    let first_dm = fixture
        .cli(
            &alpha_token,
            "cli_first_dm",
            vec!["dm", "beta", "--", "First note"],
        )
        .await;
    assert_eq!(first_dm.exit_code, 0);
    let (first_room, first_message) = direct_effects(&first_dm);
    let first_finish = fixture.finish(&alpha_token, &first_run).await;
    assert_eq!(first_finish.outcome.as_deref(), Some("acted"));
    let second_run = fixture.open_inbox_run(&alpha_token).await;
    let second_dm = fixture
        .cli(
            &alpha_token,
            "cli_second_dm",
            vec!["dm", "beta", "--", "Second note"],
        )
        .await;
    let (second_room, _) = direct_effects(&second_dm);
    assert_eq!(first_room, second_room);
    fixture.finish(&alpha_token, &second_run).await;

    fixture.open_inbox_run(&beta_token).await;
    let reacted = fixture
        .cli(
            &beta_token,
            "cli_react_once",
            vec!["react", &first_message, "👍"],
        )
        .await;
    let retried = fixture
        .cli(
            &beta_token,
            "cli_react_again",
            vec!["react", &first_message, "👍"],
        )
        .await;
    assert_eq!(reacted.exit_code, 0);
    assert_eq!(reacted, retried);
    assert!(reacted.side_effects.iter().any(|effect| matches!(
        effect,
        CliSideEffect::ReactionChanged {
            message_id,
            emoji,
            active: true,
        } if message_id == &first_message && emoji == "👍"
    )));

    fixture.stop().await;
}

fn direct_effects(result: &CliResult) -> (String, String) {
    let room_id = result
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::DirectRoomOpened { room_id, .. } => Some(room_id.clone()),
            _ => None,
        })
        .expect("direct room side effect");
    let message_id = result
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::MessagePublished { message_id, .. } => Some(message_id.clone()),
            _ => None,
        })
        .expect("direct message side effect");
    (room_id, message_id)
}

#[tokio::test]
async fn membership_changes_are_ordered_messages_and_new_members_skip_old_history() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let gamma_token = fixture.create_agent("gamma").await;
    fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_membership_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "History",
            ],
        )
        .await;
    let room_id = created
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::GroupRoomCreated { room_id } => Some(room_id.clone()),
            _ => None,
        })
        .unwrap();

    let invited = fixture
        .cli(
            &alpha_token,
            "cli_invite_gamma",
            vec!["group", "invite", &room_id, "gamma"],
        )
        .await;
    assert_membership_change(&invited, &room_id, "gamma", "invited");
    let gamma_inbox = fixture
        .client
        .get(format!("{}/runtime/inbox", fixture.base_url))
        .bearer_auth(&gamma_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert_eq!(gamma_inbox.messages.len(), 1);
    assert!(gamma_inbox.messages[0].body.contains("invited gamma"));

    fixture.open_inbox_run(&beta_token).await;
    let left = fixture
        .cli(
            &beta_token,
            "cli_beta_leave",
            vec!["group", "leave", &room_id],
        )
        .await;
    assert_membership_change(&left, &room_id, "beta", "left");
    let kicked = fixture
        .cli(
            &alpha_token,
            "cli_kick_gamma",
            vec!["group", "kick", &room_id, "gamma"],
        )
        .await;
    assert_membership_change(&kicked, &room_id, "gamma", "kicked");

    let ControlResponse::Messages { messages } = request(
        &fixture.socket,
        &ControlRequest::ListMessages {
            room_id: room_id.clone(),
        },
    )
    .await
    .unwrap() else {
        panic!("group message listing failed")
    };
    assert_eq!(
        messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert!(messages[1].body.contains("invited gamma"));
    assert!(messages[2].body.contains("beta left"));
    assert!(messages[3].body.contains("kicked gamma"));

    fixture.stop().await;
}

fn assert_membership_change(result: &CliResult, room_id: &str, participant_id: &str, change: &str) {
    assert_eq!(result.exit_code, 0);
    assert!(result.side_effects.iter().any(|effect| matches!(
        effect,
        CliSideEffect::MembershipChanged {
            room_id: actual_room,
            participant_id: actual_participant,
            change: actual_change,
        } if actual_room == room_id && actual_participant == participant_id && actual_change == change
    )));
    assert!(
        result
            .side_effects
            .iter()
            .any(|effect| matches!(effect, CliSideEffect::MessagePublished { .. }))
    );
}

#[tokio::test]
async fn multi_room_finish_settles_only_rooms_with_an_explicit_outcome() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    fixture.create_agent("beta").await;
    fixture.create_agent("gamma").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let first_group = fixture
        .cli(
            &alpha_token,
            "cli_first_settle_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "First",
            ],
        )
        .await;
    let second_group = fixture
        .cli(
            &alpha_token,
            "cli_second_settle_group",
            vec![
                "group", "create", "--member", "user", "--member", "gamma", "--", "Second",
            ],
        )
        .await;
    let first_room = group_room(&first_group);
    let second_room = group_room(&second_group);
    fixture.finish(&alpha_token, &bootstrap_run).await;
    for (room_id, body) in [(&first_room, "First task"), (&second_room, "Second task")] {
        let response = request(
            &fixture.socket,
            &ControlRequest::SendMessage {
                room_id: room_id.clone(),
                body: body.to_string(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, ControlResponse::Message(_)));
    }
    let run_id = fixture.open_inbox_run(&alpha_token).await;
    let inbox = fixture
        .cli(&alpha_token, "cli_multi_inbox", vec!["inbox"])
        .await;
    assert_eq!(inbox.exit_code, 0);
    assert!(inbox.text.contains(&first_room));
    assert!(inbox.text.contains(&second_room));
    let glance = fixture
        .cli(
            &alpha_token,
            "cli_multi_glance",
            vec!["glance", &first_room],
        )
        .await;
    assert_eq!(glance.exit_code, 0);
    fixture
        .cli(&alpha_token, "cli_ack_first_room", vec!["ack", &first_room])
        .await;
    let finished = fixture.finish(&alpha_token, &run_id).await;
    assert_eq!(finished.outcome.as_deref(), Some("silent"));

    let next = fixture
        .client
        .get(format!("{}/runtime/inbox", fixture.base_url))
        .bearer_auth(&alpha_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert!(
        next.messages
            .iter()
            .any(|message| message.room_id == second_room)
    );
    assert!(
        next.messages
            .iter()
            .all(|message| message.room_id != first_room)
    );

    fixture.stop().await;
}

#[tokio::test]
async fn failed_and_interrupted_runs_never_settle_an_acknowledged_delivery() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let first_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let first_inbox = fixture
        .cli(&alpha_token, "cli_failed_inbox", vec!["inbox"])
        .await;
    let room_id = first_inbox
        .text
        .lines()
        .find_map(|line| line.strip_prefix("room "))
        .and_then(|line| line.split_whitespace().next())
        .expect("inbox contains its room id")
        .to_string();
    fixture
        .cli(&alpha_token, "cli_failed_ack", vec!["ack", &room_id])
        .await;
    let failed = fixture
        .finish_with_status(&alpha_token, &first_run, "failed")
        .await;
    assert_eq!(failed.outcome, None);

    let second_run = fixture.open_inbox_run(&alpha_token).await;
    fixture
        .cli(&alpha_token, "cli_interrupted_ack", vec!["ack", &room_id])
        .await;
    let interrupted = fixture
        .finish_with_status(&alpha_token, &second_run, "interrupted")
        .await;
    assert_eq!(interrupted.outcome, None);

    let inbox = fixture
        .client
        .get(format!("{}/runtime/inbox", fixture.base_url))
        .bearer_auth(&alpha_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert_eq!(inbox.messages.len(), 1);
    assert_eq!(inbox.messages[0].body, "Please start a group.");

    fixture.stop().await;
}

#[tokio::test]
async fn runtime_rejects_oversized_message_bodies_before_writing_a_side_effect() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let run_id = fixture.open_direct_run("alpha", &alpha_token).await;
    let inbox = fixture
        .cli(&alpha_token, "cli_body_limit_inbox", vec!["inbox"])
        .await;
    let room_id = inbox
        .text
        .lines()
        .find_map(|line| line.strip_prefix("room "))
        .and_then(|line| line.split_whitespace().next())
        .expect("inbox contains its room id")
        .to_string();
    let oversized = "x".repeat(1024 * 1024 + 1);
    let result = fixture
        .cli(
            &alpha_token,
            "cli_oversized_reply",
            vec!["reply", &room_id, "--", &oversized],
        )
        .await;
    assert_eq!(result.exit_code, 2);
    assert!(result.side_effects.is_empty());
    fixture
        .finish_with_status(&alpha_token, &run_id, "failed")
        .await;

    fixture.stop().await;
}

fn group_room(result: &CliResult) -> String {
    result
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::GroupRoomCreated { room_id } => Some(room_id.clone()),
            _ => None,
        })
        .expect("group room side effect")
}

#[tokio::test]
async fn bounded_inbox_carries_over_without_skipping_the_next_message() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let ControlResponse::Room(room) = request(
        &fixture.socket,
        &ControlRequest::CreateDirectRoom {
            agent_id: "alpha".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("direct room creation failed")
    };
    for sequence in 1..=201 {
        let response = request(
            &fixture.socket,
            &ControlRequest::SendMessage {
                room_id: room.id.clone(),
                body: format!("message {sequence}"),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, ControlResponse::Message(_)));
    }
    let first = fixture
        .client
        .get(format!("{}/runtime/inbox", fixture.base_url))
        .bearer_auth(&alpha_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert_eq!(first.messages.len(), 200);
    assert!(first.carried_over);
    let trigger = first.trigger.unwrap();
    assert!(trigger.carried_over);
    assert_eq!(trigger.deliveries[0].from_seq, 1);
    assert_eq!(trigger.deliveries[0].up_to_seq, 200);
    let run = fixture
        .client
        .post(format!("{}/runtime/runs", fixture.base_url))
        .bearer_auth(&alpha_token)
        .json(&OpenRunRequest { trigger })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<RunView>()
        .await
        .unwrap();
    let run_inbox = fixture
        .cli(&alpha_token, "cli_bounded_inbox", vec!["inbox"])
        .await;
    assert!(run_inbox.text.contains("carried_over: true"));
    fixture
        .cli(&alpha_token, "cli_bounded_ack", vec!["ack", &room.id])
        .await;
    fixture.finish(&alpha_token, &run.id).await;

    let second = fixture
        .client
        .get(format!("{}/runtime/inbox", fixture.base_url))
        .bearer_auth(&alpha_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<InboxResponse>()
        .await
        .unwrap();
    assert!(!second.carried_over);
    assert_eq!(second.messages.len(), 1);
    assert_eq!(second.messages[0].sequence, 201);

    fixture.stop().await;
}

#[tokio::test]
async fn repeated_glance_observes_new_peer_messages_instead_of_replaying_a_cached_result() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    let beta_token = fixture.create_agent("beta").await;
    let bootstrap_run = fixture.open_direct_run("alpha", &alpha_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            "cli_glance_group",
            vec![
                "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
            ],
        )
        .await;
    let room_id = group_room(&created);
    fixture.finish(&alpha_token, &bootstrap_run).await;
    request(
        &fixture.socket,
        &ControlRequest::SendMessage {
            room_id: room_id.clone(),
            body: "Question".to_string(),
        },
    )
    .await
    .unwrap();
    fixture.open_inbox_run(&alpha_token).await;
    fixture.open_inbox_run(&beta_token).await;
    fixture
        .cli(
            &alpha_token,
            "cli_alpha_first_update",
            vec!["reply", &room_id, "--", "First update"],
        )
        .await;
    let first = fixture
        .cli(
            &beta_token,
            "cli_beta_first_glance",
            vec!["glance", &room_id],
        )
        .await;
    assert!(first.text.contains("First update"));
    assert!(first.text.contains("Roster:"));
    assert!(first.text.contains("- beta (agent"));
    fixture
        .cli(
            &alpha_token,
            "cli_alpha_second_update",
            vec!["reply", &room_id, "--", "Second update"],
        )
        .await;
    let second = fixture
        .cli(
            &beta_token,
            "cli_beta_second_glance",
            vec!["glance", &room_id],
        )
        .await;
    assert!(second.text.contains("Second update"));

    fixture.stop().await;
}

#[tokio::test]
async fn retrying_group_create_returns_the_original_group_without_duplicate_messages() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha_token = fixture.create_agent("alpha").await;
    fixture.create_agent("beta").await;
    fixture.open_direct_run("alpha", &alpha_token).await;

    let argv = vec![
        "group", "create", "--member", "user", "--member", "beta", "--", "Welcome",
    ];
    let first = fixture
        .cli(&alpha_token, "cli_group_create", argv.clone())
        .await;
    let retried = fixture.cli(&alpha_token, "cli_group_create", argv).await;

    assert_eq!(first, retried);
    assert_eq!(first.exit_code, 0);
    let room_id = first
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::GroupRoomCreated { room_id } => Some(room_id.clone()),
            _ => None,
        })
        .expect("group creation side effect");
    let published = first
        .side_effects
        .iter()
        .filter(|effect| matches!(effect, CliSideEffect::MessagePublished { .. }))
        .count();
    assert_eq!(published, 1);
    let ControlResponse::Messages { messages } = request(
        &fixture.socket,
        &ControlRequest::ListMessages {
            room_id: room_id.clone(),
        },
    )
    .await
    .unwrap() else {
        panic!("group message listing failed")
    };
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].body, "Welcome");
    let ControlResponse::Rooms { rooms } = request(&fixture.socket, &ControlRequest::ListRooms)
        .await
        .unwrap()
    else {
        panic!("room listing failed")
    };
    assert_eq!(rooms.iter().filter(|room| room.kind == "group").count(), 1);

    fixture.stop().await;
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
