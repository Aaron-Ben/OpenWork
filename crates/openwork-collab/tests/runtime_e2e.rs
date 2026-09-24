#![cfg(target_os = "macos")]

mod support;

use std::{path::PathBuf, time::Duration};

use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        engine::EngineRegistry,
        opencode::OpenCodeAdapter,
    },
    protocol::{
        AgentView, DesktopCommand, DesktopCommandRequest, DesktopCommandResult, request_id,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};
use openwork_sandbox::{SANDBOX_EXEC, SandboxEnvironment, Seatbelt};
use sqlx::{Executor, PgPool};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// 一套真实的 Desktop 视角运行时：隔离数据库、Server、Computer daemon 与一个 Agent。
struct Runtime {
    admin: PgPool,
    database: String,
    pool: PgPool,
    server: openwork_collab::server::ServerHandle,
    http: reqwest::Client,
    base_url: String,
    desktop_secret: String,
    agent: AgentView,
    daemon_shutdown: CancellationToken,
    daemon_task:
        tokio::task::JoinHandle<Result<(), openwork_collab::computer::daemon::ComputerError>>,
    _state: tempfile::TempDir,
}

impl Runtime {
    /// 缺少 `TEST_DATABASE_URL` 时返回 `None`。`engine_executable` 为 `None` 时使用 fake OpenCode。
    async fn start(engine_executable: Option<PathBuf>, model: &str) -> Option<Self> {
        let base = std::env::var("TEST_DATABASE_URL").ok()?;
        let redis_url = std::env::var("TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_string());
        let admin = PgPool::connect(&base).await.unwrap();
        let database = format!("collab_runtime_e2e_{}", Uuid::new_v4().simple());
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

        let DesktopCommandResult::Agent(agent) = desktop_command(
            &http,
            &base_url,
            &desktop_secret,
            DesktopCommand::CreateAgent {
                display_name: "Helper".to_string(),
                role: Some("Assistant".to_string()),
                persona: "Reply clearly.".to_string(),
                engine_id: "opencode".to_string(),
                main_model_id: model.to_string(),
                triage_model_id: model.to_string(),
            },
        )
        .await
        else {
            panic!("Agent creation returned the wrong result")
        };

        let state = tempfile::tempdir().unwrap();
        // 真实 OpenCode 使用用户自己的登录信息；fake OpenCode 不需要。
        let (engine_executable, user_data_home) = match engine_executable {
            Some(executable) => (executable, real_user_data_home()),
            None => (
                support::fake_opencode(&state).await,
                state.path().join("user-data"),
            ),
        };
        let daemon_shutdown = CancellationToken::new();
        let daemon = ComputerDaemon::new(
            ComputerOptions {
                openwork_root: state.path().join(".openwork"),
                runtime_session_id,
                runtime_base_url: base_url.clone(),
                computer_secret,
                shim_executable: std::path::PathBuf::from(env!("CARGO_BIN_EXE_openwork")),
                sandbox_environment: SandboxEnvironment::detect(Vec::new()).unwrap(),
                poll_interval: Duration::from_millis(100),
                roster_interval: Duration::from_millis(100),
                heartbeat_interval: Duration::from_millis(100),
                engine_rescan_interval: Duration::from_millis(250),
            },
            EngineRegistry::single(OpenCodeAdapter::new(
                engine_executable,
                Seatbelt::probe(SANDBOX_EXEC),
                user_data_home,
            )),
        );
        let daemon_task_shutdown = daemon_shutdown.clone();
        let daemon_task = tokio::spawn(async move { daemon.run(daemon_task_shutdown).await });
        Some(Self {
            admin,
            database,
            pool,
            server,
            http,
            base_url,
            desktop_secret,
            agent,
            daemon_shutdown,
            daemon_task,
            _state: state,
        })
    }

    async fn desktop(&self, command: DesktopCommand) -> DesktopCommandResult {
        desktop_command(&self.http, &self.base_url, &self.desktop_secret, command).await
    }

    /// 等到 Agent 恰好有 `count` 个 completed Run。
    async fn wait_for_completed_runs(&self, count: i64, timeout: Duration) {
        tokio::time::timeout(timeout, async {
            loop {
                let completed: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM collab_runs
                     WHERE agent_id = $1 AND status = 'completed'",
                )
                .bind(&self.agent.id)
                .fetch_one(&self.pool)
                .await
                .unwrap();
                if completed == count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
    }

    async fn stop(self) {
        self.daemon_shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(20), self.daemon_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.pool.close().await;
        self.server.shutdown().await.unwrap();
        self.admin
            .execute(format!("DROP DATABASE {} WITH (FORCE)", self.database).as_str())
            .await
            .unwrap();
        self.admin.close().await;
    }
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

async fn run_smoke(
    engine_executable: Option<PathBuf>,
    model: &str,
    prompt: &str,
    expected_body: &str,
    exact_body: bool,
    reply_timeout: Duration,
) {
    let Some(runtime) = Runtime::start(engine_executable, model).await else {
        return;
    };
    let agent = runtime.agent.clone();
    let DesktopCommandResult::Room(room) = runtime
        .desktop(DesktopCommand::CreateDirectRoom {
            agent_id: agent.id.clone(),
        })
        .await
    else {
        panic!("Room creation returned the wrong result")
    };
    runtime
        .desktop(DesktopCommand::SendMessage {
            room_id: room.id.clone(),
            body: prompt.to_string(),
            quoted_message_id: None,
        })
        .await;

    tokio::time::timeout(reply_timeout, async {
        loop {
            let DesktopCommandResult::Messages { messages } = runtime
                .desktop(DesktopCommand::ListMessages {
                    room_id: room.id.clone(),
                })
                .await
            else {
                panic!("Message listing returned the wrong result")
            };
            if messages.len() == 2 {
                assert_eq!(messages[1].author_id, agent.id);
                if exact_body {
                    assert_eq!(messages[1].body, expected_body);
                } else {
                    assert!(messages[1].body.contains(expected_body));
                }
                break;
            }
            assert!(messages.len() < 2, "Agent published more than one reply");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    runtime
        .wait_for_completed_runs(1, Duration::from_secs(3))
        .await;
    runtime.stop().await;
}

/// collaboration.md §11.4、§16 #14：Desktop 改派卡片后 Agent 直接进入卡片 Turn（fake OpenCode 的
/// triage 总是答“不参与”，所以卡片被领取就证明没有经过 triage），Run 成功后唤醒结算。
#[tokio::test]
async fn desktop_card_assignment_runs_a_card_turn_that_skips_triage() {
    let Some(runtime) = Runtime::start(None, "opencode/test").await else {
        return;
    };
    let agent_id = runtime.agent.id.clone();
    let DesktopCommandResult::Board(board) = runtime
        .desktop(DesktopCommand::CreateBoard {
            title: "Release".to_string(),
            description: None,
        })
        .await
    else {
        panic!("Board creation returned the wrong result")
    };
    // Desktop 没有建卡命令（卡片由 Agent 创建），这里直接写入一张未分配的卡片。
    let card_id = format!("card-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO collab_cards (id, board_id, column_id, title, position, created_by)
         VALUES ($1, $2, $3, 'Ship the login fix', 0, 'local-user')",
    )
    .bind(&card_id)
    .bind(&board.id)
    .bind(&board.columns[0].id)
    .execute(&runtime.pool)
    .await
    .unwrap();
    runtime
        .desktop(DesktopCommand::AssignCard {
            card_id: card_id.clone(),
            assignee_id: Some(agent_id.clone()),
        })
        .await;

    runtime
        .wait_for_completed_runs(1, Duration::from_secs(12))
        .await;
    let (column_id, assignee_id): (String, Option<String>) =
        sqlx::query_as("SELECT column_id, assignee_id FROM collab_cards WHERE id = $1")
            .bind(&card_id)
            .fetch_one(&runtime.pool)
            .await
            .unwrap();
    assert_eq!(
        (column_id, assignee_id),
        (board.columns[1].id.clone(), Some(agent_id.clone()))
    );
    let (trigger, outcome, triages): (String, Option<String>, i64) = sqlx::query_as(
        "SELECT run.trigger, run.outcome,
                (SELECT COUNT(*) FROM collab_triages triage WHERE triage.run_id = run.id)
         FROM collab_runs run WHERE run.agent_id = $1",
    )
    .bind(&agent_id)
    .fetch_one(&runtime.pool)
    .await
    .unwrap();
    assert_eq!(
        (trigger.as_str(), outcome.as_deref(), triages),
        ("card", Some("acted"), 0)
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM collab_card_wakes WHERE agent_id = $1 AND settled_at IS NULL",
    )
    .bind(&agent_id)
    .fetch_one(&runtime.pool)
    .await
    .unwrap();
    assert_eq!(pending, 0);
    runtime.stop().await;
}

#[tokio::test]
async fn desktop_server_computer_and_fake_opencode_settle_a_reply() {
    run_smoke(
        None,
        "opencode/test",
        "Please answer through OpenCode.",
        "Agent says `code` $(literal) --as=admin\nsecond line",
        true,
        Duration::from_secs(12),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires explicit authorization for an external OpenCode model request"]
async fn desktop_server_computer_and_real_opencode_smoke() {
    assert_eq!(
        std::env::var("OPENWORK_REAL_OPENCODE_SMOKE").as_deref(),
        Ok("1"),
        "set OPENWORK_REAL_OPENCODE_SMOKE=1 to confirm the external model request"
    );
    let executable = std::env::var_os("OPENCODE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("opencode"));
    let model = std::env::var("OPENWORK_REAL_OPENCODE_MODEL")
        .unwrap_or_else(|_| "deepseek/deepseek-flash".to_string());
    run_smoke(
        Some(executable),
        &model,
        "Use the openwork reply command to reply with exactly: OpenWork real smoke OK",
        "OpenWork real smoke OK",
        false,
        Duration::from_secs(180),
    )
    .await;
}

/// 用户自己的 XDG data 目录，真实 OpenCode 的登录信息在其中。
fn real_user_data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"))
}
