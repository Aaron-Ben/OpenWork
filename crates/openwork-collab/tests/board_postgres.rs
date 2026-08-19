use std::str::FromStr;

use openwork_collab::{
    model::{AgentInput, CardClaimOutcome, CardInput},
    storage::CollabStorage,
};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

#[tokio::test]
async fn concurrent_card_claim_has_exactly_one_winner() {
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
                system_prompt: "Work from the shared board".to_string(),
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
    for id in ["user", "alice", "bob"] {
        storage.add_member("general", id).await.unwrap();
    }
    storage
        .create_board("work", "general", "Shared work")
        .await
        .unwrap();
    storage
        .create_board_column("todo", "work", "Todo", 0, false)
        .await
        .unwrap();
    storage
        .create_board_column("archive", "work", "Archive", 1, true)
        .await
        .unwrap();
    let created = storage
        .create_card(
            CardInput {
                board_id: "work".to_string(),
                column_id: "todo".to_string(),
                title: "Own the database migration".to_string(),
                description: None,
                position: 0,
                assignee_id: Some("alice".to_string()),
            },
            "user",
        )
        .await
        .unwrap();
    let card_id = created.card.id;

    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for agent_id in ["alice", "bob"] {
        let storage = storage.clone();
        let barrier = barrier.clone();
        let card_id = card_id.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            (
                agent_id,
                storage.claim_card(&card_id, agent_id).await.unwrap(),
            )
        }));
    }
    let outcomes = [
        tasks.remove(0).await.unwrap(),
        tasks.remove(0).await.unwrap(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, outcome)| matches!(outcome, CardClaimOutcome::Claimed(_)))
            .count(),
        1
    );
    let winner = outcomes
        .iter()
        .find_map(|(agent_id, outcome)| {
            matches!(outcome, CardClaimOutcome::Claimed(_)).then_some(*agent_id)
        })
        .unwrap();
    let loser = outcomes
        .iter()
        .find_map(|(_, outcome)| match outcome {
            CardClaimOutcome::AlreadyClaimed { claimed_by, .. } => Some(claimed_by.as_str()),
            CardClaimOutcome::Claimed(_) => None,
        })
        .unwrap();
    assert_eq!(loser, winner);
    let losing_wire = serde_json::to_value(
        outcomes
            .iter()
            .find_map(|(_, outcome)| {
                matches!(outcome, CardClaimOutcome::AlreadyClaimed { .. }).then_some(outcome)
            })
            .unwrap(),
    )
    .unwrap();
    assert_eq!(losing_wire["status"], "already_claimed");
    assert_eq!(losing_wire["result"]["claimedBy"], winner);

    let boards = storage.boards("general").await.unwrap();
    assert_eq!(
        boards[0].columns[0].cards[0].claimed_by.as_deref(),
        Some(winner)
    );
    let messages = storage.room_messages("general").await.unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().all(|message| message.kind == "system"));
    assert_eq!(
        messages[0]
            .system_payload
            .as_ref()
            .and_then(|payload| payload.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("card_created")
    );
    assert!(messages[0].body.contains("assigned it to alice"));
    assert_eq!(
        messages[1]
            .system_payload
            .as_ref()
            .and_then(|payload| payload.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("card_claimed")
    );

    let startup_release = storage.release_all_claims().await.unwrap();
    assert_eq!(startup_release.count, 1);
    assert_eq!(startup_release.room_ids, vec!["general"]);
    assert_eq!(storage.room_messages("general").await.unwrap().len(), 2);
    assert!(
        storage.boards("general").await.unwrap()[0].columns[0].cards[0]
            .claimed_by
            .is_none()
    );

    let reclaimed = storage.claim_card(&card_id, winner).await.unwrap();
    assert!(matches!(reclaimed, CardClaimOutcome::Claimed(_)));
    let manual_release = storage
        .release_card_claim(&card_id, winner, "user", "user_cancelled")
        .await
        .unwrap()
        .unwrap();
    assert!(manual_release.card.claimed_by.is_none());
    assert_eq!(
        manual_release
            .message
            .system_payload
            .as_ref()
            .and_then(|payload| payload.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("card_claim_released")
    );

    let moved = storage
        .move_card(&card_id, "archive", 0, "user")
        .await
        .unwrap();
    assert_eq!(moved.card.column_id, "archive");
    assert_eq!(
        moved
            .message
            .system_payload
            .as_ref()
            .and_then(|payload| payload.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("card_moved")
    );
    let boards = storage.boards("general").await.unwrap();
    let archive = boards[0]
        .columns
        .iter()
        .find(|column| column.id == "archive")
        .unwrap();
    assert!(archive.is_done);
    assert_eq!(archive.cards[0].id, card_id);

    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

async fn test_storage() -> Option<(PgPool, PgPool, String, CollabStorage)> {
    let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_board_test_{}", Uuid::new_v4().simple());
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
