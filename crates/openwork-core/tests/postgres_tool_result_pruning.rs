//! compaction.md §2: the pruning watermark is persisted per Session and
//! only moves forward, so a restart keeps pruned results pruned.

use openwork_core::{PostgresStorage, SessionId, SessionInput, StorageError};
use uuid::Uuid;

#[tokio::test]
async fn watermark_persists_and_never_moves_back() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    let session_id = SessionId::new(format!("session-pruning-{}", Uuid::new_v4().simple()));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: None,
            working_directory: "/tmp".to_string(),
            default_model_id: None,
        })
        .await
        .unwrap();

    assert_eq!(
        storage
            .load_tool_result_pruned_through(&session_id)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        storage
            .advance_tool_result_pruned_through(&session_id, 7)
            .await
            .unwrap(),
        7
    );
    assert_eq!(
        storage
            .advance_tool_result_pruned_through(&session_id, 3)
            .await
            .unwrap(),
        7,
        "an older watermark never un-prunes results"
    );
    assert!(matches!(
        storage
            .advance_tool_result_pruned_through(&session_id, 0)
            .await,
        Err(StorageError::InvalidInput(_))
    ));

    let restarted = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    assert_eq!(
        restarted
            .load_tool_result_pruned_through(&session_id)
            .await
            .unwrap(),
        Some(7)
    );
    assert!(matches!(
        restarted
            .load_tool_result_pruned_through(&SessionId::new("session-missing"))
            .await,
        Err(StorageError::SessionNotFound(_))
    ));

    restarted.delete_session(&session_id).await.unwrap();
}
