use std::str::FromStr;

use openwork_collab::{
    coordination::CoordinationHub,
    model::{
        AgentInput, AgentReplyOutcome, MessagePageAnchor, MessagePageQuery, TriageRecordInput,
    },
    storage::{CollabStorage, INBOX_MESSAGE_LIMIT},
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
    assert_eq!(applied, 6);
    let collab_feature_tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables
          WHERE table_schema = current_schema()
            AND table_name IN (
                'collab_triages', 'collab_reactions',
                'collab_boards', 'collab_board_columns', 'collab_cards',
                'collab_events'
            )",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(collab_feature_tables, 6);

    storage
        .create_agent(&AgentInput {
            id: Some("alice".to_string()),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Answer clearly".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "hy3-free".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room(Some("general"), "General")
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
        3
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
    assert_eq!(messages.len(), 11);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        (1..=11).collect::<Vec<_>>()
    );

    let first_inbox = storage.inbox("alice").await.unwrap();
    let second_inbox = storage.inbox("alice").await.unwrap();
    assert_eq!(first_inbox.unread_count, 10);
    assert_eq!(second_inbox, first_inbox);
    let coordination = CoordinationHub::default();
    coordination.observe("alice", "general", 11).await;
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
                id: Some(id.to_string()),
                display_name: id.to_string(),
                role: None,
                bio: None,
                system_prompt: "Answer clearly".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "model".to_string(),
                enabled: true,
                scanner_enabled: false,
            })
            .await
            .unwrap();
    }
    storage
        .create_group_room(Some("general"), "General")
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
                    .send_agent_reply("general", id, body, Some(4))
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
    assert_eq!(held.peer_sequence, 5);
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
    assert_eq!(messages.len(), 6);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.sequence == 5)
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.sequence == 6)
            .count(),
        1
    );

    storage.create_group_room(Some("duo"), "Duo").await.unwrap();
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
        .find(|message| message.sequence == 5)
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
            up_to_sequence: 6,
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
        .create_group_room(Some("general"), "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    sqlx::query("DELETE FROM collab_messages WHERE room_id = 'general'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE collab_rooms SET next_seq = 0, last_message_at = NULL WHERE id = 'general'",
    )
    .execute(&pool)
    .await
    .unwrap();
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
    storage.mark_read("general", "user", 5_000).await.unwrap();

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
        .create_group_room(Some("general"), "General")
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
    assert_eq!(before[0].unread_count, 4);
    assert_eq!(before[0].last_read_sequence, 0);

    assert_eq!(storage.mark_read("general", "user", 2).await.unwrap(), 2);
    assert_eq!(storage.mark_read("general", "user", 1).await.unwrap(), 2);
    assert_eq!(storage.mark_read("general", "user", 99).await.unwrap(), 4);
    let after = storage.room_summaries("user").await.unwrap();
    assert_eq!(after[0].unread_count, 0);
    assert_eq!(after[0].last_read_sequence, 4);

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
        id: Some("alice".to_string()),
        display_name: "Alice".to_string(),
        role: None,
        bio: None,
        system_prompt: "Answer clearly".to_string(),
        provider_id: "opencode".to_string(),
        model_id: "hy3-free".to_string(),
        enabled: true,
        scanner_enabled: false,
    };
    storage.create_agent(&enabled).await.unwrap();
    storage
        .create_group_room(Some("general"), "General")
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
            scanner_enabled: false,
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
    let historical = history
        .iter()
        .find(|message| message.body == "historical answer")
        .unwrap();
    assert_eq!(historical.author_id, "alice");

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn create_agent_derives_ids_from_names_without_touching_explicit_ones() {
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
    let pool = PgPoolOptions::new().connect_with(options).await.unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();

    let derived_input = |display_name: &str| AgentInput {
        id: None,
        display_name: display_name.to_string(),
        role: None,
        bio: None,
        system_prompt: "Answer clearly".to_string(),
        provider_id: "opencode".to_string(),
        model_id: "model".to_string(),
        enabled: true,
        scanner_enabled: false,
    };

    let first = storage.create_agent(&derived_input("Alice")).await.unwrap();
    assert_eq!(first.id, "alice");

    let second = storage.create_agent(&derived_input("Alice")).await.unwrap();
    assert_ne!(second.id, "alice");
    assert!(second.id.starts_with("alice_"));

    let underivable = storage.create_agent(&derived_input("小艾")).await;
    assert!(underivable.is_err());

    let manual = storage
        .create_agent(&AgentInput {
            id: Some("iris".to_string()),
            display_name: "小艾".to_string(),
            role: None,
            bio: None,
            system_prompt: "Answer clearly".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    assert_eq!(manual.id, "iris");

    let invalid = storage
        .create_agent(&AgentInput {
            id: Some("Iris".to_string()),
            ..derived_input("小艾")
        })
        .await;
    assert!(invalid.is_err());

    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn group_rooms_get_uuid_ids_unless_an_explicit_one_is_provided() {
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
    let pool = PgPoolOptions::new().connect_with(options).await.unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();

    let first = storage.create_group_room(None, "Lounge").await.unwrap();
    let second = storage.create_group_room(None, "Lounge").await.unwrap();
    assert!(first.id.starts_with("room_"));
    assert!(second.id.starts_with("room_"));
    assert_ne!(first.id, second.id);

    let explicit = storage
        .create_group_room(Some("general"), "General")
        .await
        .unwrap();
    assert_eq!(explicit.id, "general");

    let blank = storage.create_group_room(None, "  ").await;
    assert!(blank.is_err());

    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn fixes_bound_agent_delivery_and_make_room_topology_writable() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_fixes_test_{}", Uuid::new_v4().simple());
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
    let alice = storage
        .create_agent(&AgentInput {
            id: Some("alice".to_string()),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: String::new(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room(Some("general"), "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    for number in 1..=3 {
        storage
            .send_message("general", "user", &format!("history-{number}"))
            .await
            .unwrap();
    }

    let joined = storage
        .add_member("general", "alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(joined.kind, "system");
    assert_eq!(
        joined.system_payload.as_ref().unwrap()["type"],
        "member_joined"
    );
    let first_inbox = storage.inbox("alice").await.unwrap();
    assert_eq!(first_inbox.messages, vec![joined.clone()]);
    assert_eq!(first_inbox.omitted_count, 0);
    storage
        .mark_read("general", "alice", joined.sequence)
        .await
        .unwrap();

    for number in 0..105 {
        storage
            .send_message("general", "user", &format!("unread-{number}"))
            .await
            .unwrap();
    }
    let bounded = storage.inbox("alice").await.unwrap();
    assert_eq!(bounded.unread_count, 105);
    assert_eq!(bounded.messages.len(), 100);
    assert_eq!(bounded.omitted_count, 5);
    assert_eq!(
        bounded.omission_notice.as_deref(),
        Some(
            "5 newer unread message(s) are not shown here; \
             they stay unread and arrive in a later turn."
        )
    );
    // The bounded slice is the OLDEST unread, so the omitted five are the
    // NEWEST and stay above the cursor the caller will advance to.
    assert_eq!(bounded.messages.first().unwrap().body, "unread-0");
    assert_eq!(bounded.messages.last().unwrap().body, "unread-99");

    storage
        .set_member_muted("general", "alice", true)
        .await
        .unwrap();
    assert!(
        storage
            .candidate_agents("general", "user")
            .await
            .unwrap()
            .is_empty()
    );
    let summary = storage.room_summaries("user").await.unwrap().remove(0);
    assert!(
        summary
            .members
            .iter()
            .find(|member| member.id == "alice")
            .unwrap()
            .muted
    );
    storage
        .set_member_muted("general", "alice", false)
        .await
        .unwrap();

    let left = storage.remove_member("general", "alice").await.unwrap();
    assert_eq!(left.kind, "system");
    assert_eq!(left.system_payload.as_ref().unwrap()["type"], "member_left");
    assert!(!storage.is_member("general", "alice").await.unwrap());

    let run_id = storage
        .begin_run(&alice, Some("general"), "message")
        .await
        .unwrap();
    storage
        .finish_run(&run_id, "completed", Some("silent"), None)
        .await
        .unwrap();
    let outcome: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM collab_runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(outcome.as_deref(), Some("silent"));

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

/// The read cursor advances to the highest sequence delivered in a turn, so the
/// delivered slice must be each room's OLDEST unread. Delivering the newest
/// instead would drop everything the limit omitted below the cursor, where
/// `inbox` can never return it again — a silent, permanent loss that triggers on
/// the very first wake after a backlog builds up.
#[tokio::test]
async fn a_backlog_larger_than_the_inbox_limit_drains_across_turns_without_losing_a_message() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_backlog_test_{}", Uuid::new_v4().simple());
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
        .create_agent(&AgentInput {
            id: Some("alice".to_string()),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: String::new(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room(Some("general"), "General")
        .await
        .unwrap();
    storage.add_member("general", "user").await.unwrap();
    storage.add_member("general", "alice").await.unwrap();

    let limit = usize::try_from(INBOX_MESSAGE_LIMIT).unwrap();
    let backlog = limit + 26;
    for number in 1..=backlog {
        storage
            .send_message("general", "user", &format!("backlog-{number}"))
            .await
            .unwrap();
    }

    let first = storage.inbox("alice").await.unwrap();
    assert_eq!(first.messages.len(), limit);
    assert!(first.omitted_count > 0);
    assert!(first.omission_notice.is_some());
    let first_sequences: Vec<i64> = first.messages.iter().map(|m| m.sequence).collect();
    let outstanding = u64::try_from(first_sequences.len()).unwrap() + first.omitted_count;
    assert_eq!(outstanding, first.unread_count);

    // The delivered slice is the LOWEST unread sequences, contiguously — that is
    // what makes the maximum a safe cursor target.
    let lowest = *first_sequences.first().unwrap();
    let highest = *first_sequences.last().unwrap();
    assert_eq!(
        first_sequences,
        (lowest..=highest).collect::<Vec<_>>(),
        "delivered slice must be a contiguous prefix, not a newest-first sample"
    );

    storage
        .mark_read("general", "alice", highest)
        .await
        .unwrap();

    let second = storage.inbox("alice").await.unwrap();
    assert_eq!(second.omitted_count, 0);
    assert_eq!(second.omission_notice, None);
    let second_sequences: Vec<i64> = second.messages.iter().map(|m| m.sequence).collect();
    assert_eq!(*second_sequences.first().unwrap(), highest + 1);

    // Nothing fell between the two turns: the union is the whole backlog.
    let mut delivered = first_sequences;
    delivered.extend(second_sequences);
    assert_eq!(
        delivered,
        (lowest..=lowest + i64::try_from(outstanding).unwrap() - 1).collect::<Vec<_>>(),
        "a message was skipped between turns"
    );

    storage
        .mark_read("general", "alice", *delivered.last().unwrap())
        .await
        .unwrap();
    let drained = storage.inbox("alice").await.unwrap();
    assert!(drained.messages.is_empty());
    assert_eq!(drained.unread_count, 0);

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
    admin.close().await;
}
