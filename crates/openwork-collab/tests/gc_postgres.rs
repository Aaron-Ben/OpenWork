use std::str::FromStr;

use openwork_collab::{
    model::AgentInput,
    storage::{CollabGcPolicy, CollabStorage},
};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

#[tokio::test]
async fn gc_deletes_only_expired_events_and_triages_in_bounded_batches() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
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
    storage
        .create_group_room("general", "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    storage
        .send_message("general", "user", "historical room message")
        .await
        .unwrap();
    let run_id = storage
        .begin_run(
            &storage.agent("alice").await.unwrap().unwrap(),
            Some("general"),
            "message",
        )
        .await
        .unwrap();
    storage
        .finish_run(&run_id, "completed", None)
        .await
        .unwrap();

    for (id, age) in [
        ("event-old-1", "40 days"),
        ("event-old-2", "31 days"),
        ("event-fresh", "29 days"),
    ] {
        sqlx::query(
            "INSERT INTO collab_events (id, run_id, agent_id, room_id, kind, payload, created_at)
             VALUES ($1, $2, 'alice', 'general', 'test.event', '{}'::jsonb,
                     (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - $3::INTERVAL)",
        )
        .bind(id)
        .bind(&run_id)
        .bind(age)
        .execute(&pool)
        .await
        .unwrap();
    }
    for (id, age) in [
        ("triage-old-1", "40 days"),
        ("triage-old-2", "31 days"),
        ("triage-fresh", "29 days"),
    ] {
        sqlx::query(
            "INSERT INTO collab_triages (
                id, agent_id, room_id, up_to_seq, actionable, source, latency_ms, created_at
             ) VALUES (
                $1, 'alice', 'general', 1, FALSE, 'empty_inbox', 0,
                (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - $2::INTERVAL
             )",
        )
        .bind(id)
        .bind(age)
        .execute(&pool)
        .await
        .unwrap();
    }

    let policy = CollabGcPolicy::new(30, 30, 1, 2_000).unwrap();
    let first = storage.gc_batch(policy).await.unwrap();
    assert_eq!(first.events_deleted, 1);
    assert_eq!(first.triages_deleted, 1);
    let completed = storage.garbage_collect(policy).await.unwrap();
    assert_eq!(completed.events_deleted, 1);
    assert_eq!(completed.triages_deleted, 1);

    let event_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM collab_events ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let triage_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM collab_triages ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let message_count: i64 = sqlx::query_scalar("SELECT count(*) FROM collab_messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    let run_count: i64 = sqlx::query_scalar("SELECT count(*) FROM collab_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event_ids, ["event-fresh"]);
    assert_eq!(triage_ids, ["triage-fresh"]);
    assert_eq!(
        message_count, 1,
        "room history must never be garbage-collected"
    );
    assert_eq!(run_count, 1, "run retention is independent from P6 GC");

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

async fn test_storage() -> Option<(PgPool, PgPool, String, CollabStorage)> {
    let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
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
    Some((admin, pool, schema, storage))
}
