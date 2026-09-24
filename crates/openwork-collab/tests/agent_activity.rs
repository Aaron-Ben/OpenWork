#![cfg(unix)]

use openwork_collab::protocol::{
    AgentActivity, AgentCommand, AgentCommandResult, AgentView, BoardView, CardAgentState,
    ComputerHeartbeatRequest, DesktopCommand, DesktopCommandResult, RunnerState, RunnerStatusView,
};

#[path = "support/room_fixture.rs"]
pub mod room_fixture;

use room_fixture::Fixture;

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

async fn desktop_events(fixture: &Fixture) -> reqwest::Response {
    fixture
        .http
        .get(format!("{}/desktop/events", fixture.base_url))
        .bearer_auth(&fixture.desktop_secret)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
}

/// 读 Desktop SSE，直到出现 `kind` 事件。
async fn wait_for(events: &mut reqwest::Response, kind: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let event = read_sse_event(events).await;
            if event.contains(kind) {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {kind} invalidation"))
}

async fn heartbeat(fixture: &Fixture, runners: Vec<RunnerStatusView>) {
    fixture
        .http
        .post(format!("{}/computer/heartbeat", fixture.base_url))
        .bearer_auth(&fixture.computer_secret)
        .json(&ComputerHeartbeatRequest {
            engine_readiness: Vec::new(),
            runners,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

fn runner(agent: &AgentView, state: RunnerState) -> RunnerStatusView {
    RunnerStatusView {
        agent_id: agent.id.clone(),
        config_revision: agent.config_revision,
        state,
        last_error: (state == RunnerState::Error)
            .then(|| "Engine opencode is not ready in this RuntimeSession".to_string()),
    }
}

async fn activities(fixture: &Fixture) -> Vec<AgentView> {
    match fixture.desktop(DesktopCommand::ListAgents).await {
        DesktopCommandResult::Agents { agents } => agents,
        result => panic!("ListAgents returned {result:?}"),
    }
}

async fn activity_of(fixture: &Fixture, agent: &AgentView) -> AgentActivity {
    activities(fixture)
        .await
        .into_iter()
        .find(|candidate| candidate.id == agent.id)
        .expect("Agent is listed")
        .activity
}

async fn card_state(fixture: &Fixture) -> Option<CardAgentState> {
    let DesktopCommandResult::Boards { boards } = fixture.desktop(DesktopCommand::ListBoards).await
    else {
        panic!("ListBoards returned the wrong result")
    };
    boards[0].columns[0].cards[0].agent_state
}

/// collaboration-desktop.md §4.1、§4.3、§5：Agent 的当前状态依次覆盖 工作中、排队、出错、空闲、归档；
/// 卡片的 `agentState` 随负责人变化；Run 打开时发布 `agent_activity`；Agent 命令的输出不带
/// `agentState`。
#[tokio::test]
async fn agent_activity_follows_runs_card_wakes_and_runner_heartbeats() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let cy = fixture.create_agent("Cy").await;
    let dee = fixture.create_agent("Dee").await;
    let eve = fixture.create_agent("Eve").await;
    let room = fixture.create_direct(&ada.id).await;
    let ada_token = fixture.token(&ada.id).await;
    let bo_token = fixture.token(&bo.id).await;

    // 工作中：Run 打开时发布 agent_activity，房间来自 Run。
    let mut events = desktop_events(&fixture).await;
    read_sse_event(&mut events).await;
    fixture
        .send_user(&room.id, "Please open the release board.")
        .await;
    let inbox = fixture.inbox(&ada_token).await;
    let run = fixture.open_run(&ada_token, &inbox).await;
    let event = wait_for(&mut events, "agent_activity").await;
    assert!(event.contains(&ada.id), "{event}");
    // 关闭 SSE 连接，否则 Server 优雅关闭时会一直等它。
    drop(events);
    let AgentActivity::Working {
        room_id,
        room_title,
        card_id,
        started_at,
        ..
    } = activity_of(&fixture, &ada).await
    else {
        panic!("Ada is not working")
    };
    assert_eq!(
        (room_id.as_deref(), room_title, card_id),
        (Some(room.id.as_str()), None, None)
    );
    assert!(started_at.ends_with("+08:00"), "{started_at}");

    // 排队：Ada 建卡指派给 Bo。
    let DesktopCommandResult::Board(board) = fixture
        .desktop(DesktopCommand::CreateBoard {
            title: "Release".to_string(),
            description: None,
        })
        .await
    else {
        panic!("CreateBoard returned the wrong result")
    };
    fixture
        .command(
            &ada_token,
            AgentCommand::CardCreate {
                board_id: board.id.clone(),
                column_id: board.columns[0].id.clone(),
                title: "Fix login".to_string(),
                description: None,
                assignee_id: Some(bo.id.clone()),
            },
        )
        .await;
    fixture
        .command(
            &ada_token,
            AgentCommand::Reply {
                room_id: room.id.clone(),
                body: "Board is ready.".to_string(),
                held_token: None,
                quoted_message_id: None,
                continuation: false,
            },
        )
        .await;
    fixture.finish(&ada_token, &run.id, "completed").await;
    assert_eq!(
        activity_of(&fixture, &bo).await,
        AgentActivity::Queued {
            card_count: 1,
            first_card_title: "Fix login".to_string()
        }
    );
    assert_eq!(card_state(&fixture).await, Some(CardAgentState::Queued));

    // 空闲：Ada 最近一次在私聊里发言；Dee 从没发言。
    let AgentActivity::Idle {
        room_id,
        last_spoke_at,
        ..
    } = activity_of(&fixture, &ada).await
    else {
        panic!("Ada is not idle")
    };
    assert_eq!(room_id.as_deref(), Some(room.id.as_str()));
    assert!(last_spoke_at.is_some_and(|at| at.ends_with("+08:00")));
    assert_eq!(
        activity_of(&fixture, &dee).await,
        AgentActivity::Idle {
            room_id: None,
            room_title: None,
            last_spoke_at: None
        }
    );

    // 出错来自 heartbeat；running 的 Runner 不改变排队。
    heartbeat(
        &fixture,
        vec![
            runner(&ada, RunnerState::Running),
            runner(&bo, RunnerState::Running),
            runner(&cy, RunnerState::Error),
        ],
    )
    .await;
    assert!(matches!(
        activity_of(&fixture, &bo).await,
        AgentActivity::Queued { .. }
    ));
    assert_eq!(
        activity_of(&fixture, &cy).await,
        AgentActivity::Error {
            message: "Engine opencode is not ready in this RuntimeSession".to_string()
        }
    );

    // 归档最先。
    let DesktopCommandResult::Agent(archived) = fixture
        .desktop(DesktopCommand::ArchiveAgent {
            agent_id: eve.id.clone(),
        })
        .await
    else {
        panic!("ArchiveAgent returned the wrong result")
    };
    assert_eq!(archived.activity, AgentActivity::Archived);

    // Bo 处理卡片：卡片 Turn 让 Bo 与卡片都是 working。
    let inbox = fixture.inbox(&bo_token).await;
    let bo_run = fixture.open_run(&bo_token, &inbox).await;
    let AgentActivity::Working {
        card_id,
        card_title,
        ..
    } = activity_of(&fixture, &bo).await
    else {
        panic!("Bo is not working")
    };
    assert_eq!(
        (card_id.is_some(), card_title.as_deref()),
        (true, Some("Fix login"))
    );
    assert_eq!(card_state(&fixture).await, Some(CardAgentState::Working));

    // Agent 命令返回的看板不带 agentState，模型看到的输出不变。
    let shown = fixture
        .command(
            &bo_token,
            AgentCommand::BoardShow {
                board_id: board.id.clone(),
            },
        )
        .await;
    let json = serde_json::to_string(&shown.result).unwrap();
    assert!(!json.contains("agentState"), "{json}");
    let _: &BoardView = match &shown.result {
        AgentCommandResult::Board { board } => board,
        result => panic!("BoardShow returned {result:?}"),
    };

    fixture.finish(&bo_token, &bo_run.id, "completed").await;
    fixture.stop().await;
}
