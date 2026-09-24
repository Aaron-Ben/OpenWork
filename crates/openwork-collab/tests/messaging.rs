#![cfg(unix)]

use openwork_collab::protocol::{
    AgentCommand, AgentCommandResult, DesktopCommand, DesktopCommandResult, QuotedMessageView,
    ResponseMode, RoomView, TeamMember, TriageReportRequest,
};
use uuid::Uuid;

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

/// collaboration.md §8.4、§16 #7：被点名的是别人时，Agent 完成 Turn 后既不回复也不 ack。
/// 这批 delivery 仍要结算，否则同一条 User 消息会在每次 poll 重新触发完整 Turn。
#[tokio::test]
async fn a_completed_silent_run_settles_its_delivery_so_the_agent_is_not_woken_again() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let room = fixture.create_direct(&alpha.id).await;
    fixture
        .send_user(&room.id, "Bo, this one is for you.")
        .await;
    let token = fixture.token(&alpha.id).await;
    let inbox = fixture.inbox(&token).await;
    let run = fixture.open_run(&token, &inbox).await;
    fixture.triage(&token, &run.id).await;

    let finished = fixture.finish(&token, &run.id, "completed").await;

    assert_eq!(finished.outcome.as_deref(), Some("silent"));
    let last_read: i64 = sqlx::query_scalar(
        "SELECT last_read_seq FROM collab_room_members
         WHERE room_id = $1 AND participant_id = $2",
    )
    .bind(&room.id)
    .bind(&alpha.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(last_read, 1);
    let (reason, settled): (Option<String>, bool) = sqlx::query_as(
        "SELECT eligible_reason, settled_at IS NOT NULL
         FROM collab_run_deliveries WHERE run_id = $1",
    )
    .bind(&run.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("completed"));
    assert!(settled);
    let next = fixture.inbox(&token).await;
    assert!(next.messages.is_empty());
    assert!(next.trigger.is_none());
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
async fn inbox_water_fills_each_unread_room_before_spending_slack_on_a_busy_room() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let busy = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    let quiet = fixture.create_direct(&alpha.id).await;
    let prefix = Uuid::new_v4().simple().to_string();
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
         SELECT 'msg-' || md5($1 || sequence::TEXT), $2, sequence, 'local-user', 'normal',
                'busy message ' || sequence::TEXT
         FROM generate_series(1, 200) AS sequence",
    )
    .bind(prefix)
    .bind(&busy.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE collab_rooms SET next_seq = 200 WHERE id = $1")
        .bind(&busy.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    fixture
        .send_user(&quiet.id, "quiet room must keep its own inbox window")
        .await;

    let token = fixture.token(&alpha.id).await;
    let inbox = fixture.inbox(&token).await;
    assert_eq!(inbox.messages.len(), 200);
    assert!(inbox.carried_over);
    assert_eq!(
        inbox
            .messages
            .iter()
            .filter(|message| message.room_id == busy.id)
            .count(),
        199
    );
    assert!(inbox.messages.iter().any(|message| {
        message.room_id == quiet.id && message.body == "quiet room must keep its own inbox window"
    }));

    fixture.stop().await;
}

/// collaboration.md §8.3、§16 #10：人类消息之后 Ada、Bo、Cy 各说一次、Ada 又说第二次，Bo 的
/// triage 以 `lap_floor` 确定性跳过；用户在 Desktop 看到这些消息后计数重新开始，再来一条
/// Agent 消息就回到 triage 模型。看到的位置只增不减，也不超过房间最后一条消息。
#[tokio::test]
async fn acc_10_a_second_lap_is_skipped_until_the_user_looks_again() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let cy = fixture.create_agent("Cy").await;
    let group = fixture
        .create_group(vec![ada.id.clone(), bo.id.clone(), cy.id.clone()])
        .await;
    fixture.send_user(&group.id, "Discuss the rollout.").await;
    let insert_agent_message = |sequence: i64, author: String| {
        let pool = fixture.pool.clone();
        let room_id = group.id.clone();
        async move {
            sqlx::query(
                "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
                 VALUES ('msg-' || md5($1 || $2::TEXT), $1, $2, $3, 'normal', 'turn ' || $2::TEXT)",
            )
            .bind(&room_id)
            .bind(sequence)
            .bind(&author)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE collab_rooms SET next_seq = $2 WHERE id = $1")
                .bind(&room_id)
                .bind(sequence)
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    for (sequence, author) in [(2, &ada.id), (3, &bo.id), (4, &cy.id), (5, &ada.id)] {
        insert_agent_message(sequence, author.clone()).await;
    }
    let read_up_to = |sequence: i64| {
        let pool = fixture.pool.clone();
        let (room_id, agent_id) = (group.id.clone(), bo.id.clone());
        async move {
            sqlx::query(
                "UPDATE collab_room_members SET last_read_seq = $3
                 WHERE room_id = $1 AND participant_id = $2",
            )
            .bind(&room_id)
            .bind(&agent_id)
            .bind(sequence)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    read_up_to(4).await;
    let token = fixture.token(&bo.id).await;
    let inbox = fixture.inbox(&token).await;
    let run = fixture.open_run(&token, &inbox).await;
    let lapping = fixture.triage(&token, &run.id).await.verdict.unwrap();
    assert_eq!(
        (lapping.actionable, lapping.source.as_str()),
        (false, "lap_floor")
    );
    fixture.finish(&token, &run.id, "completed").await;

    let viewed = fixture
        .desktop(DesktopCommand::RoomViewed {
            room_id: group.id.clone(),
            up_to_seq: 99,
        })
        .await;
    assert_eq!(
        viewed,
        DesktopCommandResult::RoomViewed {
            room_id: group.id.clone(),
            user_viewed_seq: 5,
        }
    );
    let rewound = fixture
        .desktop(DesktopCommand::RoomViewed {
            room_id: group.id.clone(),
            up_to_seq: 2,
        })
        .await;
    assert_eq!(
        rewound,
        DesktopCommandResult::RoomViewed {
            room_id: group.id.clone(),
            user_viewed_seq: 5,
        }
    );
    insert_agent_message(6, cy.id.clone()).await;
    read_up_to(5).await;
    let inbox = fixture.inbox(&token).await;
    let run = fixture.open_run(&token, &inbox).await;
    let watched = fixture.triage(&token, &run.id).await;
    assert_eq!(watched.verdict, None);
    assert!(watched.routing.is_none());
    assert!(watched.instructions.is_some());

    fixture.stop().await;
}

/// collaboration.md §8.2、§8.3、§16 #9：人类只点名 Bo 时，Bo 直接参与；Ada 先拿到路由题，
/// 答 `me` 时以 `routing` 跳过并结算 delivery、记下 response_mode，答 `each` 时参与；
/// `@all` 永不收窄。
#[tokio::test]
async fn acc_09_a_message_naming_one_agent_asks_the_others_to_route_it() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let cy = fixture.create_agent("Cy").await;
    let group = fixture
        .create_group(vec![ada.id.clone(), bo.id.clone(), cy.id.clone()])
        .await;
    fixture
        .send_user(&group.id, &format!("@{} check the migration", bo.id))
        .await;

    let bo_token = fixture.token(&bo.id).await;
    let bo_inbox = fixture.inbox(&bo_token).await;
    let bo_run = fixture.open_run(&bo_token, &bo_inbox).await;
    let bo_verdict = fixture.triage(&bo_token, &bo_run.id).await.verdict.unwrap();
    assert_eq!(
        (bo_verdict.actionable, bo_verdict.source.as_str()),
        (true, "deterministic")
    );

    let ada_token = fixture.token(&ada.id).await;
    let ada_inbox = fixture.inbox(&ada_token).await;
    let ada_run = fixture.open_run(&ada_token, &ada_inbox).await;
    let question = fixture.triage(&ada_token, &ada_run.id).await;
    assert_eq!(question.verdict, None);
    let routing = question
        .routing
        .expect("Ada must be asked to route the message");
    assert_eq!(
        routing.input,
        format!(
            "Named agents: {}\nOther agents in the room: {}, {}\n\nMessage:\n@{} check the migration",
            bo.id, ada.id, cy.id, bo.id
        )
    );
    let narrowed = fixture
        .triage_routed(&ada_token, &ada_run.id, "me")
        .await
        .verdict
        .unwrap();
    assert_eq!(
        (
            narrowed.actionable,
            narrowed.source.as_str(),
            narrowed.reason.as_str()
        ),
        (
            false,
            "routing",
            "every human message was addressed to other teammates"
        )
    );
    fixture
        .report_triage(
            &ada_token,
            &TriageReportRequest {
                run_id: ada_run.id.clone(),
                verdict: narrowed,
                model: "local/triage".to_string(),
                input_tokens: Some(40),
                output_tokens: Some(5),
                latency_ms: Some(10),
                response_mode: Some(ResponseMode::Me),
            },
        )
        .await;
    fixture.finish(&ada_token, &ada_run.id, "completed").await;
    let recorded: (String, Option<String>) =
        sqlx::query_as("SELECT source, response_mode FROM collab_triages WHERE run_id = $1")
            .bind(&ada_run.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(recorded, ("routing".to_string(), Some("me".to_string())));
    assert!(fixture.inbox(&ada_token).await.trigger.is_none());

    let cy_token = fixture.token(&cy.id).await;
    let cy_inbox = fixture.inbox(&cy_token).await;
    let cy_run = fixture.open_run(&cy_token, &cy_inbox).await;
    let engaged = fixture
        .triage_routed(&cy_token, &cy_run.id, "each")
        .await
        .verdict
        .unwrap();
    assert_eq!(
        (engaged.actionable, engaged.source.as_str()),
        (true, "routing")
    );
    fixture.finish(&cy_token, &cy_run.id, "completed").await;

    fixture
        .send_user(&group.id, &format!("@all and @{} please weigh in", bo.id))
        .await;
    let ada_inbox = fixture.inbox(&ada_token).await;
    let ada_run = fixture.open_run(&ada_token, &ada_inbox).await;
    let broadcast = fixture.triage(&ada_token, &ada_run.id).await;
    assert!(broadcast.routing.is_none());
    assert_eq!(broadcast.verdict.unwrap().source, "deterministic");

    fixture.stop().await;
}

/// collaboration.md §9.3、§16 #12：只能引用同一房间的消息，发布结果与 inbox 都带着被引用的原文；
/// 被引用的作者即使 mute 了房间也会收到引用它的消息，普通消息则不会。
#[tokio::test]
async fn acc_12_quotes_stay_in_the_room_and_reach_a_muted_author() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    let other = fixture.create_direct(&alpha.id).await;
    fixture
        .send_user(&group.id, "Which index should we add?")
        .await;
    fixture
        .send_user(&other.id, "Unrelated direct question.")
        .await;
    let token = fixture.token(&alpha.id).await;
    let inbox = fixture.inbox(&token).await;
    let question = inbox
        .messages
        .iter()
        .find(|message| message.room_id == group.id)
        .unwrap()
        .clone();
    let foreign = inbox
        .messages
        .iter()
        .find(|message| message.room_id == other.id)
        .unwrap()
        .clone();
    let run = fixture.open_run(&token, &inbox).await;

    let rejected = fixture
        .command(
            &token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "Wrong quote".to_string(),
                held_token: None,
                quoted_message_id: Some(foreign.id.clone()),
                continuation: false,
            },
        )
        .await;
    assert_eq!(
        rejected.result,
        AgentCommandResult::Error {
            code: "NOT_FOUND".to_string(),
            message: format!(
                "{} is not a message in {}; quote an id from this room's messages",
                foreign.id, group.id
            ),
        }
    );
    let published = fixture
        .command(
            &token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "Add a partial index.".to_string(),
                held_token: None,
                quoted_message_id: Some(question.id.clone()),
                continuation: false,
            },
        )
        .await;
    let AgentCommandResult::MessagePublished { message: reply } = published.result else {
        panic!("quoted reply was not published: {:?}", published.result)
    };
    assert_eq!(
        reply.quoted,
        Some(QuotedMessageView {
            id: question.id.clone(),
            author_id: "local-user".to_string(),
            author_name: "User".to_string(),
            body: "Which index should we add?".to_string(),
        })
    );
    fixture.finish(&token, &run.id, "completed").await;
    let group_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM collab_messages WHERE room_id = $1")
            .bind(&group.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(group_messages, 2);

    sqlx::query(
        "UPDATE collab_room_members SET mute_expires_at = 'infinity' WHERE room_id = $1 AND participant_id = $2",
    )
    .bind(&group.id)
    .bind(&alpha.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    fixture.send_user(&group.id, "Plain follow-up.").await;
    assert!(fixture.inbox(&token).await.messages.is_empty());
    let DesktopCommandResult::Message(sent) = fixture
        .desktop(DesktopCommand::SendMessage {
            room_id: group.id.clone(),
            body: "Why partial?".to_string(),
            quoted_message_id: Some(reply.id.clone()),
        })
        .await
    else {
        panic!("Desktop send returned the wrong result")
    };
    assert_eq!(
        sent.quoted
            .as_ref()
            .map(|quoted| quoted.author_name.as_str()),
        Some("Alpha")
    );
    let inbox = fixture.inbox(&token).await;
    assert_eq!(
        inbox
            .messages
            .iter()
            .map(|message| message.body.as_str())
            .collect::<Vec<_>>(),
        vec!["Plain follow-up.", "Why partial?"]
    );
    assert_eq!(
        inbox.messages[1]
            .quoted
            .as_ref()
            .map(|quoted| quoted.id.as_str()),
        Some(reply.id.as_str())
    );

    fixture.stop().await;
}

/// collaboration.md §7.2、§16 #8：inbox 带上本批房间的类型与标题，以及渲染名册所需的
/// 全部 active 参与者；已归档且没有出现在本批消息里的 Agent 不在其中。
#[tokio::test]
async fn acc_08_inbox_carries_room_headers_and_the_active_team() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let ada = fixture.create_agent("Ada").await;
    let bo = fixture.create_agent("Bo").await;
    let cy = fixture.create_agent("Cy").await;
    let room = fixture
        .create_group(vec![ada.id.clone(), bo.id.clone()])
        .await;
    fixture
        .send_user(&room.id, "Bo, check the migration.")
        .await;
    fixture
        .desktop(DesktopCommand::ArchiveAgent {
            agent_id: cy.id.clone(),
        })
        .await;

    let token = fixture.token(&ada.id).await;
    let inbox = fixture.inbox(&token).await;

    assert_eq!(
        inbox.rooms,
        vec![RoomView {
            id: room.id.clone(),
            kind: "group".to_string(),
            title: Some("R5 coordination".to_string()),
        }]
    );
    let member = |id: &str, kind: &str, name: &str, role: Option<&str>| TeamMember {
        id: id.to_string(),
        kind: kind.to_string(),
        display_name: name.to_string(),
        role: role.map(str::to_string),
        archived: false,
    };
    assert_eq!(
        inbox.team,
        vec![
            member("local-user", "user", "User", None),
            member(&ada.id, "agent", "Ada", Some("Collaborator")),
            member(&bo.id, "agent", "Bo", Some("Collaborator")),
        ]
    );

    fixture.stop().await;
}

#[tokio::test]
async fn group_agent_chatter_stops_at_the_deterministic_loop_cap() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    let seed = Uuid::new_v4().simple().to_string();
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
         SELECT 'msg-' || md5($1 || sequence::TEXT), $2, sequence, $3, 'normal',
                'agent loop message ' || sequence::TEXT
         FROM generate_series(1, 20) AS sequence",
    )
    .bind(seed)
    .bind(&group.id)
    .bind(&beta.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE collab_rooms SET next_seq = 20 WHERE id = $1")
        .bind(&group.id)
        .execute(&fixture.pool)
        .await
        .unwrap();

    let token = fixture.token(&alpha.id).await;
    let inbox = fixture.inbox(&token).await;
    let run = fixture.open_run(&token, &inbox).await;
    let triage = fixture.triage(&token, &run.id).await;
    let verdict = triage.verdict.expect("hard cap is deterministic");
    assert!(!verdict.actionable);
    assert_eq!(verdict.source, "loop_cap");
    let rejected = fixture
        .command(
            &token,
            AgentCommand::Reply {
                room_id: group.id,
                body: "This reply must not extend the Agent loop.".to_string(),
                held_token: None,
                quoted_message_id: None,
                continuation: false,
            },
        )
        .await;
    assert!(matches!(
        rejected.result,
        AgentCommandResult::Error { ref code, .. } if code == "LOOP_CAP"
    ));
    fixture.finish(&token, &run.id, "completed").await;

    fixture.stop().await;
}

#[tokio::test]
async fn direct_room_reads_and_private_directional_climate_form_one_loop() {
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
    // 两人私聊到检查点时，8 条 Agent 消息只来自 2 个 Agent，按 lap floor 确定性跳过
    // （collaboration.md §8.3，Cumora `pastFloor`），不再调用 triage 模型。
    let triage = fixture.triage(&alpha_token, &alpha_agent_run.id).await;
    assert_eq!(
        triage.verdict.map(|verdict| verdict.source),
        Some("lap_floor".to_string())
    );

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

    // 真正走到 triage 模型的 Agent 消息（群里第一条 Agent 回复）带着私有 Climate。
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
         VALUES ($1, $2, 1, $3, 'normal', 'Beta shares a plan.')",
    )
    .bind(format!("msg-{}", Uuid::new_v4().simple()))
    .bind(&group.id)
    .bind(&beta.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE collab_rooms SET next_seq = 1 WHERE id = $1")
        .bind(&group.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let group_inbox = fixture.inbox(&alpha_token).await;
    let group_run = fixture.open_run(&alpha_token, &group_inbox).await;
    let triage = fixture.triage(&alpha_token, &group_run.id).await;
    assert!(triage.verdict.is_none());
    let input = triage.input.expect("Agent-only traffic uses local triage");
    assert!(input.contains("Private Climate context"));
    assert!(input.contains("Strong technically; verify estimates."));
    fixture
        .finish(&alpha_token, &group_run.id, "completed")
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
