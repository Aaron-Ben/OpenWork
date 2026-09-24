#![cfg(unix)]

use openwork_collab::protocol::{AgentCommand, AgentCommandResult, InboxResponse, MuteView};

#[path = "support/room_fixture.rs"]
pub mod room_fixture;

use room_fixture::Fixture;

fn bodies(inbox: &InboxResponse, room_id: &str) -> Vec<String> {
    inbox
        .messages
        .iter()
        .filter(|message| message.room_id == room_id)
        .map(|message| message.body.clone())
        .collect()
}

/// 用户在 Agent 的私聊里发一句，打开一个 Run，之后才能发写命令。
async fn open_run(fixture: &Fixture, token: &str, direct_room: &str, body: &str) -> String {
    fixture.send_user(direct_room, body).await;
    let inbox = fixture.inbox(token).await;
    fixture.open_run(token, &inbox).await.id
}

fn mute(room_id: &str, for_minutes: Option<u32>, until: Option<&str>) -> AgentCommand {
    AgentCommand::Mute {
        room_id: room_id.to_string(),
        for_minutes,
        until: until.map(str::to_string),
    }
}

fn rejected(code: &str, message: &str) -> AgentCommandResult {
    AgentCommandResult::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

/// collaboration.md §10.1、§16 #23：静音后群消息不唤醒、不进收件箱，`@` 仍送达；静音时封住未读尾巴，
/// follow 后不补发积压；`mute list` 只列仍在静音的房间。
#[tokio::test]
async fn acc_23_a_muted_group_only_delivers_mentions_and_follow_skips_the_backlog() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    let token = fixture.token(&alpha.id).await;

    fixture.send_user(&group.id, "Kickoff").await;
    let inbox = fixture.inbox(&token).await;
    let run = fixture.open_run(&token, &inbox).await;
    fixture.send_user(&group.id, "Backlog before mute").await;

    let muted = fixture.command(&token, mute(&group.id, None, None)).await;
    assert_eq!(
        muted.result,
        AgentCommandResult::Muted {
            participant_id: alpha.id.clone(),
            mute: MuteView {
                room_id: group.id.clone(),
                title: Some("R5 coordination".to_string()),
                expires_at: None,
            },
        }
    );
    let last_read: i64 = sqlx::query_scalar(
        "SELECT last_read_seq FROM collab_room_members WHERE room_id = $1 AND participant_id = $2",
    )
    .bind(&group.id)
    .bind(&alpha.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(last_read, 2, "muting seals the unread tail");
    assert_eq!(
        fixture.command(&token, AgentCommand::MuteList).await.result,
        AgentCommandResult::Mutes {
            mutes: vec![MuteView {
                room_id: group.id.clone(),
                title: Some("R5 coordination".to_string()),
                expires_at: None,
            }],
        }
    );
    fixture.finish(&token, &run.id, "completed").await;

    // 普通群消息不进收件箱；@ 自己的消息送达，连同封尾之后的消息，但不含静音前的积压。
    fixture.send_user(&group.id, "General chatter").await;
    let inbox = fixture.inbox(&token).await;
    assert!(inbox.trigger.is_none());
    assert!(inbox.messages.is_empty());
    fixture
        .send_user(&group.id, &format!("@{} please look", alpha.id))
        .await;
    let inbox = fixture.inbox(&token).await;
    assert_eq!(
        bodies(&inbox, &group.id),
        vec![
            "General chatter".to_string(),
            format!("@{} please look", alpha.id)
        ]
    );
    let run = fixture.open_run(&token, &inbox).await;
    let follow = AgentCommand::Follow {
        room_id: group.id.clone(),
    };
    assert_eq!(
        fixture.command(&token, follow.clone()).await.result,
        AgentCommandResult::Followed {
            room_id: group.id.clone(),
            was_muted: true,
        }
    );
    assert_eq!(
        fixture.command(&token, follow).await.result,
        AgentCommandResult::Followed {
            room_id: group.id.clone(),
            was_muted: false,
        }
    );
    fixture.finish(&token, &run.id, "completed").await;
    fixture.send_user(&group.id, "After follow").await;
    let inbox = fixture.inbox(&token).await;
    assert_eq!(bodies(&inbox, &group.id), vec!["After follow".to_string()]);
    let run = fixture.open_run(&token, &inbox).await;

    // --for 到期后自动恢复，也不再出现在 mute list 里。
    let timed = fixture
        .command(&token, mute(&group.id, Some(90), None))
        .await;
    let AgentCommandResult::Muted { mute: view, .. } = timed.result else {
        panic!("mute --for returned {:?}", timed.result)
    };
    let expires_at = view.expires_at.expect("a timed mute has an expiry");
    assert!(expires_at.ends_with("+08:00"), "{expires_at}");
    fixture.finish(&token, &run.id, "completed").await;
    sqlx::query(
        "UPDATE collab_room_members
         SET mute_expires_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '1 minute'
         WHERE room_id = $1 AND participant_id = $2",
    )
    .bind(&group.id)
    .bind(&alpha.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    fixture.send_user(&group.id, "After expiry").await;
    let inbox = fixture.inbox(&token).await;
    assert_eq!(bodies(&inbox, &group.id), vec!["After expiry".to_string()]);
    let run = fixture.open_run(&token, &inbox).await;
    assert_eq!(
        fixture.command(&token, AgentCommand::MuteList).await.result,
        AgentCommandResult::Mutes { mutes: Vec::new() }
    );
    fixture.finish(&token, &run.id, "completed").await;

    fixture.stop().await;
}

/// collaboration.md §10.1、§16 #23：Direct Room、非成员房间和不合法的期限都被拒，且不改动成员状态。
#[tokio::test]
async fn acc_23_direct_rooms_foreign_rooms_and_bad_spans_are_rejected() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    let gamma = fixture.create_agent("Gamma").await;
    let foreign = fixture
        .create_group(vec![beta.id.clone(), gamma.id.clone()])
        .await;
    let direct = fixture.create_direct(&alpha.id).await;
    let token = fixture.token(&alpha.id).await;
    let run = open_run(&fixture, &token, &direct.id, "Open a run.").await;

    let cases = [
        (
            mute(&direct.id, None, None),
            rejected(
                "INVALID_ARGUMENT",
                "direct rooms always deliver; mute a group instead",
            ),
        ),
        (
            mute(&foreign.id, None, None),
            rejected(
                "NOT_FOUND",
                &format!("you are not a member of {}", foreign.id),
            ),
        ),
        (
            mute(&group.id, Some(0), None),
            rejected(
                "INVALID_ARGUMENT",
                "--for duration must be between 1 minute and 90 days",
            ),
        ),
        (
            mute(&group.id, Some(90 * 24 * 60 + 1), None),
            rejected(
                "INVALID_ARGUMENT",
                "--for duration must be between 1 minute and 90 days",
            ),
        ),
        (
            mute(&group.id, None, Some("2020-01-01T00:00:00+08:00")),
            rejected("INVALID_ARGUMENT", "--until must be in the future"),
        ),
        (
            mute(&group.id, None, Some("tomorrow")),
            rejected("INVALID_ARGUMENT", "invalid --until timestamp"),
        ),
        (
            mute(&group.id, Some(30), Some("2099-01-01T00:00:00+08:00")),
            rejected("INVALID_ARGUMENT", "use either --until or --for, not both"),
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(fixture.command(&token, command).await.result, expected);
    }
    let muted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM collab_room_members WHERE mute_expires_at IS NOT NULL",
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(muted, 0);

    fixture.finish(&token, &run, "completed").await;
    fixture.stop().await;
}
