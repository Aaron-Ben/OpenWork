use std::str::FromStr;

use openwork_collab::{
    event::CollabEventPublisher,
    model::{AgentInput, TriageRecordInput},
    observation::{ObservationSink, OwnedObservation, TokenUsage, record_triage, start},
    storage::CollabStorage,
};
use serde_json::json;
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn sideband_worker_persists_triage_and_deduplicated_message_usage() {
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
    let run_id = storage
        .begin_run(
            &storage.agent("alice").await.unwrap().unwrap(),
            Some("general"),
            "message",
        )
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let events = CollabEventPublisher::default();
    let mut committed = events.subscribe(0).await.receiver;
    let (sink, worker) = start(storage.clone(), events, cancel.clone());
    sink.set_active_run("alice", Some(&run_id));
    sink.record(OwnedObservation::for_active_run(
        "alice",
        Some("general"),
        "speech.published",
        json!({"messageId": "reply_1", "sequence": 2}),
    ));
    sink.set_active_run("alice", None);
    storage
        .finish_run(&run_id, "completed", None)
        .await
        .unwrap();

    record_triage(
        &storage,
        &sink,
        TriageRecordInput {
            agent_id: "alice",
            room_id: "general",
            up_to_sequence: 1,
            actionable: true,
            response_mode: Some("me"),
            source: "support_model",
            reason: Some("actionable"),
            prompt_note: None,
            provider_id: Some("cheap"),
            model_id: Some("small"),
            input_tokens: Some(3),
            output_tokens: Some(1),
            latency_ms: 5,
        },
    )
    .await
    .unwrap();
    record_usage(&sink, &run_id, "msg_1", 10, 2, 4);
    record_usage(&sink, &run_id, "msg_1", 12, 3, 5);
    record_usage(&sink, &run_id, "msg_1", 12, 3, 5);
    record_usage(&sink, &run_id, "msg_2", 7, 1, 2);

    for _ in 0..5 {
        tokio::time::timeout(std::time::Duration::from_secs(2), committed.recv())
            .await
            .expect("observation worker commit notification")
            .unwrap();
    }
    let (input, cached, output): (Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT input_tokens, cached_input_tokens, output_tokens
           FROM collab_runs WHERE id = $1",
    )
    .bind(&run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((input, cached, output), (Some(19), Some(4), Some(7)));
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM collab_events ORDER BY created_at, id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "triage.decision")
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "usage.reported")
            .count(),
        3
    );
    let speech_run_id: Option<String> =
        sqlx::query_scalar("SELECT run_id FROM collab_events WHERE kind = 'speech.published'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(speech_run_id.as_deref(), Some(run_id.as_str()));

    cancel.cancel();
    worker.shutdown().await;
    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

fn record_usage(
    sink: &ObservationSink,
    run_id: &str,
    message_id: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
) {
    sink.record(
        OwnedObservation::linked(
            Some(run_id),
            Some("alice"),
            Some("general"),
            "usage.reported",
            json!({
                "messageId": message_id,
                "inputTokens": input_tokens,
                "cachedInputTokens": cached_input_tokens,
                "outputTokens": output_tokens,
            }),
        )
        .with_usage(Some(TokenUsage {
            input_tokens,
            cached_input_tokens,
            output_tokens,
        })),
    );
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
