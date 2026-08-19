use std::str::FromStr;

use openwork_collab::{
    coordination::CoordinationHub,
    model::{
        AgentInput, AgentReplyOutcome, MessagePageAnchor, MessagePageQuery, TriageRecordInput,
    },
    storage::CollabStorage,
};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

#[tokio::test]
async fn p1_migration_and_message_invariants_work_from_an_empty_schema() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_test_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(options)
        .await
        .unwrap();
    let storage = CollabStorage::from_pool(pool.clone());

    storage.migrate().await.unwrap();
    storage.migrate().await.unwrap();
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM collab_schema_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(applied, 3);
    let collab_feature_tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables
          WHERE table_schema = current_schema()
            AND table_name IN (
                'collab_triages', 'collab_reactions',
                'collab_boards', 'collab_board_columns', 'collab_cards'
            )",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(collab_feature_tables, 5);

    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Answer clearly".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "hy3-free".to_string(),
            enabled: true,
        })
        .await
        .unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    storage.add_member("general", "alice").await.unwrap();

    let first = storage
        .send_message("general", "user", "hello")
        .await
        .unwrap();
    let duplicate = storage
        .send_message("general", "user", "hello")
        .await
        .unwrap();
    assert!(!first.deduplicated);
    assert!(duplicate.deduplicated);
    assert_eq!(duplicate.message.id, first.message.id);
    assert_eq!(
        storage
            .room("general")
            .await
            .unwrap()
            .unwrap()
            .next_sequence,
        1
    );

    let mut tasks = Vec::new();
    for number in 0..8 {
        let storage = storage.clone();
        tasks.push(tokio::spawn(async move {
            storage
                .send_message("general", "user", &format!("message-{number}"))
                .await
                .unwrap()
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let messages = storage.room_messages("general").await.unwrap();
    assert_eq!(messages.len(), 9);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        (1..=9).collect::<Vec<_>>()
    );

    let first_inbox = storage.inbox("alice").await.unwrap();
    let second_inbox = storage.inbox("alice").await.unwrap();
    assert_eq!(first_inbox.unread_count, 9);
    assert_eq!(second_inbox, first_inbox);
    let coordination = CoordinationHub::default();
    coordination.observe("alice", "general", 9).await;
    let after_seen = storage.inbox("alice").await.unwrap();
    assert_eq!(after_seen, first_inbox);
    assert!(
        first_inbox
            .messages
            .iter()
            .all(|message| message.created_at.ends_with("+08:00"))
    );

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_group_replies_hold_the_stale_writer_and_retry_without_duplicates() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_test_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    for id in ["alice", "bob"] {
        storage
            .create_agent(&AgentInput {
                id: id.to_string(),
                display_name: id.to_string(),
                role: None,
                bio: None,
                system_prompt: "Answer clearly".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "model".to_string(),
                enabled: true,
            })
            .await
            .unwrap();
    }
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for id in ["user", "alice", "bob"] {
        storage.add_member("general", id).await.unwrap();
    }
    storage
        .send_message("general", "user", "question")
        .await
        .unwrap();

    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for (id, body) in [("alice", "alice answer"), ("bob", "bob answer")] {
        let storage = storage.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            (
                id,
                storage
                    .send_agent_reply("general", id, body, Some(1))
                    .await
                    .unwrap(),
            )
        }));
    }
    let first = tasks.remove(0).await.unwrap();
    let second = tasks.remove(0).await.unwrap();
    let outcomes = [first, second];
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, outcome)| matches!(outcome, AgentReplyOutcome::Published(_)))
            .count(),
        1
    );
    let (held_agent_id, held) = outcomes
        .into_iter()
        .find_map(|(agent_id, outcome)| match outcome {
            AgentReplyOutcome::Held(held) => Some((agent_id, held)),
            AgentReplyOutcome::Published(_) => None,
        })
        .unwrap();
    assert_eq!(held.peer_sequence, 2);
    assert_eq!(held.messages.len(), 1);

    let retry = storage
        .send_agent_reply(
            "general",
            held_agent_id,
            "recomputed answer",
            Some(held.peer_sequence),
        )
        .await
        .unwrap();
    assert!(matches!(retry, AgentReplyOutcome::Published(_)));
    let messages = storage.room_messages("general").await.unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.sequence == 2)
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.sequence == 3)
            .count(),
        1
    );

    storage.create_group_room("duo", "Duo").await.unwrap();
    storage.add_member("duo", "user").await.unwrap();
    storage.add_member("duo", "alice").await.unwrap();
    storage.send_message("duo", "user", "hello").await.unwrap();
    let direct_reply = storage
        .send_agent_reply("duo", "alice", "hello back", Some(0))
        .await
        .unwrap();
    assert!(matches!(direct_reply, AgentReplyOutcome::Published(_)));

    let second_message = messages
        .iter()
        .find(|message| message.sequence == 2)
        .unwrap();
    let reaction = storage
        .add_reaction(&second_message.id, "alice", "👍")
        .await
        .unwrap();
    assert_eq!(reaction.message_id, second_message.id);
    assert!(reaction.created_at.ends_with("+08:00"));

    storage
        .record_triage(TriageRecordInput {
            agent_id: "alice",
            room_id: "general",
            up_to_sequence: 3,
            actionable: false,
            response_mode: Some("one_of_us"),
            source: "support_model",
            reason: Some("not relevant"),
            prompt_note: Some("stay quiet"),
            provider_id: Some("cheap"),
            model_id: Some("support"),
            input_tokens: Some(12),
            output_tokens: Some(5),
            latency_ms: 42,
        })
        .await
        .unwrap();
    let triages = storage.triage_records(Some("general")).await.unwrap();
    assert_eq!(triages.len(), 1);
    assert!(!triages[0].actionable);
    assert_eq!(triages[0].source, "support_model");
    assert!(triages[0].created_at.ends_with("+08:00"));

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn ten_thousand_message_room_opens_a_bounded_page_around_the_read_cursor() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_test_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    sqlx::query(
        "INSERT INTO collab_messages (
            id, room_id, sequence, author_id, kind, body, created_at
         )
         SELECT 'msg-' || value, 'general', value, 'user', 'normal',
                'message-' || value,
                (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                    + (value * INTERVAL '1 millisecond')
           FROM generate_series(1, 10000) AS value",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE collab_rooms SET next_seq = 10000 WHERE id = 'general'")
        .execute(&pool)
        .await
        .unwrap();
    storage.mark_user_read("general", 5_000).await.unwrap();

    let page = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        storage.message_page(
            "general",
            MessagePageQuery {
                anchor: MessagePageAnchor::Around(5_000),
                limit: 50,
            },
        ),
    )
    .await
    .expect("a bounded page in a 10,000-message room must not scan into a visible delay")
    .unwrap();

    assert_eq!(page.messages.len(), 50);
    assert_eq!(page.messages.first().unwrap().sequence, 4_975);
    assert_eq!(page.messages.last().unwrap().sequence, 5_024);
    assert!(page.has_older);
    assert!(page.has_newer);

    let unread = storage.room_summaries("user").await.unwrap().remove(0);
    assert_eq!(unread.last_read_sequence, 5_000);
    assert_eq!(unread.unread_count, 5_000);

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn room_summary_keeps_unread_separate_and_read_cursor_is_monotonic() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_test_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    for number in 1..=3 {
        storage
            .send_message("general", "user", &format!("message-{number}"))
            .await
            .unwrap();
    }

    let before = storage.room_summaries("user").await.unwrap();
    assert_eq!(before[0].unread_count, 3);
    assert_eq!(before[0].last_read_sequence, 0);

    assert_eq!(storage.mark_user_read("general", 2).await.unwrap(), 2);
    assert_eq!(storage.mark_user_read("general", 1).await.unwrap(), 2);
    assert_eq!(storage.mark_user_read("general", 99).await.unwrap(), 3);
    let after = storage.room_summaries("user").await.unwrap();
    assert_eq!(after[0].unread_count, 0);
    assert_eq!(after[0].last_read_sequence, 3);

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn disabling_an_agent_stops_triage_candidacy_without_erasing_room_identity() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_test_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    let enabled = AgentInput {
        id: "alice".to_string(),
        display_name: "Alice".to_string(),
        role: None,
        bio: None,
        system_prompt: "Answer clearly".to_string(),
        provider_id: "opencode".to_string(),
        model_id: "hy3-free".to_string(),
        enabled: true,
    };
    storage.create_agent(&enabled).await.unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    storage.add_member("general", "alice").await.unwrap();
    storage
        .send_message("general", "alice", "historical answer")
        .await
        .unwrap();

    let mentioned = storage.candidate_agents("general", "user").await.unwrap();
    assert_eq!(
        mentioned
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alice"]
    );
    storage
        .update_agent(&AgentInput {
            enabled: false,
            ..enabled
        })
        .await
        .unwrap();

    assert!(
        storage
            .candidate_agents("general", "user")
            .await
            .unwrap()
            .is_empty()
    );
    let room = storage.room_summaries("user").await.unwrap().remove(0);
    let alice = room
        .members
        .iter()
        .find(|member| member.id == "alice")
        .unwrap();
    assert!(!alice.enabled);
    let history = storage.room_messages("general").await.unwrap();
    assert_eq!(history[0].author_id, "alice");
    assert_eq!(history[0].body, "historical answer");

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}
