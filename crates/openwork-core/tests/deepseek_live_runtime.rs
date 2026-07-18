use std::sync::Arc;
use std::time::Duration;

use openwork_core::{
    ClientRequestId, ModelInput, OpenWorkCore, PermissionDecision, PostgresStorage, SessionId,
    SessionInput, SessionUpdate, ToolCallId, session::TurnId,
};
use openwork_models::model::ContentBlock;
use serde_json::json;
use uuid::Uuid;

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL, DEEPSEEK_API_KEY, and network access"]
async fn deepseek_v4_flash_completes_a_durable_runtime_turn() {
    let database_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY");

    let storage = Arc::new(PostgresStorage::connect(Some(&database_url)).await.unwrap());
    let core = OpenWorkCore::from_storage(Arc::clone(&storage))
        .await
        .unwrap();
    let model_id = unique("model-deepseek-live");
    core.register_model(&ModelInput {
        id: model_id.clone(),
        display_name: "DeepSeek V4 Flash live test".to_string(),
        provider_kind: "deepseek".to_string(),
        model_name: "deepseek-v4-flash".to_string(),
        base_url: "https://api.deepseek.com".to_string(),
        credential_ref: Some("DEEPSEEK_API_KEY".to_string()),
        enabled: true,
        config: json!({}),
    })
    .await
    .unwrap();

    let session_id = SessionId::new(unique("session-deepseek-live"));
    core.create_session(&SessionInput {
        id: session_id.clone(),
        title: Some("DeepSeek live smoke test".to_string()),
        working_directory: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        default_model_id: Some(model_id.clone()),
    })
    .await
    .unwrap();

    let mut updates = core.subscribe_updates();
    let accepted = core
        .start_turn(
            &session_id,
            ClientRequestId::new(unique("request-deepseek-live")),
            vec![ContentBlock::text(
                "Reply with exactly: OPENWORK_RUNTIME_OK. Do not call tools.",
            )],
        )
        .await
        .unwrap();

    let outcome = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let envelope = updates.recv().await.unwrap();
            match envelope.update {
                SessionUpdate::PermissionRequested { request } => {
                    core.resolve_permission(
                        &session_id,
                        TurnId::new(envelope.turn_id.to_string()),
                        ToolCallId::new(request.tool_call_id.to_string()),
                        PermissionDecision::Allow,
                    )
                    .await
                    .unwrap();
                }
                SessionUpdate::TurnFinished { outcome } => break outcome,
                _ => {}
            }
        }
    })
    .await
    .expect("DeepSeek turn timed out");

    let final_text = match outcome {
        openwork_core::TurnOutcome::Completed { final_text } => final_text,
        other => panic!("unexpected live turn outcome: {other:?}"),
    };
    assert!(final_text.contains("OPENWORK_RUNTIME_OK"));
    let loaded = core.load_session(&session_id).await.unwrap();
    assert!(loaded.messages.len() >= 2);
    assert!(!core.get_trace(&accepted.turn_id).await.unwrap().is_empty());

    core.delete_session(&session_id).await.unwrap();
    sqlx::query("DELETE FROM models_v2 WHERE id = $1")
        .bind(model_id)
        .execute(storage.pool())
        .await
        .unwrap();
}
