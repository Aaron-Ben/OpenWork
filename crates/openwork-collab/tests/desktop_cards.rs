#![cfg(unix)]

use openwork_collab::protocol::{
    AgentCommand, AgentCommandResult, BoardView, CardChangeView, CardView, DesktopCommand,
    DesktopCommandRequest, DesktopCommandResult, request_id,
};

#[path = "support/room_fixture.rs"]
pub mod room_fixture;

use room_fixture::Fixture;

async fn card_change(fixture: &Fixture, command: DesktopCommand) -> CardChangeView {
    match fixture.desktop(command).await {
        DesktopCommandResult::Card(change) => change,
        result => panic!("card command returned {result:?}"),
    }
}

async fn board(fixture: &Fixture) -> BoardView {
    let DesktopCommandResult::Boards { boards } = fixture.desktop(DesktopCommand::ListBoards).await
    else {
        panic!("ListBoards returned the wrong result")
    };
    boards.into_iter().next().expect("one board")
}

async fn pending_wakes(fixture: &Fixture, card_id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT agent_id FROM collab_card_wakes
         WHERE card_id = $1 AND settled_at IS NULL ORDER BY agent_id",
    )
    .bind(card_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap()
}

async fn rejected(fixture: &Fixture, command: DesktopCommand) -> (u16, String) {
    let response = fixture
        .http
        .post(format!("{}/desktop/commands", fixture.base_url))
        .bearer_auth(&fixture.desktop_secret)
        .json(&DesktopCommandRequest {
            request_id: command.is_mutating().then(request_id),
            command,
        })
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

fn titles(cards: &[CardView]) -> Vec<&str> {
    cards.iter().map(|card| card.title.as_str()).collect()
}

/// collaboration.md §11.2、§11.4，collaboration-desktop.md §4.3：Desktop 建卡、编辑与移动卡片；
/// 新建时指定负责人与新增的 `@` 叫醒对应的 Agent 并在结果里列出；Desktop 读取的卡片带更新时间，
/// Agent 命令看到的卡片不带。
#[tokio::test]
async fn acc_14_desktop_creates_edits_and_moves_cards_and_wakes_who_it_names() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let DesktopCommandResult::Board(created) = fixture
        .desktop(DesktopCommand::CreateBoard {
            title: "Release".to_string(),
            description: None,
        })
        .await
    else {
        panic!("CreateBoard returned the wrong result")
    };
    let todo = created.columns[0].id.clone();
    let doing = created.columns[1].id.clone();

    let first = card_change(
        &fixture,
        DesktopCommand::CreateCard {
            board_id: created.id.clone(),
            column_id: todo.clone(),
            title: "Backfill in batches".to_string(),
            description: None,
            assignee_id: Some(ada.id.clone()),
        },
    )
    .await;
    assert_eq!(first.woken_agent_ids, vec![ada.id.clone()]);
    assert_eq!(first.card.created_by, "local-user");
    assert_eq!(
        pending_wakes(&fixture, &first.card.id).await,
        vec![ada.id.clone()]
    );
    let second = card_change(
        &fixture,
        DesktopCommand::CreateCard {
            board_id: created.id.clone(),
            column_id: todo.clone(),
            title: "Rollback drill".to_string(),
            description: None,
            assignee_id: None,
        },
    )
    .await;
    assert!(second.woken_agent_ids.is_empty());

    // 只改描述：新增的 @bo 叫醒 Bo；已经在的 @bo 再保存不重复叫醒。
    let edited = card_change(
        &fixture,
        DesktopCommand::UpdateCard {
            card_id: second.card.id.clone(),
            title: None,
            description: Some(format!("@{} please rehearse it", bo.id)),
        },
    )
    .await;
    assert_eq!(
        (edited.card.title.as_str(), edited.woken_agent_ids.clone()),
        ("Rollback drill", vec![bo.id.clone()])
    );
    let renamed = card_change(
        &fixture,
        DesktopCommand::UpdateCard {
            card_id: second.card.id.clone(),
            title: Some("Rollback rehearsal".to_string()),
            description: None,
        },
    )
    .await;
    assert!(renamed.woken_agent_ids.is_empty());
    assert_eq!(
        renamed.card.description.as_deref(),
        Some(format!("@{} please rehearse it", bo.id).as_str())
    );
    let (status, body) = rejected(
        &fixture,
        DesktopCommand::UpdateCard {
            card_id: second.card.id.clone(),
            title: None,
            description: None,
        },
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("nothing to update"), "{body}");

    // 同列重排与换列。
    card_change(
        &fixture,
        DesktopCommand::MoveCard {
            card_id: second.card.id.clone(),
            column_id: todo.clone(),
            before_card_id: Some(first.card.id.clone()),
        },
    )
    .await;
    assert_eq!(
        titles(&board(&fixture).await.columns[0].cards),
        vec!["Rollback rehearsal", "Backfill in batches"]
    );
    let moved = card_change(
        &fixture,
        DesktopCommand::MoveCard {
            card_id: first.card.id.clone(),
            column_id: doing.clone(),
            before_card_id: None,
        },
    )
    .await;
    assert!(moved.woken_agent_ids.is_empty());
    let listed = board(&fixture).await;
    assert_eq!(titles(&listed.columns[0].cards), vec!["Rollback rehearsal"]);
    assert_eq!(
        titles(&listed.columns[1].cards),
        vec!["Backfill in batches"]
    );
    assert!(
        listed.columns[1].cards[0]
            .updated_at
            .as_deref()
            .is_some_and(|at| at.ends_with("+08:00"))
    );

    // Agent 命令的卡片不带 updatedAt，模型看到的输出不变。
    let ada_token = fixture.token(&ada.id).await;
    let shown = fixture
        .command(
            &ada_token,
            AgentCommand::BoardShow {
                board_id: created.id.clone(),
            },
        )
        .await;
    assert!(matches!(shown.result, AgentCommandResult::Board { .. }));
    let json = serde_json::to_string(&shown.result).unwrap();
    assert!(!json.contains("updatedAt"), "{json}");
    fixture.stop().await;
}
