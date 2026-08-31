#![cfg(unix)]

use openwork_collab::{
    protocol::{
        AgendaCandidate, AgendaDecision, AgendaDecisionRequest, AgendaDecisionResponse,
        AgendaPayload, AgentCommand, AgentCommandRequest, AgentCommandResponse, AgentCommandResult,
        AgentTokenResponse, AgentView, BoardView, CardView, DesktopCommand, DesktopCommandRequest,
        DesktopCommandResult, FinishRunRequest, InboxResponse, OpenRunRequest, RunView, request_id,
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
        let database = format!("collab_r6_{}", Uuid::new_v4().simple());
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

    async fn desktop_response(&self, command: DesktopCommand) -> reqwest::Response {
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
    }

    async fn desktop(&self, command: DesktopCommand) -> DesktopCommandResult {
        self.desktop_response(command)
            .await
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn create_agent(&self, prefix: &str) -> AgentView {
        let unique = Uuid::new_v4().simple().to_string();
        let result = self
            .desktop(DesktopCommand::CreateAgent {
                display_name: format!("{prefix}-{}", &unique[..10]),
                role: Some("Collaborator".to_string()),
                persona: format!("You are {prefix}."),
                engine_id: "opencode".to_string(),
                main_model_id: "local/main".to_string(),
                triage_model_id: "local/triage".to_string(),
            })
            .await;
        let DesktopCommandResult::Agent(agent) = result else {
            panic!("create Agent returned the wrong result")
        };
        assert!(!agent.agenda_enabled);
        agent
    }

    async fn create_board(&self) -> BoardView {
        let result = self
            .desktop(DesktopCommand::CreateBoard {
                title: "Workspace board".to_string(),
                description: Some("R6 shared facts".to_string()),
            })
            .await;
        let DesktopCommandResult::Board(board) = result else {
            panic!("create Board returned the wrong result")
        };
        board
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

    async fn start_message_run(&self, agent_id: &str) -> (String, RunView, String) {
        let room = self
            .desktop(DesktopCommand::CreateDirectRoom {
                agent_id: agent_id.to_string(),
            })
            .await;
        let DesktopCommandResult::Room(room) = room else {
            panic!("create Direct Room returned the wrong result")
        };
        self.desktop(DesktopCommand::SendMessage {
            room_id: room.id.clone(),
            body: "Open an R6 test run.".to_string(),
        })
        .await;
        let token = self.token(agent_id).await;
        let inbox = self
            .http
            .get(format!("{}/agent/inbox", self.base_url))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<InboxResponse>()
            .await
            .unwrap();
        let run = self
            .http
            .post(format!("{}/agent/runs", self.base_url))
            .bearer_auth(&token)
            .json(&OpenRunRequest {
                trigger: inbox.trigger.expect("message creates a trigger"),
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<RunView>()
            .await
            .unwrap();
        (token, run, room.id)
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

    async fn create_card(
        &self,
        token: &str,
        board_id: &str,
        column_id: &str,
        title: &str,
        assignee_id: Option<String>,
    ) -> CardView {
        let response = self
            .command(
                token,
                AgentCommand::CardCreate {
                    board_id: board_id.to_string(),
                    column_id: column_id.to_string(),
                    title: title.to_string(),
                    description: Some("R6 card".to_string()),
                    assignee_id,
                },
            )
            .await;
        let AgentCommandResult::Card { card } = response.result else {
            panic!(
                "create Card returned the wrong result: {:?}",
                response.result
            )
        };
        card
    }

    async fn agenda(&self, token: &str) -> AgendaPayload {
        self.http
            .get(format!("{}/agent/agenda/payload", self.base_url))
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

    async fn finish(&self, token: &str, run_id: &str) {
        self.http
            .post(format!("{}/agent/runs/{run_id}/finish", self.base_url))
            .bearer_auth(token)
            .json(&FinishRunRequest {
                status: "completed".to_string(),
                input_tokens: Some(1),
                cached_input_tokens: Some(0),
                output_tokens: Some(1),
                error_code: None,
                error_message: None,
                assistant_text: None,
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
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
async fn desktop_owns_board_structure_while_agent_owns_the_card_workflow() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let (alpha_token, alpha_run, _) = fixture.start_message_run(&alpha.id).await;
    let mut board = fixture.create_board().await;
    assert_eq!(board.created_by, "local-user");
    assert_eq!(board.columns.len(), 3);
    assert_eq!(
        board
            .columns
            .iter()
            .filter(|column| column.is_terminal)
            .count(),
        1
    );
    assert_eq!(board.columns[0].position, 0);
    assert_eq!(board.columns[1].position, 1);
    assert_eq!(board.columns[2].position, 2);

    let result = fixture
        .desktop(DesktopCommand::UpdateBoard {
            board_id: board.id.clone(),
            title: "Renamed workspace".to_string(),
            description: None,
        })
        .await;
    let DesktopCommandResult::Board(updated) = result else {
        panic!("update Board returned the wrong result")
    };
    assert_eq!(updated.created_by, "local-user");
    assert_eq!(updated.description, None);

    let result = fixture
        .desktop(DesktopCommand::CreateBoardColumn {
            board_id: board.id.clone(),
            title: "Review".to_string(),
            is_terminal: false,
        })
        .await;
    let DesktopCommandResult::Board(updated) = result else {
        panic!("create Column returned the wrong result")
    };
    let review_id = updated.columns.last().unwrap().id.clone();
    board = match fixture
        .desktop(DesktopCommand::MoveBoardColumn {
            column_id: review_id.clone(),
            before_column_id: Some(updated.columns[1].id.clone()),
        })
        .await
    {
        DesktopCommandResult::Board(board) => board,
        _ => panic!("move Column returned the wrong result"),
    };
    assert_eq!(board.columns[1].id, review_id);
    assert_eq!(
        board
            .columns
            .iter()
            .map(|column| column.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );

    let board_read = fixture
        .command(
            &alpha_token,
            AgentCommand::BoardShow {
                board_id: board.id.clone(),
            },
        )
        .await;
    assert!(matches!(
        board_read.result,
        AgentCommandResult::Board { .. }
    ));
    let todo_id = board.columns[0].id.clone();
    let card = fixture
        .create_card(&alpha_token, &board.id, &todo_id, "Implement R6", None)
        .await;
    assert_eq!(card.created_by, alpha.id);

    let card = match fixture
        .command(
            &alpha_token,
            AgentCommand::CardUpdate {
                card_id: card.id.clone(),
                title: "Implement and verify R6".to_string(),
                description: None,
            },
        )
        .await
        .result
    {
        AgentCommandResult::Card { card } => card,
        result => panic!("update Card returned {result:?}"),
    };
    assert_eq!(card.created_by, alpha.id);
    assert_eq!(card.description, None);
    let assigned = fixture
        .command(
            &alpha_token,
            AgentCommand::CardAssign {
                card_id: card.id.clone(),
                assignee_id: beta.id.clone(),
            },
        )
        .await;
    assert!(matches!(
        assigned.result,
        AgentCommandResult::Card { ref card } if card.assignee_id.as_deref() == Some(beta.id.as_str())
    ));
    let shown = fixture
        .command(
            &alpha_token,
            AgentCommand::CardShow {
                card_id: card.id.clone(),
            },
        )
        .await;
    assert!(matches!(shown.result, AgentCommandResult::Card { .. }));

    let column_conflict = fixture
        .desktop_response(DesktopCommand::DeleteBoardColumn {
            column_id: todo_id.clone(),
        })
        .await;
    assert_eq!(column_conflict.status(), reqwest::StatusCode::CONFLICT);
    let board_conflict = fixture
        .desktop_response(DesktopCommand::DeleteBoard {
            board_id: board.id.clone(),
        })
        .await;
    assert_eq!(board_conflict.status(), reqwest::StatusCode::CONFLICT);

    let assigned = fixture
        .desktop(DesktopCommand::AssignCard {
            card_id: card.id.clone(),
            assignee_id: Some("local-user".to_string()),
        })
        .await;
    assert!(matches!(
        assigned,
        DesktopCommandResult::Card(ref card) if card.assignee_id.as_deref() == Some("local-user")
    ));
    assert!(matches!(
        fixture
            .desktop(DesktopCommand::DeleteCard {
                card_id: card.id.clone(),
            })
            .await,
        DesktopCommandResult::Deleted { .. }
    ));
    assert!(matches!(
        fixture
            .desktop(DesktopCommand::DeleteBoardColumn { column_id: todo_id })
            .await,
        DesktopCommandResult::Board(_)
    ));
    assert!(matches!(
        fixture
            .desktop(DesktopCommand::DeleteBoard {
                board_id: board.id.clone(),
            })
            .await,
        DesktopCommandResult::Deleted { .. }
    ));
    fixture.finish(&alpha_token, &alpha_run.id).await;
    fixture.stop().await;
}

#[tokio::test]
async fn concurrent_claim_and_move_keep_one_owner_and_contiguous_positions() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("ClaimAlpha").await;
    let beta = fixture.create_agent("ClaimBeta").await;
    let (alpha_token, alpha_run, _) = fixture.start_message_run(&alpha.id).await;
    let (beta_token, beta_run, _) = fixture.start_message_run(&beta.id).await;
    let board = fixture.create_board().await;
    let source_id = board.columns[0].id.clone();
    let target_id = board.columns[1].id.clone();
    let claim_card = fixture
        .create_card(&alpha_token, &board.id, &source_id, "Claim me", None)
        .await;

    let (alpha_claim, beta_claim) = tokio::join!(
        fixture.command(
            &alpha_token,
            AgentCommand::CardClaim {
                card_id: claim_card.id.clone(),
            }
        ),
        fixture.command(
            &beta_token,
            AgentCommand::CardClaim {
                card_id: claim_card.id.clone(),
            }
        )
    );
    let winners = [&alpha_claim, &beta_claim]
        .into_iter()
        .filter(|response| matches!(response.result, AgentCommandResult::Card { .. }))
        .count();
    let conflicts = [&alpha_claim, &beta_claim]
        .into_iter()
        .filter(|response| {
            matches!(response.result, AgentCommandResult::Error { ref code, .. } if code == "CONFLICT")
        })
        .count();
    assert_eq!((winners, conflicts), (1, 1));

    let mut cards = Vec::new();
    for index in 0..6 {
        cards.push(
            fixture
                .create_card(
                    &alpha_token,
                    &board.id,
                    &source_id,
                    &format!("Move {index}"),
                    None,
                )
                .await,
        );
    }
    let moves = cards[..4].iter().map(|card| {
        fixture.command(
            &alpha_token,
            AgentCommand::CardMove {
                card_id: card.id.clone(),
                column_id: target_id.clone(),
                before_card_id: None,
            },
        )
    });
    let results = futures_util::future::join_all(moves).await;
    assert!(
        results
            .iter()
            .all(|response| matches!(response.result, AgentCommandResult::Card { .. }))
    );
    let target_positions: Vec<i32> = sqlx::query_scalar(
        "SELECT position FROM collab_cards WHERE column_id = $1 ORDER BY position",
    )
    .bind(&target_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(target_positions, vec![0, 1, 2, 3]);
    let source_positions: Vec<i32> = sqlx::query_scalar(
        "SELECT position FROM collab_cards WHERE column_id = $1 ORDER BY position",
    )
    .bind(&source_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(source_positions, vec![0, 1, 2]);

    fixture.finish(&alpha_token, &alpha_run.id).await;
    fixture.finish(&beta_token, &beta_run.id).await;
    fixture.stop().await;
}

#[tokio::test]
async fn agenda_is_opt_in_and_opens_a_card_focused_run_without_a_room() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("AgendaAlpha").await;
    let (token, message_run, room_id) = fixture.start_message_run(&alpha.id).await;
    let board = fixture.create_board().await;
    let todo = board
        .columns
        .iter()
        .find(|column| !column.is_terminal)
        .unwrap();
    let done = board
        .columns
        .iter()
        .find(|column| column.is_terminal)
        .unwrap();
    let card = fixture
        .create_card(
            &token,
            &board.id,
            &todo.id,
            "Agenda candidate",
            Some(alpha.id.clone()),
        )
        .await;

    assert!(
        fixture
            .agenda(&token)
            .await
            .candidate_set
            .candidates
            .is_empty()
    );
    fixture
        .desktop(DesktopCommand::SetAgentAgenda {
            agent_id: alpha.id.clone(),
            enabled: true,
        })
        .await;
    let initial = fixture.agenda(&token).await;
    assert!(initial.candidate_set.candidates.iter().any(
        |candidate| matches!(candidate, AgendaCandidate::AssignedCard { card_id, room_id: None, .. } if card_id == &card.id)
    ));

    fixture
        .desktop(DesktopCommand::UpdateBoardColumn {
            column_id: todo.id.clone(),
            title: "Looks done but is active".to_string(),
            is_terminal: false,
        })
        .await;
    assert!(fixture.agenda(&token).await.candidate_set.candidates.iter().any(
        |candidate| matches!(candidate, AgendaCandidate::AssignedCard { card_id, .. } if card_id == &card.id)
    ));
    fixture
        .command(
            &token,
            AgentCommand::CardMove {
                card_id: card.id.clone(),
                column_id: done.id.clone(),
                before_card_id: None,
            },
        )
        .await;
    assert!(fixture.agenda(&token).await.candidate_set.candidates.iter().all(
        |candidate| !matches!(candidate, AgendaCandidate::AssignedCard { card_id, .. } if card_id == &card.id)
    ));
    fixture
        .desktop(DesktopCommand::UpdateBoardColumn {
            column_id: done.id.clone(),
            title: "Archived result".to_string(),
            is_terminal: true,
        })
        .await;
    assert!(fixture.agenda(&token).await.candidate_set.candidates.iter().all(
        |candidate| !matches!(candidate, AgendaCandidate::AssignedCard { card_id, .. } if card_id == &card.id)
    ));
    fixture
        .command(
            &token,
            AgentCommand::CardMove {
                card_id: card.id.clone(),
                column_id: todo.id.clone(),
                before_card_id: None,
            },
        )
        .await;
    fixture.finish(&token, &message_run.id).await;

    sqlx::query(
        "UPDATE collab_rooms
         SET last_message_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '10 minutes'
         WHERE id = $1",
    )
    .bind(&room_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let payload = fixture.agenda(&token).await;
    assert!(payload.candidate_set.candidates.iter().any(
        |candidate| matches!(candidate, AgendaCandidate::AssignedCard { card_id, .. } if card_id == &card.id)
    ));
    assert!(payload.candidate_set.candidates.iter().any(
        |candidate| matches!(candidate, AgendaCandidate::StalledRoom { room_id: candidate_room, .. } if candidate_room == &room_id)
    ));
    let candidate_id = payload
        .candidate_set
        .candidates
        .iter()
        .find_map(|candidate| match candidate {
            AgendaCandidate::AssignedCard {
                candidate_id,
                card_id,
                ..
            } if card_id == &card.id => Some(candidate_id.clone()),
            _ => None,
        })
        .unwrap();
    let decision = fixture
        .http
        .post(format!("{}/agent/agenda/decision", fixture.base_url))
        .bearer_auth(&token)
        .json(&AgendaDecisionRequest {
            candidate_set: payload.candidate_set,
            decision: AgendaDecision::Act {
                candidate_id,
                reason: "Assigned card is actionable".to_string(),
            },
            model: "local/triage".to_string(),
            input_tokens: 1,
            output_tokens: 1,
            latency_ms: 1,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgendaDecisionResponse>()
        .await
        .unwrap();
    let agenda_trigger = decision.trigger.expect("card decision creates a trigger");
    let agenda_run = fixture
        .http
        .post(format!("{}/agent/runs", fixture.base_url))
        .bearer_auth(&token)
        .json(&OpenRunRequest {
            trigger: agenda_trigger.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<RunView>()
        .await
        .unwrap();
    let replayed_run = fixture
        .http
        .post(format!("{}/agent/runs", fixture.base_url))
        .bearer_auth(&token)
        .json(&OpenRunRequest {
            trigger: agenda_trigger,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<RunView>()
        .await
        .unwrap();
    assert_eq!(replayed_run.id, agenda_run.id);
    let active_runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM collab_runs WHERE agent_id = $1 AND status = 'running'",
    )
    .bind(&alpha.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(active_runs, 1);
    let persisted: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT room_id, focus_card_id FROM collab_runs WHERE id = $1")
            .bind(&agenda_run.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(persisted, (None, Some(card.id.clone())));
    fixture.finish(&token, &agenda_run.id).await;
    let cooled_payload = fixture.agenda(&token).await;
    let cooled_candidate_id = cooled_payload
        .candidate_set
        .candidates
        .iter()
        .find_map(|candidate| match candidate {
            AgendaCandidate::AssignedCard {
                candidate_id,
                card_id,
                ..
            } if card_id == &card.id => Some(candidate_id.clone()),
            _ => None,
        })
        .unwrap();
    let cooled_decision = fixture
        .http
        .post(format!("{}/agent/agenda/decision", fixture.base_url))
        .bearer_auth(&token)
        .json(&AgendaDecisionRequest {
            candidate_set: cooled_payload.candidate_set,
            decision: AgendaDecision::Act {
                candidate_id: cooled_candidate_id,
                reason: "Immediate duplicate attempt".to_string(),
            },
            model: "local/triage".to_string(),
            input_tokens: 1,
            output_tokens: 1,
            latency_ms: 1,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<AgendaDecisionResponse>()
        .await
        .unwrap();
    assert!(
        cooled_decision.trigger.is_none(),
        "Redis Agenda cooldown must suppress an immediate second dispatch"
    );

    fixture
        .desktop(DesktopCommand::SetAgentAgenda {
            agent_id: alpha.id,
            enabled: false,
        })
        .await;
    assert!(
        fixture
            .agenda(&token)
            .await
            .candidate_set
            .candidates
            .is_empty()
    );
    fixture.stop().await;
}
