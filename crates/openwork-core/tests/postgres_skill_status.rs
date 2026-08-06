use openwork_core::PostgresStorage;
use uuid::Uuid;

#[tokio::test]
async fn skill_status_persists_disabled_and_enabled_values_by_name() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let skill_name = format!("skill-status-{}", Uuid::new_v4().simple());
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();

    storage.set_skill_disabled(&skill_name, true).await.unwrap();
    assert!(
        storage
            .disabled_skill_names()
            .await
            .unwrap()
            .contains(&skill_name)
    );

    let restarted = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    assert!(
        restarted
            .disabled_skill_names()
            .await
            .unwrap()
            .contains(&skill_name)
    );

    restarted
        .set_skill_disabled(&skill_name, false)
        .await
        .unwrap();
    assert!(
        !restarted
            .disabled_skill_names()
            .await
            .unwrap()
            .contains(&skill_name)
    );

    sqlx::query("DELETE FROM skill_status WHERE name = $1")
        .bind(skill_name)
        .execute(restarted.pool())
        .await
        .unwrap();
}
