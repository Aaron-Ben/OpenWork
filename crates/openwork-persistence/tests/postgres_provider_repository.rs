mod common;

use common::{connect_test_pool, test_config};
use openwork_persistence::{ApiKeyCipher, PostgresPersistence, PostgresProviderRepository};
use openwork_protocol::provider::{
    ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderRepository,
};

fn input(name: &str, models: &[(&str, ModelTier)]) -> ProviderInput {
    ProviderInput {
        name: name.to_string(),
        base_url: "https://api.example.com".to_string(),
        api_key: "test-key".to_string(),
        kind: ProviderKind::Deepseek,
        models: models
            .iter()
            .map(|(model_id, model_tier)| ProviderModel {
                model_id: (*model_id).to_string(),
                display_name: None,
                model_tier: *model_tier,
                enabled: true,
            })
            .collect(),
        enabled: true,
        extra_body: None,
    }
}

#[tokio::test]
async fn provider_and_models_share_a_postgres_repository_transaction() {
    let Some(config) = test_config(2) else {
        return;
    };
    PostgresPersistence::migrate_database(config.clone())
        .await
        .unwrap();
    let pool = connect_test_pool(&config).await.unwrap();
    sqlx::query("DROP TABLE IF EXISTS provider_models, providers CASCADE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM schema_migrations WHERE name = 'create_provider_registry'")
        .execute(&pool)
        .await
        .unwrap();
    PostgresPersistence::migrate_database(config).await.unwrap();
    let timestamp_columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name, data_type
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name IN ('providers', 'provider_models')
           AND column_name IN ('created_at', 'updated_at', 'deleted_at')
         ORDER BY table_name, column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(timestamp_columns.len(), 6);
    assert!(
        timestamp_columns
            .iter()
            .all(|(_, data_type)| data_type == "timestamp without time zone")
    );
    let repository = PostgresProviderRepository::new(pool, ApiKeyCipher::from_key([7; 32]));
    sqlx::query("TRUNCATE TABLE providers CASCADE")
        .execute(repository.pool())
        .await
        .unwrap();
    let custom_driver_insert = sqlx::query(
        "INSERT INTO providers
         (id, name, base_url, api_key_encrypted, driver_code, enabled, active)
         VALUES ('prov-custom', 'Custom', 'https://example.com', 'v1:test',
                 'openai_chat_standard', true, false)",
    )
    .execute(repository.pool())
    .await;
    assert!(custom_driver_insert.is_err());

    let first = repository
        .create(input(
            "first",
            &[
                ("model-b", ModelTier::Lite),
                ("model-a", ModelTier::Pro),
                ("model-b", ModelTier::Plus),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(first.models.len(), 2);
    assert_eq!(first.models[0].model_id, "model-b");
    assert_eq!(first.models[0].model_tier, ModelTier::Lite);
    assert_eq!(first.models[1].model_id, "model-a");
    assert_eq!(first.models[1].model_tier, ModelTier::Pro);
    assert_eq!(repository.active_id().await.unwrap().unwrap(), first.id);
    let created_at_is_beijing_local: bool = sqlx::query_scalar(
        "SELECT created_at BETWEEN
             (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '5 seconds'
             AND (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') + INTERVAL '5 seconds'
         FROM providers WHERE id = $1",
    )
    .bind(&first.id)
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert!(created_at_is_beijing_local);
    assert_eq!(
        repository
            .load_runtime(&first.id)
            .await
            .unwrap()
            .unwrap()
            .credential
            .expose(),
        "test-key"
    );

    let stored_api_key: String =
        sqlx::query_scalar("SELECT api_key_encrypted FROM providers WHERE id = $1")
            .bind(&first.id)
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert!(stored_api_key.starts_with("v1:"));
    assert!(!stored_api_key.contains("test-key"));

    let api_key_columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'providers'
           AND column_name LIKE 'api_key%'",
    )
    .fetch_all(repository.pool())
    .await
    .unwrap();
    assert_eq!(api_key_columns, vec!["api_key_encrypted"]);

    repository
        .update(&first.id, input("first", &[("model-a", ModelTier::Pro)]))
        .await
        .unwrap();
    let removed_model_state: (bool, bool) = sqlx::query_as(
        "SELECT is_deleted, deleted_at IS NOT NULL
         FROM provider_models WHERE provider_id = $1 AND model_id = 'model-b'",
    )
    .bind(&first.id)
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert_eq!(removed_model_state, (true, true));

    repository
        .update(
            &first.id,
            input(
                "first",
                &[("model-b", ModelTier::Lite), ("model-a", ModelTier::Pro)],
            ),
        )
        .await
        .unwrap();
    let restored_model_state: (bool, bool) = sqlx::query_as(
        "SELECT is_deleted, deleted_at IS NULL
         FROM provider_models WHERE provider_id = $1 AND model_id = 'model-b'",
    )
    .bind(&first.id)
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert_eq!(restored_model_state, (false, true));

    let second = repository
        .create(input("second", &[("model-c", ModelTier::Plus)]))
        .await
        .unwrap();
    repository.activate(&second.id).await.unwrap();
    repository.delete(&first.id).await.unwrap();
    assert!(repository.get_profile(&first.id).await.unwrap().is_none());
    assert!(repository.load_runtime(&first.id).await.unwrap().is_none());

    let provider_deletion_state: (bool, bool) =
        sqlx::query_as("SELECT is_deleted, deleted_at IS NOT NULL FROM providers WHERE id = $1")
            .bind(&first.id)
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(provider_deletion_state, (true, true));

    let child_counts: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE NOT is_deleted),
                COUNT(*) FILTER (WHERE is_deleted)
         FROM provider_models WHERE provider_id = $1",
    )
    .bind(&first.id)
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert_eq!(child_counts, (0, 2));
}
