#![cfg(unix)]

use openwork_collab::{
    protocol::{
        AgentCommand, AgentCommandRequest, AgentCommandResponse, AgentCommandResult,
        AgentTokenResponse, AgentView, DesktopCommand, DesktopCommandRequest, DesktopCommandResult,
        FinishRunRequest, InboxResponse, OpenRunRequest, RoomView, RunView, TriagePayload,
        request_id,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use sqlx::{Executor, PgPool};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    admin: PgPool,
    database: String,
    pool: PgPool,
    server: openwork_collab::server::ServerHandle,
    http: reqwest::Client,
    base_url: String,
    desktop_secret: String,
    computer_secret: String,
}

impl Fixture {
    async fn start() -> Option<Self> {
        let guard = FIXTURE_LOCK.lock().await;
        let base = std::env::var("TEST_DATABASE_URL").ok()?;
        let redis_url = std::env::var("TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
        let admin = PgPool::connect(&base).await.unwrap();
        let database = format!("collab_r5_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE {database}").as_str())
            .await
            .unwrap();
        let (prefix, _) = base.rsplit_once('/').unwrap();
        let database_url = format!("{prefix}/{database}");
        let credentials = RuntimeCredentials::generate();
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
        let pool = PgPool::connect(&database_url).await.unwrap();
        Some(Self {
            _guard: guard,
            admin,
            database,
            pool,
            server,
            http: reqwest::Client::new(),
            base_url,
            desktop_secret,
            computer_secret,
        })
    }

    async fn desktop(&self, command: DesktopCommand) -> DesktopCommandResult {
        self.http
            .post(format!("{}/desktop/commands", self.base_url))
            .bearer_auth(&self.desktop_secret)
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

    async fn create_agent(&self, name: &str) -> AgentView {
        let result = self
            .desktop(DesktopCommand::CreateAgent {
                display_name: name.to_string(),
                role: Some("Collaborator".to_string()),
                persona: format!("You are {name}."),
                engine_id: "opencode".to_string(),
                main_model_id: "local/main".to_string(),
                triage_model_id: "local/triage".to_string(),
            })
            .await;
        let DesktopCommandResult::Agent(agent) = result else {
            panic!("create Agent returned the wrong result")
        };
        agent
    }

    async fn create_direct(&self, agent_id: &str) -> RoomView {
        let result = self
            .desktop(DesktopCommand::CreateDirectRoom {
                agent_id: agent_id.to_string(),
            })
            .await;
        let DesktopCommandResult::Room(room) = result else {
            panic!("create Direct Room returned the wrong result")
        };
        room
    }

    async fn create_group(&self, agent_ids: Vec<String>) -> RoomView {
        let result = self
            .desktop(DesktopCommand::CreateGroupRoom {
                title: "R5 coordination".to_string(),
                agent_ids,
            })
            .await;
        let DesktopCommandResult::Room(room) = result else {
            panic!("create Group Room returned the wrong result")
        };
        room
    }

    async fn send_user(&self, room_id: &str, body: &str) {
        let result = self
            .desktop(DesktopCommand::SendMessage {
                room_id: room_id.to_string(),
                body: body.to_string(),
            })
            .await;
        assert!(matches!(result, DesktopCommandResult::Message(_)));
    }

    async fn token(&self, agent_id: &str) -> String {
        self.http
            .post(format!(
                "{}/computer/agents/{agent_id}/token",
                self.base_url
            ))
            .bearer_auth(&self.computer_secret)
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

    async fn inbox(&self, token: &str) -> InboxResponse {
        self.http
            .get(format!("{}/agent/inbox", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn agent_events(&self, token: &str) -> reqwest::Response {
        self.http
            .get(format!("{}/agent/events", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
    }

    async fn open_run(&self, token: &str, inbox: &InboxResponse) -> RunView {
        self.http
            .post(format!("{}/agent/runs", self.base_url))
            .bearer_auth(token)
            .json(&OpenRunRequest {
                trigger: inbox.trigger.clone().expect("inbox has a trigger"),
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

    async fn triage(&self, token: &str, run_id: &str) -> TriagePayload {
        self.http
            .get(format!(
                "{}/agent/inbox-triage/payload?run_id={run_id}",
                self.base_url
            ))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn command(&self, token: &str, command: AgentCommand) -> AgentCommandResponse {
        self.http
            .post(format!("{}/agent/commands", self.base_url))
            .bearer_auth(token)
            .json(&AgentCommandRequest {
                request_id: request_id(),
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

    async fn finish(&self, token: &str, run_id: &str, status: &str) -> RunView {
        self.http
            .post(format!("{}/agent/runs/{run_id}/finish", self.base_url))
            .bearer_auth(token)
            .json(&FinishRunRequest {
                status: status.to_string(),
                input_tokens: Some(1),
                cached_input_tokens: Some(0),
                output_tokens: Some(1),
                error_code: (status != "completed").then(|| "TEST_FAILURE".to_string()),
                error_message: (status != "completed").then(|| "intentional failure".to_string()),
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

async fn read_sse_event(response: &mut reqwest::Response) -> String {
    let mut buffer = String::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .unwrap()
            .expect("SSE stream ended unexpectedly");
        buffer.push_str(std::str::from_utf8(&chunk).unwrap());
        if let Some(end) = buffer.find("\n\n") {
            return buffer[..end].to_string();
        }
    }
}

#[tokio::test]
async fn failed_run_keeps_the_durable_delivery_and_human_triage_is_deterministic() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let room = fixture.create_direct(&alpha.id).await;
    fixture
        .send_user(&room.id, "Please keep this message durable.")
        .await;
    let token = fixture.token(&alpha.id).await;
    let inbox = fixture.inbox(&token).await;
    let message_id = inbox.messages[0].id.clone();
    let run = fixture.open_run(&token, &inbox).await;

    let triage = fixture.triage(&token, &run.id).await;
    let verdict = triage.verdict.expect("human triage is deterministic");
    assert!(verdict.actionable);
    assert_eq!(verdict.source, "deterministic");
    fixture.finish(&token, &run.id, "failed").await;

    let last_read: i64 = sqlx::query_scalar(
        "SELECT last_read_seq FROM collab_room_members
         WHERE room_id = $1 AND participant_id = $2",
    )
    .bind(&room.id)
    .bind(&alpha.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(last_read, 0);
    let retried = fixture.inbox(&token).await;
    assert_eq!(retried.messages[0].id, message_id);
    assert!(retried.trigger.is_some());

    let interrupted_run = fixture.open_run(&token, &retried).await;
    fixture
        .finish(&token, &interrupted_run.id, "interrupted")
        .await;
    let last_read_after_interruption: i64 = sqlx::query_scalar(
        "SELECT last_read_seq FROM collab_room_members
         WHERE room_id = $1 AND participant_id = $2",
    )
    .bind(&room.id)
    .bind(&alpha.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(last_read_after_interruption, 0);
    let after_interruption = fixture.inbox(&token).await;
    assert_eq!(after_interruption.messages[0].id, message_id);

    fixture.stop().await;
}

#[tokio::test]
async fn per_agent_sse_is_isolated_and_the_durable_inbox_does_not_depend_on_it() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let room = fixture.create_direct(&alpha.id).await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let mut alpha_events = fixture.agent_events(&alpha_token).await;
    let mut beta_events = fixture.agent_events(&beta_token).await;
    let alpha_initial = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_sse_event(&mut alpha_events),
    )
    .await
    .unwrap();
    let beta_initial = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_sse_event(&mut beta_events),
    )
    .await
    .unwrap();
    assert!(alpha_initial.contains("event: agent"));
    assert!(beta_initial.contains("event: agent"));

    fixture
        .send_user(&room.id, "This wake belongs only to Alpha.")
        .await;
    let alpha_wake = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        read_sse_event(&mut alpha_events),
    )
    .await
    .expect("Alpha did not receive its per-Agent wake");
    assert!(alpha_wake.contains("event: agent"));
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(300),
            read_sse_event(&mut beta_events),
        )
        .await
        .is_err(),
        "Beta received Alpha's wake"
    );

    drop(alpha_events);
    drop(beta_events);
    let inbox = fixture.inbox(&alpha_token).await;
    assert!(inbox.messages.iter().any(|message| {
        message.room_id == room.id && message.body == "This wake belongs only to Alpha."
    }));

    fixture.stop().await;
}

#[tokio::test]
async fn concurrent_group_replies_hold_one_agent_until_a_single_reconsideration() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    fixture
        .send_user(&group.id, "Both of you may investigate this.")
        .await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let alpha_inbox = fixture.inbox(&alpha_token).await;
    let beta_inbox = fixture.inbox(&beta_token).await;
    let alpha_run = fixture.open_run(&alpha_token, &alpha_inbox).await;
    let beta_run = fixture.open_run(&beta_token, &beta_inbox).await;

    let (alpha_reply, beta_reply) = tokio::join!(
        fixture.command(
            &alpha_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "Alpha answer".to_string(),
                held_token: None,
            },
        ),
        fixture.command(
            &beta_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "Beta answer".to_string(),
                held_token: None,
            },
        )
    );

    let published = [&alpha_reply, &beta_reply]
        .iter()
        .filter(|response| matches!(response.result, AgentCommandResult::MessagePublished { .. }))
        .count();
    assert_eq!(published, 1);
    let (held_token, held_agent_token) = match (&alpha_reply.result, &beta_reply.result) {
        (AgentCommandResult::Held { retry_token, .. }, _) => {
            (retry_token.clone(), alpha_token.as_str())
        }
        (_, AgentCommandResult::Held { retry_token, .. }) => {
            (retry_token.clone(), beta_token.as_str())
        }
        results => panic!("expected one HELD response, got {results:?}"),
    };
    let consumed_held_token = held_token.clone();
    let retried = fixture
        .command(
            held_agent_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "Reconsidered answer".to_string(),
                held_token: Some(held_token),
            },
        )
        .await;
    assert!(matches!(
        retried.result,
        AgentCommandResult::MessagePublished { .. }
    ));
    let reused = fixture
        .command(
            held_agent_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "A forbidden second reconsideration".to_string(),
                held_token: Some(consumed_held_token),
            },
        )
        .await;
    assert!(matches!(
        reused.result,
        AgentCommandResult::Error { ref code, .. } if code == "HELD"
    ));

    let alpha_finish = fixture
        .finish(&alpha_token, &alpha_run.id, "completed")
        .await;
    let beta_finish = fixture.finish(&beta_token, &beta_run.id, "completed").await;
    assert_eq!(alpha_finish.outcome.as_deref(), Some("acted"));
    assert_eq!(beta_finish.outcome.as_deref(), Some("acted"));
    let message_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM collab_messages WHERE room_id = $1")
            .bind(&group.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(message_count, 3);

    fixture.stop().await;
}

#[tokio::test]
async fn direct_room_reads_and_private_directional_climate_form_one_r5_loop() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let alpha_user_room = fixture.create_direct(&alpha.id).await;
    let beta_user_room = fixture.create_direct(&beta.id).await;
    fixture
        .send_user(&alpha_user_room.id, "Coordinate with Beta.")
        .await;
    fixture
        .send_user(&beta_user_room.id, "Coordinate with Alpha.")
        .await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let alpha_inbox = fixture.inbox(&alpha_token).await;
    let beta_inbox = fixture.inbox(&beta_token).await;
    let alpha_run = fixture.open_run(&alpha_token, &alpha_inbox).await;
    let beta_run = fixture.open_run(&beta_token, &beta_inbox).await;

    let alpha_climate = fixture
        .command(
            &alpha_token,
            AgentCommand::ClimateNote {
                participant_id: beta.id.clone(),
                affinity: 0.75,
                trust: 0.5,
                note: "Strong technically; verify estimates.".to_string(),
            },
        )
        .await;
    assert!(matches!(
        alpha_climate.result,
        AgentCommandResult::Climate { .. }
    ));
    let beta_climate = fixture
        .command(
            &beta_token,
            AgentCommand::ClimateNote {
                participant_id: alpha.id.clone(),
                affinity: -0.25,
                trust: 0.9,
                note: "Careful reviewer.".to_string(),
            },
        )
        .await;
    assert!(matches!(
        beta_climate.result,
        AgentCommandResult::Climate { .. }
    ));
    fixture
        .command(
            &alpha_token,
            AgentCommand::Ack {
                room_id: alpha_user_room.id.clone(),
            },
        )
        .await;
    fixture
        .command(
            &beta_token,
            AgentCommand::Ack {
                room_id: beta_user_room.id.clone(),
            },
        )
        .await;

    let (alpha_dm, beta_dm) = tokio::join!(
        fixture.command(
            &alpha_token,
            AgentCommand::DirectMessage {
                participant_id: beta.id.clone(),
                body: "Alpha to Beta".to_string(),
            },
        ),
        fixture.command(
            &beta_token,
            AgentCommand::DirectMessage {
                participant_id: alpha.id.clone(),
                body: "Beta to Alpha".to_string(),
            },
        )
    );
    let direct_room = match (&alpha_dm.result, &beta_dm.result) {
        (
            AgentCommandResult::DirectMessageSent { room_id: first, .. },
            AgentCommandResult::DirectMessageSent {
                room_id: second, ..
            },
        ) => {
            assert_eq!(first, second);
            first.clone()
        }
        results => panic!("DM returned the wrong results: {results:?}"),
    };
    fixture
        .finish(&alpha_token, &alpha_run.id, "completed")
        .await;
    fixture.finish(&beta_token, &beta_run.id, "completed").await;

    let direct_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM collab_rooms
         WHERE kind = 'direct' AND direct_key = $1",
    )
    .bind(format!("{}|{}", alpha.id, beta.id))
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(direct_count, 1);

    let rooms = fixture.command(&alpha_token, AgentCommand::Rooms).await;
    let AgentCommandResult::Rooms { rooms } = rooms.result else {
        panic!("rooms returned the wrong result")
    };
    assert!(rooms.iter().any(|room| room.id == direct_room));
    let messages = fixture
        .command(
            &alpha_token,
            AgentCommand::Messages {
                room_id: direct_room.clone(),
                tail: 50,
            },
        )
        .await;
    let AgentCommandResult::Messages { messages, .. } = messages.result else {
        panic!("messages returned the wrong result")
    };
    assert_eq!(messages.len(), 2);
    let members = fixture
        .command(
            &alpha_token,
            AgentCommand::Members {
                room_id: direct_room.clone(),
            },
        )
        .await;
    let AgentCommandResult::Members { members, .. } = members.result else {
        panic!("members returned the wrong result")
    };
    assert_eq!(members.len(), 2);
    let participants = fixture
        .command(&alpha_token, AgentCommand::Participants)
        .await;
    let AgentCommandResult::Participants { participants } = participants.result else {
        panic!("participants returned the wrong result")
    };
    assert!(
        participants
            .iter()
            .any(|participant| participant.id == beta.id)
    );

    let own_climate = fixture
        .command(
            &alpha_token,
            AgentCommand::ClimateShow {
                participant_id: None,
            },
        )
        .await;
    let AgentCommandResult::Climates { climates } = own_climate.result else {
        panic!("Climate show returned the wrong result")
    };
    assert_eq!(climates.len(), 1);
    assert_eq!(climates[0].agent_id, alpha.id);
    assert_eq!(climates[0].about_participant_id, beta.id);
    assert_eq!(
        climates[0].last_note.as_deref(),
        Some("Strong technically; verify estimates.")
    );

    let no_active_write = fixture
        .command(
            &alpha_token,
            AgentCommand::ClimateNote {
                participant_id: beta.id.clone(),
                affinity: 0.8,
                trust: 0.6,
                note: "This must require a Run.".to_string(),
            },
        )
        .await;
    assert!(matches!(
        no_active_write.result,
        AgentCommandResult::Error { ref code, .. } if code == "UNAUTHENTICATED"
    ));

    let alpha_agent_inbox = fixture.inbox(&alpha_token).await;
    assert!(
        alpha_agent_inbox
            .messages
            .iter()
            .any(|message| message.author_id == beta.id && message.body == "Beta to Alpha")
    );
    assert_eq!(alpha_agent_inbox.climates.len(), 1);
    assert_eq!(
        alpha_agent_inbox.climates[0].last_note.as_deref(),
        Some("Strong technically; verify estimates.")
    );
    let engage_run = fixture.open_run(&alpha_token, &alpha_agent_inbox).await;
    let engage = fixture.triage(&alpha_token, &engage_run.id).await;
    let engage = engage.verdict.expect("Agent DM engages between checks");
    assert!(engage.actionable);
    assert_eq!(engage.source, "agent_dm_engage");
    fixture.finish(&alpha_token, &engage_run.id, "failed").await;

    let mut transaction = fixture.pool.begin().await.unwrap();
    for sequence in 3..=8_i64 {
        sqlx::query(
            "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
             VALUES ($1, $2, $3, $4, 'normal', $5)",
        )
        .bind(format!("msg-{}", Uuid::new_v4().simple()))
        .bind(&direct_room)
        .bind(sequence)
        .bind(&beta.id)
        .bind(format!("Beta checkpoint message {sequence}"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE collab_rooms SET next_seq = 8 WHERE id = $1")
        .bind(&direct_room)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let checkpoint_inbox = fixture.inbox(&alpha_token).await;
    let alpha_agent_run = fixture.open_run(&alpha_token, &checkpoint_inbox).await;
    let triage = fixture.triage(&alpha_token, &alpha_agent_run.id).await;
    assert!(triage.verdict.is_none());
    let input = triage.input.expect("Agent-only traffic uses local triage");
    assert!(input.contains("Private Climate context"));
    assert!(input.contains("Strong technically; verify estimates."));

    let self_climate = fixture
        .command(
            &alpha_token,
            AgentCommand::ClimateNote {
                participant_id: alpha.id.clone(),
                affinity: 0.0,
                trust: 0.0,
                note: "Self record is invalid.".to_string(),
            },
        )
        .await;
    assert!(matches!(
        self_climate.result,
        AgentCommandResult::Error { ref code, .. } if code == "INVALID_ARGUMENT"
    ));
    fixture
        .command(
            &alpha_token,
            AgentCommand::Ack {
                room_id: direct_room,
            },
        )
        .await;
    fixture
        .finish(&alpha_token, &alpha_agent_run.id, "completed")
        .await;

    let climate_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collab_agent_climates")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(climate_count, 2);
    let history_exists: bool = sqlx::query_scalar(
        "SELECT to_regclass('collab_agent_climate_history') IS NOT NULL
             OR to_regclass('collab_climate_events') IS NOT NULL",
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert!(!history_exists);

    fixture.stop().await;
}
