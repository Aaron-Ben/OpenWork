use std::str::FromStr;

use openwork_collab::{
    model::{AgentInput, ObservationInput, TriageRecordInput},
    storage::CollabStorage,
};
use serde_json::json;
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

#[tokio::test]
async fn flat_log_timeline_combines_runs_triages_and_open_ended_events() {
    let Some((admin, pool, schema, storage)) = test_storage().await else {
        return;
    };
    storage
        .create_agent(&AgentInput {
            id: Some("alice".to_string()),
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
        .create_group_room(Some("general"), "General")
        .await
        .unwrap();
    for participant in ["user", "alice"] {
        storage.add_member("general", participant).await.unwrap();
    }
    let triage = storage
        .record_triage(TriageRecordInput {
            agent_id: "alice",
            room_id: "general",
            up_to_sequence: 1,
            actionable: true,
            response_mode: Some("me"),
            source: "support_model",
            reason: Some("Alice owns the work"),
            prompt_note: Some("Reply with the result"),
            provider_id: Some("cheap"),
            model_id: Some("small"),
            input_tokens: Some(12),
            output_tokens: Some(4),
            latency_ms: 25,
        })
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
    for (kind, payload) in [
        ("triage.decision", json!({"triageId": triage.id})),
        ("prompt.started", json!({"trigger": "message"})),
        (
            "command.execution",
            json!({"tool": "bash", "status": "completed"}),
        ),
        (
            "speech.published",
            json!({"messageId": "msg_1", "sequence": 2}),
        ),
        (
            "usage.reported",
            json!({"inputTokens": 12, "outputTokens": 4}),
        ),
        (
            "future.open_code_kind",
            json!({"shape": "must remain accepted"}),
        ),
        ("prompt.completed", json!({"status": "completed"})),
    ] {
        storage
            .insert_observation(ObservationInput {
                run_id: Some(&run_id),
                agent_id: Some("alice"),
                room_id: Some("general"),
                kind,
                payload,
            })
            .await
            .unwrap();
    }
    storage
        .finish_run(&run_id, "completed", Some("silent"), None)
        .await
        .unwrap();

    let timeline = storage.log_entries(Some("general"), 100).await.unwrap();
    assert!(timeline.iter().any(|entry| entry.source == "run"));
    assert!(timeline.iter().any(|entry| entry.source == "triage"));
    assert!(timeline.iter().any(|entry| entry.source == "event"));
    let event_kinds = timeline
        .iter()
        .filter(|entry| entry.source == "event")
        .map(|entry| entry.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        event_kinds,
        [
            "prompt.completed",
            "future.open_code_kind",
            "usage.reported",
            "speech.published",
            "command.execution",
            "prompt.started",
            "triage.decision",
        ]
    );
    assert!(
        timeline
            .iter()
            .all(|entry| entry.created_at.ends_with("+08:00"))
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
