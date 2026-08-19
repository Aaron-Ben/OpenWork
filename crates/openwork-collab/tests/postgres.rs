use std::str::FromStr;

use openwork_collab::{model::AgentInput, storage::CollabStorage};
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
    assert_eq!(applied, 1);

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
