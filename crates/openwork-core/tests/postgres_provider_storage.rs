use std::sync::Arc;

use openwork_core::{ApiKeyCipher, ModelCapabilities, PostgresProviderRepository, PostgresStorage};
use openwork_models::provider::{
    ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderRepository,
};
use uuid::Uuid;

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn test_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        context_window_tokens: 200_000,
        max_output_tokens: 32_768,
        max_reasoning_tokens: None,
        accepts_data_blocks: true,
    }
}

#[tokio::test]
async fn core_provider_storage_owns_encrypted_crud_and_model_projection() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    let repository =
        PostgresProviderRepository::new(storage.pool().clone(), ApiKeyCipher::from_key([37; 32]));
    sqlx::query(
        "DELETE FROM models WHERE credential_ref IN (
            SELECT 'provider:' || provider_id
            FROM provider_credentials
            WHERE display_name LIKE 'provider-core-test-%'
        )",
    )
    .execute(storage.pool())
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM provider_credentials
         WHERE display_name LIKE 'provider-core-test-%'",
    )
    .execute(storage.pool())
    .await
    .unwrap();
    let provider_name = unique("provider-core-test");
    let input = ProviderInput {
        name: provider_name.clone(),
        base_url: "https://example.invalid/v1".to_string(),
        api_key: "test-secret-never-logged".to_string(),
        kind: ProviderKind::Deepseek,
        models: vec![
            ProviderModel {
                model_id: "model-a".to_string(),
                display_name: Some("Model A".to_string()),
                model_tier: ModelTier::Lite,
                enabled: true,
                capabilities: Some(test_capabilities()),
            },
            ProviderModel {
                model_id: "model-b".to_string(),
                display_name: None,
                model_tier: ModelTier::Pro,
                enabled: false,
                capabilities: Some(test_capabilities()),
            },
        ],
        enabled: true,
        extra_body: Some(
            serde_json::json!({ "reasoning_effort": "high" })
                .as_object()
                .unwrap()
                .clone(),
        ),
    };

    let created = repository.create(input.clone()).await.unwrap();
    assert_eq!(created.name, provider_name);
    assert_eq!(created.models.len(), 2);

    let runtime = repository.load_runtime(&created.id).await.unwrap().unwrap();
    assert_eq!(runtime.credential.expose(), "test-secret-never-logged");
    assert_eq!(runtime.profile, created);
    assert_eq!(
        runtime
            .adapter_options
            .as_ref()
            .and_then(|options| options.get("reasoning_effort"))
            .and_then(serde_json::Value::as_str),
        Some("high")
    );

    let updated = repository
        .update(
            &created.id,
            ProviderInput {
                name: provider_name.clone(),
                api_key: String::new(),
                models: vec![ProviderModel {
                    model_id: "model-c".to_string(),
                    display_name: Some("Model C".to_string()),
                    model_tier: ModelTier::Plus,
                    enabled: true,
                    capabilities: Some(test_capabilities()),
                }],
                ..input
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.models.len(), 1);
    assert_eq!(updated.models[0].model_id, "model-c");
    let runtime = repository.load_runtime(&created.id).await.unwrap().unwrap();
    assert_eq!(runtime.credential.expose(), "test-secret-never-logged");
    assert_eq!(
        runtime
            .adapter_options
            .as_ref()
            .and_then(|options| options.get("reasoning_effort"))
            .and_then(serde_json::Value::as_str),
        Some("high")
    );

    repository.delete(&created.id).await.unwrap();
    assert!(repository.get_profile(&created.id).await.unwrap().is_none());

    let remaining_models: i64 =
        sqlx::query_scalar("SELECT count(*) FROM models WHERE credential_ref = $1")
            .bind(format!("provider:{}", created.id))
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(remaining_models, 0);
}

#[test]
fn api_key_cipher_debug_output_is_always_redacted() {
    let cipher = Arc::new(ApiKeyCipher::from_key([11; 32]));
    assert_eq!(format!("{cipher:?}"), "ApiKeyCipher([REDACTED])");
}
