use std::str::FromStr;

use openwork_collab::{
    autonomy::AutonomyEngine,
    model::{AgentInput, CardInput},
    proactivity::{NudgeClaim, ProactivityHub},
    storage::CollabStorage,
};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

#[tokio::test]
async fn agenda_reads_only_explicitly_incomplete_cards_for_the_agent() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Use the shared board".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    storage
        .create_board("work", "general", "Work")
        .await
        .unwrap();
    storage
        .create_board_column("todo", "work", "Done-ish words do not matter", 0, false)
        .await
        .unwrap();
    storage
        .create_board_column("finished", "work", "Backlog", 1, true)
        .await
        .unwrap();
    for (column_id, title) in [
        ("todo", "Still actionable"),
        ("finished", "Already complete"),
    ] {
        storage
            .create_card(
                CardInput {
                    board_id: "work".to_string(),
                    column_id: column_id.to_string(),
                    title: title.to_string(),
                    description: None,
                    position: 0,
                    assignee_id: Some("alice".to_string()),
                },
                "user",
            )
            .await
            .unwrap();
    }

    let candidates = storage.agenda_candidates("alice").await.unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].room_id, "general");
    assert_eq!(candidates[0].cards.len(), 1);
    assert_eq!(candidates[0].cards[0].title, "Still actionable");

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn scanner_candidates_are_explicitly_opted_in() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    for (id, scanner_enabled) in [("alice", false), ("bob", true)] {
        storage
            .create_agent(&AgentInput {
                id: id.to_string(),
                display_name: id.to_string(),
                role: None,
                bio: None,
                system_prompt: "Observe shared work".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "model".to_string(),
                enabled: true,
                scanner_enabled,
            })
            .await
            .unwrap();
    }

    let agents = storage.scanner_agents().await.unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "bob");

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_stall_sweeps_have_one_in_memory_pusher_for_a_real_room() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    for id in ["alice", "bob"] {
        storage
            .create_agent(&AgentInput {
                id: id.to_string(),
                display_name: id.to_string(),
                role: None,
                bio: None,
                system_prompt: "Advance stalled work".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "model".to_string(),
                enabled: true,
                scanner_enabled: false,
            })
            .await
            .unwrap();
    }
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice", "bob"] {
        storage.add_member("general", participant).await.unwrap();
    }
    storage
        .send_message("general", "user", "This work is still in progress")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE collab_rooms
            SET last_message_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                - INTERVAL '10 minutes'
          WHERE id = 'general'",
    )
    .execute(&pool)
    .await
    .unwrap();
    for agent_id in ["alice", "bob"] {
        let candidates = storage.agenda_candidates(agent_id).await.unwrap();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.room_id == "general" && candidate.stalled)
        );
    }

    let hub = ProactivityHub::default();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let now = std::time::Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let hub = hub.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            hub.try_claim_stall("general", now).unwrap()
        }));
    }
    let outcomes = [
        tasks.remove(0).await.unwrap(),
        tasks.remove(0).await.unwrap(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == NudgeClaim::Claimed)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == NudgeClaim::ClaimedByPeer)
            .count(),
        1
    );

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn actionable_stall_keeps_its_claim_until_dispatch_commits() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Advance stalled work".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    storage
        .send_message("general", "user", "This work stalled")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE collab_rooms
            SET last_message_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                - INTERVAL '10 minutes'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE collab_agents
            SET created_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                - INTERVAL '2 minutes'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let hub = ProactivityHub::default();
    let wake = AutonomyEngine::new(
        storage.clone(),
        hub.clone(),
        openwork_collab::observation::ObservationSink::discarding(),
    )
    .sweep_agenda()
    .await
    .unwrap()
    .expect("a missing cheap model fails open for a real stalled candidate");
    assert!(wake.stalled_claim);
    assert_eq!(
        hub.try_claim_stall("general", std::time::Instant::now())
            .unwrap(),
        NudgeClaim::ClaimedByPeer,
        "cheap approval alone must not finish the claim before dispatch"
    );
    hub.cancel_stall("general").unwrap();

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn empty_agenda_records_the_reason_without_starting_a_main_run() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Work only when the agenda has a real item".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    sqlx::query(
        "UPDATE collab_agents
            SET created_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '2 minutes'
          WHERE id = 'alice'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let wake = AutonomyEngine::new(
        storage.clone(),
        ProactivityHub::default(),
        openwork_collab::observation::ObservationSink::discarding(),
    )
    .sweep_agenda()
    .await
    .unwrap();
    assert!(wake.is_none());
    let records = storage.triage_records(Some("general")).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, "empty_inbox");
    assert!(
        records[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("no assigned or mentioned")
    );
    let run_count: i64 = sqlx::query_scalar("SELECT count(*) FROM collab_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        run_count, 0,
        "an empty agenda must spend no main-Agent turn"
    );

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn unfinished_assigned_card_produces_a_focused_agenda_wake() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: Some("implementer".to_string()),
            bio: None,
            system_prompt: "Execute assigned shared work".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    storage
        .create_board("work", "general", "Work")
        .await
        .unwrap();
    storage
        .create_board_column("todo", "work", "Not done", 0, false)
        .await
        .unwrap();
    storage
        .create_card(
            CardInput {
                board_id: "work".to_string(),
                column_id: "todo".to_string(),
                title: "Implement P5".to_string(),
                description: None,
                position: 0,
                assignee_id: Some("alice".to_string()),
            },
            "user",
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE collab_agents
            SET created_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '2 minutes'
          WHERE id = 'alice'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let wake = AutonomyEngine::new(
        storage.clone(),
        ProactivityHub::default(),
        openwork_collab::observation::ObservationSink::discarding(),
    )
    .sweep_agenda()
    .await
    .unwrap()
    .expect("agenda failures must not silence a real card candidate");
    assert_eq!(wake.agent_id, "alice");
    assert_eq!(wake.room_id, "general");
    assert_eq!(wake.trigger, "agenda");
    assert_eq!(wake.context["cards"][0]["title"], "Implement P5");
    let records = storage.triage_records(Some("general")).await.unwrap();
    assert_eq!(records[0].source, "fail_open");
    assert!(records[0].actionable);
    let marker = storage
        .insert_proactive_marker("general", "alice", "agenda", &wake.reason)
        .await
        .unwrap();
    assert_eq!(marker.kind, "system");
    assert_eq!(
        marker.system_payload.as_ref().unwrap()["type"],
        "proactive_wake"
    );
    assert_eq!(marker.system_payload.as_ref().unwrap()["trigger"], "agenda");

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn agent_dm_is_reused_by_its_member_set_for_loop_detection() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    for id in ["alice", "bob"] {
        storage
            .create_agent(&AgentInput {
                id: id.to_string(),
                display_name: id.to_string(),
                role: None,
                bio: None,
                system_prompt: "Make progress in direct work".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "model".to_string(),
                enabled: true,
                scanner_enabled: false,
            })
            .await
            .unwrap();
    }

    let first = storage.create_direct_room("alice", "bob").await.unwrap();
    let reused = storage.create_direct_room("bob", "alice").await.unwrap();
    assert_eq!(reused.id, first.id);
    assert_eq!(first.kind, "direct");
    assert!(
        storage
            .agent_direct_exchange(&first.id, 32)
            .await
            .unwrap()
            .is_some()
    );

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test]
async fn scanner_does_not_repeat_an_unchanged_cross_room_snapshot() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Observe cross-room changes".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "model".to_string(),
            enabled: true,
            scanner_enabled: true,
        })
        .await
        .unwrap();
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    for index in 1..=8 {
        storage
            .send_message("general", "user", &format!("activity {index}"))
            .await
            .unwrap();
    }
    sqlx::query(
        "UPDATE collab_agents
            SET created_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '2 minutes'
          WHERE id = 'alice'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let engine = AutonomyEngine::new(
        storage.clone(),
        ProactivityHub::default(),
        openwork_collab::observation::ObservationSink::discarding(),
    );

    assert!(
        engine.sweep_scanner().await.unwrap().is_empty(),
        "first snapshot is a no-cost baseline"
    );
    assert!(
        engine.sweep_scanner().await.unwrap().is_empty(),
        "unchanged snapshot must not wake again"
    );
    storage
        .send_message("general", "user", "activity 9")
        .await
        .unwrap();
    let changed = engine.sweep_scanner().await.unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].trigger, "scanner");
    assert_eq!(changed[0].room_id, "general");
    storage
        .insert_proactive_marker(
            "general",
            "alice",
            "scanner",
            "reviewed the changed snapshot",
        )
        .await
        .unwrap();
    storage
        .send_message("general", "alice", "I reviewed this change")
        .await
        .unwrap();
    assert!(
        engine.sweep_scanner().await.unwrap().is_empty(),
        "the scanner must not wake itself for its own marker or reply"
    );

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

async fn test_storage() -> Option<(PgPool, PgPool, String, CollabStorage)> {
    let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_proactivity_test_{}", Uuid::new_v4().simple());
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
    Some((admin, pool, schema, storage))
}
