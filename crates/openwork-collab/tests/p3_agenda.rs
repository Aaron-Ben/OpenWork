#![cfg(unix)]

use std::{path::Path, time::Duration};

use openwork_collab::{
    protocol::{
        AgendaCandidate, AgendaDecision, AgendaDecisionRequest, AgendaDecisionResponse,
        AgendaPayload, AgentRoster, AgentTokenResponse, COLLAB_PROTOCOL_VERSION, CliRequest,
        CliResult, CliSideEffect, ComputerStatus, ControlRequest, ControlResponse,
        DeviceStartResponse, EngineInventoryView, EngineStatus, FinishRunRequest, HeartbeatRequest,
        InboxResponse, OpenRunRequest, RunView,
    },
    server::{CollaborationServer, ServerHandle, ServerOptions, control::request},
};
use sqlx::{Executor, PgPool};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    admin: PgPool,
    pool: PgPool,
    database: String,
    socket: std::path::PathBuf,
    server: ServerHandle,
    client: reqwest::Client,
    base_url: String,
    device_token: String,
    generation: i64,
}

impl Fixture {
    async fn start() -> Option<Self> {
        Self::start_with_redis(
            std::env::var("TEST_REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string()),
        )
        .await
    }

    async fn start_with_redis(redis_url: String) -> Option<Self> {
        let guard = FIXTURE_LOCK.lock().await;
        let base_database_url = std::env::var("TEST_DATABASE_URL").ok()?;
        let admin = PgPool::connect(&base_database_url).await.unwrap();
        let database = format!("collab_p3_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE {database}").as_str())
            .await
            .unwrap();
        let (prefix, _) = base_database_url.rsplit_once('/').unwrap();
        let database_url = format!("{prefix}/{database}");
        let pool = PgPool::connect(&database_url).await.unwrap();
        let state = tempfile::tempdir().unwrap().keep();
        let socket = state.join("control.sock");
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
            CancellationToken::new(),
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
            _guard: guard,
            admin,
            pool,
            database,
            socket,
            server,
            client,
            base_url,
            device_token,
            generation,
        })
    }

    async fn create_agent(&self, prefix: &str) -> (String, String) {
        let id = format!("{prefix}_{}", &Uuid::new_v4().simple().to_string()[..12]);
        let response = request(
            &self.socket,
            &ControlRequest::CreateAgent {
                id: id.clone(),
                display_name: id.clone(),
                system_prompt: "Collaborate carefully.".to_string(),
                model: "opencode/mimo-v2.5-free".to_string(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, ControlResponse::Agent(_)));
        let token = self
            .client
            .post(format!(
                "{}/api/computers/me/agents/{id}/token",
                self.base_url
            ))
            .bearer_auth(&self.device_token)
            .json(&serde_json::json!({"generation": self.generation}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<AgentTokenResponse>()
            .await
            .unwrap()
            .token;
        (id, token)
    }

    async fn direct_room(&self, agent_id: &str) -> String {
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
        room.id
    }

    async fn send_user(&self, room_id: &str, body: &str) {
        let response = request(
            &self.socket,
            &ControlRequest::SendMessage {
                room_id: room_id.to_string(),
                body: body.to_string(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, ControlResponse::Message(_)));
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
        let trigger = inbox.trigger.expect("inbox has a trigger");
        self.open_trigger(token, trigger).await
    }

    async fn open_trigger(
        &self,
        token: &str,
        trigger: openwork_collab::protocol::TriggerEnvelope,
    ) -> String {
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

    async fn cli(&self, token: &str, argv: Vec<String>) -> CliResult {
        let response = self
            .client
            .post(format!("{}/runtime/cli", self.base_url))
            .bearer_auth(token)
            .json(&CliRequest {
                request_id: format!("cli_{}", Uuid::new_v4().simple()),
                argv,
            })
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(status.is_success(), "Runtime CLI failed: {status} {body}");
        serde_json::from_str(&body).unwrap()
    }

    async fn finish(&self, token: &str, run_id: &str) -> RunView {
        self.client
            .post(format!("{}/runtime/runs/{run_id}/finish", self.base_url))
            .bearer_auth(token)
            .json(&FinishRunRequest {
                status: "completed".to_string(),
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
            .json()
            .await
            .unwrap()
    }

    async fn agenda(&self, token: &str) -> AgendaPayload {
        let response = self
            .client
            .get(format!("{}/runtime/agenda/payload", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(
            status.is_success(),
            "Agenda payload failed: {status} {body}"
        );
        serde_json::from_str(&body).unwrap()
    }

    async fn decide(
        &self,
        token: &str,
        payload: AgendaPayload,
        decision: AgendaDecision,
    ) -> AgendaDecisionResponse {
        self.client
            .post(format!("{}/runtime/agenda/decision", self.base_url))
            .bearer_auth(token)
            .json(&AgendaDecisionRequest {
                candidate_set: payload.candidate_set,
                decision,
                model: "opencode/mimo-v2.5-free".to_string(),
                input_tokens: 10,
                output_tokens: 5,
                latency_ms: 20,
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

    async fn create_group(&self, owner: &str, owner_token: &str, invited: &str) -> String {
        let direct = self.direct_room(owner).await;
        self.send_user(&direct, "Create a group for the project.")
            .await;
        let run = self.open_inbox_run(owner_token).await;
        let result = self
            .cli(
                owner_token,
                vec![
                    "group".into(),
                    "create".into(),
                    "--member".into(),
                    "user".into(),
                    "--member".into(),
                    invited.into(),
                    "--".into(),
                    "Project room opened".into(),
                ],
            )
            .await;
        assert_eq!(result.exit_code, 0, "{}", result.text);
        let room_id = result
            .side_effects
            .iter()
            .find_map(|effect| match effect {
                CliSideEffect::GroupRoomCreated { room_id } => Some(room_id.clone()),
                _ => None,
            })
            .unwrap();
        self.finish(owner_token, &run).await;
        room_id
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

#[tokio::test]
async fn p3_card_claim_has_one_winner_and_agenda_run_keeps_a_focus() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let (alpha, alpha_token) = fixture.create_agent("alpha").await;
    let (beta, beta_token) = fixture.create_agent("beta").await;
    let ControlResponse::Agent(alpha_view) = request(
        &fixture.socket,
        &ControlRequest::SetAgentProactivity {
            agent_id: alpha.clone(),
            enabled: true,
        },
    )
    .await
    .unwrap() else {
        panic!("Agent proactivity update failed")
    };
    assert!(alpha_view.scanner_enabled);
    let roster = fixture
        .client
        .get(format!(
            "{}/api/computers/me/agents?generation={}",
            fixture.base_url, fixture.generation
        ))
        .bearer_auth(&fixture.device_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgentRoster>()
        .await
        .unwrap();
    assert!(
        roster
            .agents
            .iter()
            .any(|assignment| assignment.id == alpha && assignment.scanner_enabled)
    );
    let room_id = fixture.create_group(&alpha, &alpha_token, &beta).await;
    let beta_opening_run = fixture.open_inbox_run(&beta_token).await;
    fixture
        .cli(&beta_token, vec!["ack".into(), room_id.clone()])
        .await;
    fixture.finish(&beta_token, &beta_opening_run).await;

    let ControlResponse::Board(board) = request(
        &fixture.socket,
        &ControlRequest::CreateBoard {
            room_id: room_id.clone(),
            title: "Delivery".to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!("board creation failed")
    };
    let todo = board.columns[0].id.clone();
    fixture
        .send_user(&room_id, "Pick up the delivery cards.")
        .await;
    let alpha_run = fixture.open_inbox_run(&alpha_token).await;
    let beta_run = fixture.open_inbox_run(&beta_token).await;
    let created = fixture
        .cli(
            &alpha_token,
            vec![
                "card".into(),
                "create".into(),
                "--board".into(),
                board.id.clone(),
                "--column".into(),
                todo.clone(),
                "--title".into(),
                "Race-safe card".into(),
                "--assignee".into(),
                beta.clone(),
            ],
        )
        .await;
    let card_id = created
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::CardCreated { card_id, .. } => Some(card_id.clone()),
            _ => None,
        })
        .unwrap();
    let (left, right) = tokio::join!(
        fixture.cli(
            &alpha_token,
            vec!["card".into(), "claim".into(), card_id.clone()]
        ),
        fixture.cli(
            &beta_token,
            vec!["card".into(), "claim".into(), card_id.clone()]
        )
    );
    assert_eq!(
        usize::from(left.exit_code == 0) + usize::from(right.exit_code == 0),
        1
    );
    let winner: String = sqlx::query_scalar("SELECT claimed_by FROM collab_cards WHERE id = $1")
        .bind(&card_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert!(winner == alpha || winner == beta);

    let agenda_card = fixture
        .cli(
            &alpha_token,
            vec![
                "card".into(),
                "create".into(),
                "--board".into(),
                board.id.clone(),
                "--column".into(),
                todo,
                "--title".into(),
                "Agenda card".into(),
                "--assignee".into(),
                alpha.clone(),
            ],
        )
        .await;
    let agenda_card_id = agenda_card
        .side_effects
        .iter()
        .find_map(|effect| match effect {
            CliSideEffect::CardCreated { card_id, .. } => Some(card_id.clone()),
            _ => None,
        })
        .unwrap();
    fixture.finish(&alpha_token, &alpha_run).await;
    fixture.finish(&beta_token, &beta_run).await;

    let payload = fixture.agenda(&alpha_token).await;
    let candidate_id = payload
        .candidate_set
        .candidates
        .iter()
        .find_map(|candidate| match candidate {
            AgendaCandidate::AssignedCard {
                candidate_id,
                card_id,
                ..
            } if card_id == &agenda_card_id => Some(candidate_id.clone()),
            _ => None,
        })
        .expect("assigned card is an Agenda candidate");
    let decision = fixture
        .decide(
            &alpha_token,
            payload,
            AgendaDecision::Act {
                candidate_id,
                reason: "The assigned card is still open".to_string(),
            },
        )
        .await;
    let trigger = decision.trigger.expect("Agenda action returns a trigger");
    assert_eq!(trigger.trigger, "agenda");
    assert_eq!(
        trigger.agenda_focus.as_ref().unwrap().card_id.as_deref(),
        Some(agenda_card_id.as_str())
    );
    let stale_replay = trigger.clone();
    let agenda_run = fixture.open_trigger(&alpha_token, trigger).await;
    let claimed = fixture
        .cli(
            &alpha_token,
            vec!["card".into(), "claim".into(), agenda_card_id.clone()],
        )
        .await;
    assert_eq!(claimed.exit_code, 0, "{}", claimed.text);
    let replay_response = fixture
        .client
        .post(format!("{}/runtime/runs", fixture.base_url))
        .bearer_auth(&alpha_token)
        .json(&OpenRunRequest {
            trigger: stale_replay,
        })
        .send()
        .await
        .unwrap();
    assert_eq!(replay_response.status(), reqwest::StatusCode::CONFLICT);
    let finished = fixture.finish(&alpha_token, &agenda_run).await;
    assert_eq!(finished.outcome.as_deref(), Some("acted"));
    let ControlResponse::Boards { boards } = request(&fixture.socket, &ControlRequest::ListBoards)
        .await
        .unwrap()
    else {
        panic!("board listing failed")
    };
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].id, board.id);
    fixture.stop().await;
}

#[tokio::test]
async fn p3_stalled_room_claim_allows_only_one_agent_trigger() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let (alpha, alpha_token) = fixture.create_agent("stall_a").await;
    let (beta, beta_token) = fixture.create_agent("stall_b").await;
    let room_id = fixture.create_group(&alpha, &alpha_token, &beta).await;
    let beta_run = fixture.open_inbox_run(&beta_token).await;
    fixture
        .cli(&beta_token, vec!["ack".into(), room_id.clone()])
        .await;
    fixture.finish(&beta_token, &beta_run).await;
    sqlx::query(
        "UPDATE collab_rooms
         SET last_message_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '10 minutes'
         WHERE id = $1",
    )
    .bind(&room_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let alpha_payload = fixture.agenda(&alpha_token).await;
    let beta_payload = fixture.agenda(&beta_token).await;
    let alpha_candidate = stalled_candidate_id(&alpha_payload, &room_id);
    let beta_candidate = stalled_candidate_id(&beta_payload, &room_id);
    let (alpha_decision, beta_decision) = tokio::join!(
        fixture.decide(
            &alpha_token,
            alpha_payload,
            AgendaDecision::Act {
                candidate_id: alpha_candidate,
                reason: "A concrete next step is waiting".to_string(),
            }
        ),
        fixture.decide(
            &beta_token,
            beta_payload,
            AgendaDecision::Act {
                candidate_id: beta_candidate,
                reason: "A concrete next step is waiting".to_string(),
            }
        )
    );
    let triggers = [alpha_decision.trigger, beta_decision.trigger]
        .into_iter()
        .flatten()
        .count();
    assert_eq!(triggers, 1);
    fixture.stop().await;
}

#[tokio::test]
async fn p3_decline_cap_stops_until_new_room_activity() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let (agent, token) = fixture.create_agent("decline").await;
    let room_id = fixture.direct_room(&agent).await;
    let empty = fixture.agenda(&token).await;
    assert!(empty.candidate_set.candidates.is_empty());
    fixture
        .send_user(&room_id, "A task may need another pass.")
        .await;
    let run = fixture.open_inbox_run(&token).await;
    fixture
        .cli(&token, vec!["ack".into(), room_id.clone()])
        .await;
    fixture.finish(&token, &run).await;
    age_room(&fixture.pool, &room_id).await;
    for _ in 0..3 {
        let payload = fixture.agenda(&token).await;
        assert!(!payload.candidate_set.candidates.is_empty());
        let response = fixture
            .decide(
                &token,
                payload,
                AgendaDecision::Decline {
                    reason: "The conversation is already complete".to_string(),
                },
            )
            .await;
        assert!(response.trigger.is_none());
    }
    assert!(
        fixture
            .agenda(&token)
            .await
            .candidate_set
            .candidates
            .is_empty()
    );
    fixture
        .send_user(&room_id, "New facts make this active again.")
        .await;
    age_room(&fixture.pool, &room_id).await;
    assert!(
        !fixture
            .agenda(&token)
            .await
            .candidate_set
            .candidates
            .is_empty()
    );
    fixture.stop().await;
}

#[tokio::test]
async fn p3_redis_failure_closes_the_proactive_path() {
    let Some(fixture) = Fixture::start_with_redis("redis://127.0.0.1:1".to_string()).await else {
        return;
    };
    let (_, token) = fixture.create_agent("redis_down").await;
    let response = fixture
        .client
        .get(format!("{}/runtime/agenda/payload", fixture.base_url))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_server_error());
    fixture.stop().await;
}

fn stalled_candidate_id(payload: &AgendaPayload, room_id: &str) -> String {
    payload
        .candidate_set
        .candidates
        .iter()
        .find_map(|candidate| match candidate {
            AgendaCandidate::StalledRoom {
                candidate_id,
                room_id: candidate_room,
                ..
            } if candidate_room == room_id => Some(candidate_id.clone()),
            _ => None,
        })
        .expect("stalled room candidate")
}

async fn age_room(pool: &PgPool, room_id: &str) {
    sqlx::query(
        "UPDATE collab_rooms
         SET last_message_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '10 minutes'
         WHERE id = $1",
    )
    .bind(room_id)
    .execute(pool)
    .await
    .unwrap();
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
