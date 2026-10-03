#![cfg(unix)]
//! 发布前的检查（collaboration.md §9、§7.3、§8.3）：连发、HELD、逐字重复、列出消息后的 seen 与
//! triage 模型失败时的结算。

use openwork_collab::protocol::{
    AgentCommand, AgentCommandResult, MessageView, TriageReportRequest, TriageVerdict, request_id,
};
use uuid::Uuid;

#[path = "support/room_fixture.rs"]
pub mod room_fixture;

use room_fixture::Fixture;

fn post(room_id: &str, body: &str, continuation: bool) -> AgentCommand {
    AgentCommand::Reply {
        room_id: room_id.to_string(),
        body: body.to_string(),
        held_token: None,
        quoted_message_id: None,
        continuation,
    }
}

fn reply(room_id: &str, body: &str) -> AgentCommand {
    post(room_id, body, false)
}

/// `reply --continue`。
fn continued(room_id: &str, body: &str) -> AgentCommand {
    post(room_id, body, true)
}

fn bodies(messages: &[MessageView]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.body.as_str())
        .collect()
}

async fn message_count(fixture: &Fixture, room_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM collab_messages WHERE room_id = $1")
        .bind(room_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

async fn delivery_reason(fixture: &Fixture, run_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT eligible_reason FROM collab_run_deliveries WHERE run_id = $1")
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

/// 连发拒绝的文本里只有秒数会变；取出秒数后逐字比较其余部分。
fn assert_monologue(result: &AgentCommandResult, room_id: &str) {
    let AgentCommandResult::Error { code, message } = result else {
        panic!("expected a MONOLOGUE rejection, got {result:?}")
    };
    assert_eq!(code, "MONOLOGUE");
    let prefix = format!("you already posted in {room_id} ");
    let seconds = message
        .strip_prefix(&prefix)
        .and_then(|rest| rest.split_once("s ago"))
        .map(|(seconds, _)| seconds)
        .expect("rejection names the age");
    assert!(
        seconds
            .parse::<u32>()
            .is_ok_and(|seconds| (1..600).contains(&seconds))
    );
    assert_eq!(
        message,
        &format!(
            "you already posted in {room_id} {seconds}s ago and nobody has replied yet — you can't post again until someone else speaks. If you have more to say, fold it into your next message when someone responds. Right now: stay silent and let someone else move the thread. Override only if it's truly urgent: rerun with --continue."
        )
    );
}

/// collaboration.md §9.2、§15 #11：两个 Agent 同时在群里发同一句（`--continue` 跳过 HELD，只剩逐字重复
/// 这道检查），锁住房间行后比较，只有先提交的一条发出。
#[tokio::test]
async fn acc_11_concurrent_identical_group_posts_publish_only_once() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    fixture.send_user(&group.id, "Agree or not?").await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let alpha_inbox = fixture.inbox(&alpha_token).await;
    let beta_inbox = fixture.inbox(&beta_token).await;
    fixture.open_run(&alpha_token, &alpha_inbox).await;
    fixture.open_run(&beta_token, &beta_inbox).await;

    let (from_alpha, from_beta) = tokio::join!(
        fixture.command(&alpha_token, continued(&group.id, "Agreed.")),
        fixture.command(&beta_token, continued(&group.id, "Agreed.")),
    );
    let outcomes = [&from_alpha.result, &from_beta.result];
    let published = outcomes
        .iter()
        .filter(|result| matches!(result, AgentCommandResult::MessagePublished { .. }))
        .count();
    let duplicates = outcomes
        .iter()
        .filter(|result| matches!(result, AgentCommandResult::Error { code, .. } if code == "DUPLICATE"))
        .count();
    assert_eq!((published, duplicates), (1, 1));
    assert_eq!(message_count(&fixture, &group.id).await, 2);

    fixture.stop().await;
}

/// collaboration.md §9.4、§15 #20：自己的上一条是房间最后一条且不到 10 分钟时拒绝；同一 Run 的第 2 条
/// 放行、第 3 条拒绝；`--continue` 放行；新 Run 里第一条就连发也拒绝，且 delivery 不推进；私聊不检查。
#[tokio::test]
async fn acc_20_an_agent_cannot_post_twice_in_a_row_until_someone_else_speaks() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    fixture.send_user(&group.id, "Plan the launch.").await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let alpha_inbox = fixture.inbox(&alpha_token).await;
    let beta_inbox = fixture.inbox(&beta_token).await;
    fixture.open_run(&alpha_token, &alpha_inbox).await;
    let beta_run = fixture.open_run(&beta_token, &beta_inbox).await;

    for body in ["Drafting the plan now.", "Plan: ship on Friday."] {
        let posted = fixture.command(&alpha_token, reply(&group.id, body)).await;
        assert!(matches!(
            posted.result,
            AgentCommandResult::MessagePublished { .. }
        ));
    }
    let third = fixture
        .command(&alpha_token, reply(&group.id, "Also: freeze on Thursday."))
        .await;
    assert_monologue(&third.result, &group.id);
    assert_eq!(message_count(&fixture, &group.id).await, 3);
    let forced = fixture
        .command(
            &alpha_token,
            continued(&group.id, "Also: freeze on Thursday."),
        )
        .await;
    assert!(matches!(
        forced.result,
        AgentCommandResult::MessagePublished { .. }
    ));

    let held = fixture
        .command(&beta_token, reply(&group.id, "Friday works."))
        .await;
    assert!(matches!(held.result, AgentCommandResult::Held { .. }));
    let answered = fixture
        .command(&beta_token, reply(&group.id, "Friday works."))
        .await;
    assert!(matches!(
        answered.result,
        AgentCommandResult::MessagePublished { .. }
    ));
    fixture.finish(&beta_token, &beta_run.id, "completed").await;

    let next_inbox = fixture.inbox(&beta_token).await;
    let next_run = fixture.open_run(&beta_token, &next_inbox).await;
    let again = fixture
        .command(&beta_token, reply(&group.id, "Friday, confirmed."))
        .await;
    assert_monologue(&again.result, &group.id);
    assert_eq!(delivery_reason(&fixture, &next_run.id).await, None);

    let dm = |body: &str| AgentCommand::DirectMessage {
        participant_id: beta.id.clone(),
        body: body.to_string(),
    };
    for body in ["Ping.", "Ping again."] {
        let sent = fixture.command(&alpha_token, dm(body)).await;
        assert!(matches!(
            sent.result,
            AgentCommandResult::DirectMessageSent { .. }
        ));
    }

    fixture.stop().await;
}

/// collaboration.md §7.3、§9.1、§15 #21：HELD 一次最多列 8 条没看过的消息，重发时再列剩下的；
/// `messages` 列出过的消息算作看过，之后的 `reply` 不再因为它们被 HELD。
#[tokio::test]
async fn acc_21_held_lists_eight_messages_and_listing_counts_as_seen() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    fixture.send_user(&group.id, "Count to nine, Alpha.").await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let alpha_inbox = fixture.inbox(&alpha_token).await;
    let beta_inbox = fixture.inbox(&beta_token).await;
    fixture.open_run(&alpha_token, &alpha_inbox).await;
    fixture.open_run(&beta_token, &beta_inbox).await;
    let numbers = (1..=9).map(|number| number.to_string()).collect::<Vec<_>>();
    for number in &numbers {
        let posted = fixture
            .command(&alpha_token, continued(&group.id, number))
            .await;
        assert!(matches!(
            posted.result,
            AgentCommandResult::MessagePublished { .. }
        ));
    }

    let first = fixture
        .command(&beta_token, reply(&group.id, "Done?"))
        .await;
    let AgentCommandResult::Held { messages, .. } = &first.result else {
        panic!("Beta must be held: {:?}", first.result)
    };
    assert_eq!(bodies(messages), ["1", "2", "3", "4", "5", "6", "7", "8"]);
    let second = fixture
        .command(&beta_token, reply(&group.id, "Done?"))
        .await;
    let AgentCommandResult::Held { messages, .. } = &second.result else {
        panic!("Beta must be held on the ninth: {:?}", second.result)
    };
    assert_eq!(bodies(messages), ["9"]);
    let third = fixture
        .command(&beta_token, reply(&group.id, "Done?"))
        .await;
    assert!(matches!(
        third.result,
        AgentCommandResult::MessagePublished { .. }
    ));

    let held = fixture
        .command(&alpha_token, reply(&group.id, "Yes, done."))
        .await;
    assert!(matches!(held.result, AgentCommandResult::Held { .. }));
    let posted = fixture
        .command(&alpha_token, reply(&group.id, "Yes, done."))
        .await;
    assert!(matches!(
        posted.result,
        AgentCommandResult::MessagePublished { .. }
    ));
    let listed = fixture
        .command(
            &beta_token,
            AgentCommand::Messages {
                room_id: group.id.clone(),
                tail: 5,
            },
        )
        .await;
    assert!(matches!(listed.result, AgentCommandResult::Messages { .. }));
    let after_listing = fixture
        .command(&beta_token, reply(&group.id, "Great."))
        .await;
    assert!(
        matches!(
            after_listing.result,
            AgentCommandResult::MessagePublished { .. }
        ),
        "a message already listed must not hold the reply: {:?}",
        after_listing.result
    );

    fixture.stop().await;
}

/// collaboration.md §8.3、§15 #22：只含 Agent 消息的批次走到 triage 模型；模型失败时 fail closed，
/// 结论记为 `fail_closed`、delivery 以 `triage_false` 结算，之后不会再为这条消息唤醒；
/// `fail_closed` 不能是 actionable。
#[tokio::test]
async fn acc_22_a_failed_triage_model_fails_closed_for_agent_only_messages() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    sqlx::query(
        "WITH next AS (
             UPDATE collab_rooms SET next_seq = next_seq + 1 WHERE id = $1 RETURNING next_seq
         )
         INSERT INTO collab_messages (id, room_id, sequence, author_id, body)
         SELECT $2, $1, next_seq, $3, 'What does everyone think?' FROM next",
    )
    .bind(&group.id)
    .bind(format!("msg-{}", Uuid::new_v4().simple()))
    .bind(&alpha.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let beta_token = fixture.token(&beta.id).await;
    let inbox = fixture.inbox(&beta_token).await;
    let run = fixture.open_run(&beta_token, &inbox).await;
    let payload = fixture.triage(&beta_token, &run.id).await;
    assert!(payload.verdict.is_none() && payload.instructions.is_some());

    let report = |actionable: bool| TriageReportRequest {
        run_id: run.id.clone(),
        verdict: TriageVerdict {
            actionable,
            reason: "local triage failed (unparseable output); fail closed".to_string(),
            prompt_note: String::new(),
            source: "fail_closed".to_string(),
        },
        model: "local/triage".to_string(),
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(10),
        response_mode: None,
    };
    let refused = fixture
        .http
        .post(format!("{}/agent/triage", fixture.base_url))
        .bearer_auth(&beta_token)
        .json(&report(true))
        .send()
        .await
        .unwrap();
    assert!(!refused.status().is_success());
    fixture.report_triage(&beta_token, &report(false)).await;
    fixture.finish(&beta_token, &run.id, "completed").await;

    let source: String = sqlx::query_scalar("SELECT source FROM collab_triages WHERE run_id = $1")
        .bind(&run.id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(source, "fail_closed");
    assert_eq!(
        delivery_reason(&fixture, &run.id).await.as_deref(),
        Some("triage_false")
    );
    assert!(fixture.inbox(&beta_token).await.trigger.is_none());

    fixture.stop().await;
}

/// collaboration.md §9.2、§15 #11：成员超过 2 人的房间里，与上一条别人的消息逐字相同（去掉首尾
/// 空白）时拒绝，带 HELD token 或 `--continue` 重试也拒绝，被拒的消息不写入、delivery 不推进；
/// 私聊不拦：两个 Agent 同时私聊对方同一句话都能发出（Cumora 以 `member_count > 2` 为条件）。
#[tokio::test]
async fn acc_11_a_verbatim_repeat_of_the_last_peer_message_is_rejected() {
    let Some(fixture) = Fixture::start().await else {
        return;
    };
    let alpha = fixture.create_agent("Alpha").await;
    let beta = fixture.create_agent("Beta").await;
    let group = fixture
        .create_group(vec![alpha.id.clone(), beta.id.clone()])
        .await;
    fixture
        .send_user(&group.id, "Count to 5, one number each.")
        .await;
    let alpha_token = fixture.token(&alpha.id).await;
    let beta_token = fixture.token(&beta.id).await;
    let alpha_inbox = fixture.inbox(&alpha_token).await;
    let beta_inbox = fixture.inbox(&beta_token).await;
    fixture.open_run(&alpha_token, &alpha_inbox).await;
    let beta_run = fixture.open_run(&beta_token, &beta_inbox).await;
    let reply = |body: &str, held_token: Option<String>| AgentCommand::Reply {
        room_id: group.id.clone(),
        body: body.to_string(),
        held_token,
        quoted_message_id: None,
        continuation: false,
    };
    let duplicate_text = format!(
        "your message is identical to the latest message from Alpha in {}: \"1\". Alpha already said it; pick a different angle, the next item in a sequence, or stay silent.",
        group.id
    );

    let first = fixture.command(&alpha_token, reply("1", None)).await;
    assert!(matches!(
        first.result,
        AgentCommandResult::MessagePublished { .. }
    ));
    let held = fixture.command(&beta_token, reply("1", None)).await;
    let AgentCommandResult::Held { retry_token, .. } = held.result else {
        panic!("Beta must be held first: {:?}", held.result)
    };
    let duplicate = fixture
        .command(&beta_token, reply(" 1\n", Some(retry_token)))
        .await;
    let rejected = AgentCommandResult::Error {
        code: "DUPLICATE".to_string(),
        message: duplicate_text,
    };
    assert_eq!(duplicate.result, rejected);
    let continued = fixture
        .command(
            &beta_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "1".to_string(),
                held_token: None,
                quoted_message_id: None,
                continuation: true,
            },
        )
        .await;
    assert_eq!(continued.result, rejected);
    let (messages, eligible): (i64, Option<String>) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM collab_messages WHERE room_id = $1),
                (SELECT eligible_reason FROM collab_run_deliveries WHERE run_id = $2)",
    )
    .bind(&group.id)
    .bind(&beta_run.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!((messages, eligible), (2, None));
    let next = fixture.command(&beta_token, reply("2", None)).await;
    assert!(matches!(
        next.result,
        AgentCommandResult::MessagePublished { .. }
    ));

    let dm = |participant_id: &str| AgentCommand::DirectMessage {
        participant_id: participant_id.to_string(),
        body: "Agreed.".to_string(),
    };
    let (from_alpha, from_beta) = tokio::join!(
        fixture.command(&alpha_token, dm(&beta.id)),
        fixture.command(&beta_token, dm(&alpha.id)),
    );
    for outcome in [&from_alpha.result, &from_beta.result] {
        assert!(
            matches!(outcome, AgentCommandResult::DirectMessageSent { .. }),
            "a Direct Room repeat must publish: {outcome:?}"
        );
    }

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
                quoted_message_id: None,
                continuation: false,
            },
        ),
        fixture.command(
            &beta_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "Beta answer".to_string(),
                held_token: None,
                quoted_message_id: None,
                continuation: false,
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
    let retry_request_id = request_id();
    let retry_command = AgentCommand::Reply {
        room_id: group.id.clone(),
        body: "Reconsidered answer".to_string(),
        held_token: Some(held_token),
        quoted_message_id: None,
        continuation: false,
    };
    let retried = fixture
        .command_with_request_id(
            held_agent_token,
            retry_request_id.clone(),
            retry_command.clone(),
        )
        .await;
    let AgentCommandResult::MessagePublished {
        message: retried_message,
    } = &retried.result
    else {
        panic!("HELD retry was not published: {:?}", retried.result)
    };
    let replayed = fixture
        .command_with_request_id(held_agent_token, retry_request_id, retry_command)
        .await;
    let AgentCommandResult::MessagePublished {
        message: replayed_message,
    } = replayed.result
    else {
        panic!("idempotent HELD retry was not replayed")
    };
    assert_eq!(replayed_message.id, retried_message.id);
    let reused = fixture
        .command(
            held_agent_token,
            AgentCommand::Reply {
                room_id: group.id.clone(),
                body: "A forbidden second reconsideration".to_string(),
                held_token: Some(consumed_held_token),
                quoted_message_id: None,
                continuation: false,
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
