#![cfg(unix)]

use openwork_collab::protocol::{
    AgentCommand, AgentCommandResult, DesktopCommand, DesktopCommandRequest, DesktopCommandResult,
    LastMessageView, RoomNoteView, RoomSnapshotView, RoomSummaryView, request_id,
};

#[path = "support/room_fixture.rs"]
pub mod room_fixture;

use room_fixture::Fixture;

async fn rooms(fixture: &Fixture) -> Vec<RoomSummaryView> {
    let DesktopCommandResult::Rooms { rooms } = fixture.desktop(DesktopCommand::ListRooms).await
    else {
        panic!("ListRooms returned the wrong result")
    };
    rooms
}

async fn room(fixture: &Fixture, room_id: &str) -> RoomSummaryView {
    rooms(fixture)
        .await
        .into_iter()
        .find(|room| room.id == room_id)
        .expect("room is listed")
}

async fn snapshot(fixture: &Fixture, room_id: &str) -> RoomSnapshotView {
    let DesktopCommandResult::RoomSnapshot(snapshot) = fixture
        .desktop(DesktopCommand::OpenRoom {
            room_id: room_id.to_string(),
        })
        .await
    else {
        panic!("OpenRoom returned the wrong result")
    };
    *snapshot
}

async fn pin(fixture: &Fixture, room_id: &str, pinned: bool) {
    let result = fixture
        .desktop(DesktopCommand::PinRoom {
            room_id: room_id.to_string(),
            pinned,
        })
        .await;
    assert_eq!(
        result,
        DesktopCommandResult::RoomPinned {
            room_id: room_id.to_string(),
            pinned
        }
    );
}

/// 发出 Desktop 命令并返回失败时的状态码与正文。
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

/// Ada 读收件箱、开 Run、依次执行 `commands`，然后结束 Run；返回各命令的结果。
async fn ada_runs(
    fixture: &Fixture,
    token: &str,
    commands: Vec<AgentCommand>,
) -> Vec<AgentCommandResult> {
    let inbox = fixture.inbox(token).await;
    let run = fixture.open_run(token, &inbox).await;
    let mut results = Vec::new();
    for command in commands {
        results.push(fixture.command(token, command).await.result);
    }
    fixture.finish(token, &run.id, "completed").await;
    results
}

fn reply(room_id: &str, body: &str, quoted: Option<String>) -> AgentCommand {
    AgentCommand::Reply {
        room_id: room_id.to_string(),
        body: body.to_string(),
        held_token: None,
        quoted_message_id: quoted,
        continuation: false,
    }
}

/// collaboration-desktop.md §4.2、§4.5、§7.1：房间列表带未读数、最近消息、成员（用户在前）、是否成员
/// 与置顶；看过后未读归零；置顶可以取消，房间不存在时拒绝。
#[tokio::test]
async fn acc_15_room_list_carries_unread_last_message_members_and_pin() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let direct = fixture.create_direct(&ada.id).await;
    let group = fixture
        .create_group(vec![ada.id.clone(), bo.id.clone()])
        .await;
    let ada_token = fixture.token(&ada.id).await;

    fixture.send_user(&group.id, "Hello team").await;
    let results = ada_runs(
        &fixture,
        &ada_token,
        vec![
            reply(&group.id, "On it.", None),
            AgentCommand::DirectMessage {
                participant_id: bo.id.clone(),
                body: "Can you review?".to_string(),
            },
        ],
    )
    .await;
    let AgentCommandResult::DirectMessageSent {
        room_id: whisper_id,
        ..
    } = &results[1]
    else {
        panic!("DM returned {:?}", results[1])
    };

    let listed = room(&fixture, &group.id).await;
    assert_eq!(listed.unread_count, 1);
    assert_eq!(
        listed.last_message,
        Some(LastMessageView {
            author_name: "Ada".to_string(),
            body: "On it.".to_string()
        })
    );
    assert!(
        listed
            .last_message_at
            .as_deref()
            .is_some_and(|at| at.ends_with("+08:00"))
    );
    assert_eq!(
        listed.member_ids,
        vec!["local-user", ada.id.as_str(), bo.id.as_str()]
    );
    assert!(listed.user_is_member && !listed.pinned);
    let quiet = room(&fixture, &direct.id).await;
    assert_eq!(
        (
            quiet.unread_count,
            quiet.last_message,
            quiet.title.as_deref()
        ),
        (0, None, Some("Ada"))
    );

    fixture
        .desktop(DesktopCommand::RoomViewed {
            room_id: group.id.clone(),
            up_to_seq: 2,
        })
        .await;
    assert_eq!(room(&fixture, &group.id).await.unread_count, 0);

    pin(&fixture, &group.id, true).await;
    pin(&fixture, &group.id, true).await;
    assert!(room(&fixture, &group.id).await.pinned);
    pin(&fixture, &group.id, false).await;
    assert!(!room(&fixture, &group.id).await.pinned);
    let (status, body) = rejected(
        &fixture,
        DesktopCommand::PinRoom {
            room_id: "room-missing".to_string(),
            pinned: true,
        },
    )
    .await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("Room does not exist"), "{body}");

    // Agent 之间的私聊：用户不是成员，只由“Agent 私聊”页展示。
    let whisper = room(&fixture, whisper_id).await;
    assert!(!whisper.user_is_member);
    assert_eq!(whisper.member_ids, vec![ada.id.as_str(), bo.id.as_str()]);

    fixture.stop().await;
}

/// collaboration-desktop.md §4.2、§7.2、§7.3：快照里的消息带作者名、类型、role、时间与引用；路由说明行
/// 来自本房间的 triage；房间不存在时拒绝。
#[tokio::test]
async fn acc_12_room_snapshot_carries_authors_quotes_and_notes() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let cy = fixture.create_agent("Cy").await;
    let group = fixture
        .create_group(vec![ada.id.clone(), bo.id.clone(), cy.id.clone()])
        .await;
    let ada_token = fixture.token(&ada.id).await;
    fixture
        .send_user(&group.id, &format!("@{} can you check the index?", bo.id))
        .await;
    let first = snapshot(&fixture, &group.id).await.messages[0]
        .message
        .id
        .clone();
    ada_runs(
        &fixture,
        &ada_token,
        vec![reply(&group.id, "Bo owns it.", Some(first.clone()))],
    )
    .await;
    for agent in [&ada.id, &cy.id] {
        sqlx::query(
            "INSERT INTO collab_triages (id, agent_id, runtime_session_id, room_id, up_to_seq,
                 actionable, source, engine_id, response_mode)
             VALUES ($1, $2, 'runtime-test', $3, 1, FALSE, 'routing', 'opencode', 'me')",
        )
        .bind(format!("triage-{}", uuid::Uuid::new_v4().simple()))
        .bind(agent)
        .bind(&group.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    }

    let opened = snapshot(&fixture, &group.id).await;
    assert_eq!(opened.room_id, group.id);
    let [user, reply] = &opened.messages[..] else {
        panic!("expected two messages, got {:?}", opened.messages)
    };
    assert_eq!(
        (
            user.author_name.as_str(),
            user.author_kind.as_str(),
            user.author_role.as_deref()
        ),
        ("User", "user", None)
    );
    assert_eq!(
        (
            reply.author_name.as_str(),
            reply.author_kind.as_str(),
            reply.author_role.as_deref()
        ),
        ("Ada", "agent", Some("Collaborator"))
    );
    assert!(reply.created_at.ends_with("+08:00"), "{}", reply.created_at);
    assert_eq!(
        reply
            .message
            .quoted
            .as_ref()
            .map(|quoted| quoted.id.as_str()),
        Some(first.as_str())
    );
    assert_eq!(
        opened.notes,
        vec![RoomNoteView::Routing {
            after_sequence: 1,
            skipped_names: vec!["Ada".to_string(), "Cy".to_string()],
            target_names: vec!["Bo".to_string()],
        }]
    );

    // 邀请成员写入的系统消息作者也是用户，但不是人类发言，不能把对话切成两段（§7.3）。
    let dee = fixture.create_agent("Dee").await;
    fixture
        .desktop(DesktopCommand::AddGroupMember {
            room_id: group.id.clone(),
            agent_id: dee.id.clone(),
        })
        .await;
    for up_to_seq in [2_i64, 3] {
        sqlx::query(
            "INSERT INTO collab_triages (id, agent_id, runtime_session_id, room_id, up_to_seq,
                 actionable, source, engine_id)
             VALUES ($1, $2, 'runtime-test', $3, $4, FALSE, 'lap_floor', 'opencode')",
        )
        .bind(format!("triage-{}", uuid::Uuid::new_v4().simple()))
        .bind(&cy.id)
        .bind(&group.id)
        .bind(up_to_seq)
        .execute(&fixture.pool)
        .await
        .unwrap();
    }
    let reopened = snapshot(&fixture, &group.id).await;
    assert_eq!(
        reopened.notes[1..],
        [RoomNoteView::LapFloor {
            after_sequence: 2,
            speaker_name: "Ada".to_string(),
        }]
    );

    let (status, body) = rejected(
        &fixture,
        DesktopCommand::OpenRoom {
            room_id: "room-missing".to_string(),
        },
    )
    .await;
    assert_eq!(status, 404, "{body}");
    fixture.stop().await;
}
