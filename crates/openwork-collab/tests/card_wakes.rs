#![cfg(unix)]

use openwork_collab::protocol::{
    AgentCommand, AgentCommandResult, AgentView, BoardView, CardView, CardWakeReason, CardWakeRef,
    ColumnKind, DesktopCommand, DesktopCommandResult, InboxResponse, RunView,
};

#[path = "support/room_fixture.rs"]
pub mod room_fixture;

use room_fixture::Fixture;
use uuid::Uuid;

/// 显示名带随机后缀：Redis 的唤醒限额按 Agent id 计数，跨测试共享。
async fn agent(fixture: &Fixture, prefix: &str) -> AgentView {
    let unique = Uuid::new_v4().simple().to_string();
    fixture
        .create_agent(&format!("{prefix}{}", &unique[..10]))
        .await
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

/// 让 Agent 有一个 running Run，之后才能发 Agent 命令。
async fn start_run(fixture: &Fixture, agent_id: &str) -> (String, RunView) {
    let room = fixture.create_direct(agent_id).await;
    fixture.send_user(&room.id, "Start a board run.").await;
    let token = fixture.token(agent_id).await;
    let inbox = fixture.inbox(&token).await;
    let run = fixture.open_run(&token, &inbox).await;
    (token, run)
}

async fn create_board(fixture: &Fixture) -> BoardView {
    match fixture
        .desktop(DesktopCommand::CreateBoard {
            title: "Release".to_string(),
            description: None,
        })
        .await
    {
        DesktopCommandResult::Board(board) => board,
        result => panic!("create Board returned {result:?}"),
    }
}

async fn card(fixture: &Fixture, token: &str, command: AgentCommand) -> CardView {
    match fixture.command(token, command).await.result {
        AgentCommandResult::Card { card } => card,
        result => panic!("card command returned {result:?}"),
    }
}

fn create(board: &BoardView, title: &str, assignee_id: Option<&str>) -> AgentCommand {
    AgentCommand::CardCreate {
        board_id: board.id.clone(),
        column_id: board.columns[0].id.clone(),
        title: title.to_string(),
        description: None,
        assignee_id: assignee_id.map(str::to_string),
    }
}

fn update(card_id: &str, description: &str) -> AgentCommand {
    AgentCommand::CardUpdate {
        card_id: card_id.to_string(),
        title: "Fix the login redirect".to_string(),
        description: Some(description.to_string()),
    }
}

/// `(card_id, reason, revision, run_id)` of every pending wake for `agent_id`, oldest card first.
async fn pending(fixture: &Fixture, agent_id: &str) -> Vec<(String, String, i32, Option<String>)> {
    sqlx::query_as(
        "SELECT card_id, reason, revision, run_id FROM collab_card_wakes
         WHERE agent_id = $1 AND settled_at IS NULL
         ORDER BY created_at, id",
    )
    .bind(agent_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap()
}

fn wake_refs(inbox: &InboxResponse) -> Vec<CardWakeRef> {
    inbox
        .trigger
        .as_ref()
        .expect("pending card wakes create a trigger")
        .card_wakes
        .clone()
}

/// collaboration.md §16 #14: real reassignments and new mentions wake once, edits merge into one
/// pending wake, the initiator and archived Agents are skipped, and only a successful Run settles
/// the wakes it saw — a change merged while the Run was working stays pending.
#[tokio::test]
async fn acc_14_real_changes_wake_once_and_merge_until_a_successful_run_settles_them() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = agent(&fixture, "Alpha").await;
    let beta = agent(&fixture, "Beta").await;
    let gamma = agent(&fixture, "Gamma").await;
    let (alpha_token, alpha_run) = start_run(&fixture, &alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let board = create_board(&fixture).await;
    let mut beta_events = fixture.agent_events(&beta_token).await;
    read_sse_event(&mut beta_events).await;

    // 新建时直接指定负责人算一次改派；SSE 立即叫醒 Beta。
    let login = card(
        &fixture,
        &alpha_token,
        create(&board, "Fix the login redirect", Some(&beta.id)),
    )
    .await;
    let event = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        read_sse_event(&mut beta_events),
    )
    .await
    .expect("Beta was not woken over SSE");
    assert!(event.contains("\"kind\":\"board\""), "{event}");
    assert!(event.contains(&login.id), "{event}");
    drop(beta_events);

    let inbox = fixture.inbox(&beta_token).await;
    assert!(inbox.messages.is_empty());
    assert_eq!(inbox.cards.len(), 1);
    let wake = &inbox.cards[0];
    assert_eq!(
        (
            wake.reason,
            wake.revision,
            wake.card_id.as_str(),
            wake.card_title.as_str(),
            wake.board_title.as_str(),
            wake.column_title.as_str(),
            wake.column_kind,
            wake.assignee_id.as_deref(),
        ),
        (
            CardWakeReason::Assigned,
            1,
            login.id.as_str(),
            "Fix the login redirect",
            "Release",
            "Todo",
            Some(ColumnKind::Todo),
            Some(beta.id.as_str()),
        )
    );
    let trigger = inbox.trigger.as_ref().unwrap();
    assert_eq!(trigger.trigger, "card");
    assert!(trigger.deliveries.is_empty());
    assert_eq!(
        trigger.card_wakes,
        vec![CardWakeRef {
            id: wake.id.clone(),
            revision: 1
        }]
    );

    // 重复提交同一负责人、已有的 @ 都不算变化；新增的 @ 合并进同一条。
    card(
        &fixture,
        &alpha_token,
        AgentCommand::CardAssign {
            card_id: login.id.clone(),
            assignee_id: beta.id.clone(),
        },
    )
    .await;
    assert_eq!(
        pending(&fixture, &beta.id).await,
        vec![(login.id.clone(), "assigned".to_string(), 1, None)]
    );
    let ask = format!("Please @{} check it.", beta.id);
    card(&fixture, &alpha_token, update(&login.id, &ask)).await;
    card(
        &fixture,
        &alpha_token,
        update(&login.id, &format!("{ask} Again.")),
    )
    .await;
    assert_eq!(
        pending(&fixture, &beta.id).await,
        vec![(login.id.clone(), "mentioned".to_string(), 2, None)]
    );

    // 发起者自己与已归档的 Agent 不唤醒。
    fixture
        .desktop(DesktopCommand::ArchiveAgent {
            agent_id: gamma.id.clone(),
        })
        .await;
    card(
        &fixture,
        &alpha_token,
        update(
            &login.id,
            &format!("{ask} Again. cc @{} @{}", alpha.id, gamma.id),
        ),
    )
    .await;
    assert!(pending(&fixture, &alpha.id).await.is_empty());
    assert!(pending(&fixture, &gamma.id).await.is_empty());
    assert_eq!(
        pending(&fixture, &beta.id).await,
        vec![(login.id.clone(), "mentioned".to_string(), 2, None)]
    );

    let review = card(
        &fixture,
        &alpha_token,
        AgentCommand::CardCreate {
            board_id: board.id.clone(),
            column_id: board.columns[1].id.clone(),
            title: format!("Review the API with @{}", beta.id),
            description: None,
            assignee_id: None,
        },
    )
    .await;
    assert_eq!(
        pending(&fixture, &beta.id)
            .await
            .into_iter()
            .map(|(card_id, reason, ..)| (card_id, reason))
            .collect::<Vec<_>>(),
        vec![
            (login.id.clone(), "mentioned".to_string()),
            (review.id.clone(), "mentioned".to_string())
        ]
    );

    // 失败的 Run 不结算。
    let inbox = fixture.inbox(&beta_token).await;
    assert_eq!(wake_refs(&inbox).len(), 2);
    let failed = fixture.open_run(&beta_token, &inbox).await;
    assert!(
        pending(&fixture, &beta.id)
            .await
            .iter()
            .all(|(.., run_id)| run_id.as_deref() == Some(failed.id.as_str()))
    );
    fixture.finish(&beta_token, &failed.id, "failed").await;
    assert_eq!(pending(&fixture, &beta.id).await.len(), 2);

    // 读完收件箱、打开 Run 之前 Review 被改派给 Beta：版本号变了，这个 Run 不认领它。
    let inbox = fixture.inbox(&beta_token).await;
    let assign = |card_id: &str, assignee: &str| DesktopCommand::AssignCard {
        card_id: card_id.to_string(),
        assignee_id: Some(assignee.to_string()),
    };
    fixture.desktop(assign(&review.id, &beta.id)).await;
    let working = fixture.open_run(&beta_token, &inbox).await;
    assert_eq!(
        pending(&fixture, &beta.id).await,
        vec![
            (
                login.id.clone(),
                "mentioned".to_string(),
                2,
                Some(working.id.clone())
            ),
            (review.id.clone(), "assigned".to_string(), 2, None)
        ]
    );
    // Run 进行中 Login 被改派走再改派回来：合并让它脱离这个 Run，Run 成功后两条都仍待处理。
    fixture.desktop(assign(&login.id, "local-user")).await;
    fixture.desktop(assign(&login.id, &beta.id)).await;
    fixture.finish(&beta_token, &working.id, "completed").await;
    assert_eq!(
        pending(&fixture, &beta.id).await,
        vec![
            (login.id.clone(), "assigned".to_string(), 3, None),
            (review.id.clone(), "assigned".to_string(), 2, None)
        ]
    );
    let inbox = fixture.inbox(&beta_token).await;
    let settled = fixture.open_run(&beta_token, &inbox).await;
    fixture.finish(&beta_token, &settled.id, "completed").await;
    assert!(pending(&fixture, &beta.id).await.is_empty());
    fixture.desktop(assign(&review.id, "local-user")).await;
    fixture.desktop(assign(&review.id, &beta.id)).await;

    // 同批的未读消息随卡片 Turn 一起交付、一起结算。
    let direct = fixture.create_direct(&beta.id).await;
    fixture
        .send_user(&direct.id, "The API review is urgent.")
        .await;
    let inbox = fixture.inbox(&beta_token).await;
    let trigger = inbox.trigger.as_ref().unwrap();
    assert_eq!(trigger.trigger, "card");
    assert_eq!(trigger.deliveries.len(), 1);
    assert_eq!(inbox.cards.len(), 1);
    assert_eq!(inbox.messages[0].body, "The API review is urgent.");
    let last = fixture.open_run(&beta_token, &inbox).await;
    fixture.finish(&beta_token, &last.id, "completed").await;
    assert!(pending(&fixture, &beta.id).await.is_empty());
    let inbox = fixture.inbox(&beta_token).await;
    assert!(inbox.trigger.is_none());
    assert!(inbox.cards.is_empty());

    fixture
        .finish(&alpha_token, &alpha_run.id, "completed")
        .await;
    fixture.stop().await;
}

/// collaboration.md §16 #14: Agent-triggered card wakes share the 30-per-minute wake limit with
/// message wakes; wakes over the limit are not written. Desktop-triggered wakes are not limited.
/// One card Turn carries at most the ten oldest wakes and counts the rest.
#[tokio::test]
async fn acc_14_agent_card_wakes_are_rate_limited_and_a_turn_carries_at_most_ten() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = agent(&fixture, "Alpha").await;
    let beta = agent(&fixture, "Beta").await;
    let (alpha_token, alpha_run) = start_run(&fixture, &alpha.id).await;
    let board = create_board(&fixture).await;

    for index in 0..31 {
        card(
            &fixture,
            &alpha_token,
            create(&board, &format!("Task {index}"), Some(&beta.id)),
        )
        .await;
    }
    assert_eq!(pending(&fixture, &beta.id).await.len(), 30);

    let spare = card(&fixture, &alpha_token, create(&board, "Spare", None)).await;
    fixture
        .desktop(DesktopCommand::AssignCard {
            card_id: spare.id.clone(),
            assignee_id: Some(beta.id.clone()),
        })
        .await;
    assert_eq!(pending(&fixture, &beta.id).await.len(), 31);

    // 一轮最多 10 张，按首次写入的先后；其余留到下一轮，并报告还有几张。
    let beta_token = fixture.token(&beta.id).await;
    let titles = |inbox: &InboxResponse| {
        inbox
            .cards
            .iter()
            .map(|card| card.card_title.clone())
            .collect::<Vec<_>>()
    };
    let inbox = fixture.inbox(&beta_token).await;
    assert_eq!(
        titles(&inbox),
        (0..10)
            .map(|index| format!("Task {index}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(inbox.more_cards, 21);
    assert_eq!(wake_refs(&inbox).len(), 10);
    let first = fixture.open_run(&beta_token, &inbox).await;
    fixture.finish(&beta_token, &first.id, "completed").await;
    assert_eq!(pending(&fixture, &beta.id).await.len(), 21);
    let inbox = fixture.inbox(&beta_token).await;
    assert_eq!(
        titles(&inbox),
        (10..20)
            .map(|index| format!("Task {index}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(inbox.more_cards, 11);

    fixture
        .finish(&alpha_token, &alpha_run.id, "completed")
        .await;
    fixture.stop().await;
}
