use std::time::Duration;

use openwork_database::{Database, DatabaseConfig};
use openwork_persistence::{PROVIDER_MIGRATIONS, PostgresProviderRepository};
use openwork_protocol::provider::{
    ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderRepository,
};

fn input(name: &str, models: &[(&str, ModelTier)]) -> ProviderInput {
    ProviderInput {
        name: name.to_string(),
        base_url: "https://api.example.com".to_string(),
        api_key: "test-key".to_string(),
        kind: ProviderKind::OpenaiCompatible,
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
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(5),
    })
    .await
    .unwrap();
    database.migrate(PROVIDER_MIGRATIONS).await.unwrap();
    let repository = PostgresProviderRepository::new(database.pool().clone());
    sqlx::query("TRUNCATE TABLE providers CASCADE")
        .execute(repository.pool())
        .await
        .unwrap();

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

    let second = repository
        .create(input("second", &[("model-c", ModelTier::Plus)]))
        .await
        .unwrap();
    repository.activate(&second.id).await.unwrap();
    repository.delete(&first.id).await.unwrap();
    assert!(repository.get_profile(&first.id).await.unwrap().is_none());

    let child_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM provider_models WHERE provider_id = $1")
            .bind(&first.id)
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(child_count, 0);
}
