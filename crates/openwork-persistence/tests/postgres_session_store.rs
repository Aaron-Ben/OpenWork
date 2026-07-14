use std::sync::Arc;

mod common;

use common::{connect_test_pool, test_config};
use openwork_persistence::{
    NewMessage, PostgresEventJournal, PostgresPersistence, SessionError, SessionInput,
    SessionStore, TurnOutcome,
};
use openwork_protocol::model::{ContentBlock, Role};
use uuid::Uuid;

async fn test_store() -> Option<SessionStore> {
    let config = test_config(4)?;
    PostgresPersistence::migrate_database(config.clone())
        .await
        .ok()?;
    let journal = Arc::new(PostgresEventJournal::new(
        connect_test_pool(&config).await.ok()?,
    ));
    Some(SessionStore::new(journal))
}

#[tokio::test]
async fn a_turn_cannot_be_finished_under_a_different_session() {
    let Some(store) = test_store().await else {
        return;
    };
    let first = store.create_session(input()).await.unwrap();
    let second = store.create_session(input()).await.unwrap();
    let turn_id = format!("turn-{}", Uuid::new_v4().simple());
    store
        .start_turn(
            &turn_id,
            &first.id,
            NewMessage {
                role: Role::User,
                parts: vec![ContentBlock::text("hello")],
            },
        )
        .await
        .unwrap();

    let error = store
        .finish_turn(&turn_id, &second.id, Vec::new(), TurnOutcome::Completed)
        .await
        .unwrap_err();
    assert!(matches!(error, SessionError::InvalidEvent { .. }));
}

fn input() -> SessionInput {
    SessionInput {
        title: Some("Journal session".to_string()),
        provider_id: "provider-1".to_string(),
        model: "model-1".to_string(),
        working_dir: Some("/tmp/openwork".to_string()),
    }
}

#[tokio::test]
async fn session_events_replace_session_table_create_list_rename_and_delete() {
    let Some(store) = test_store().await else {
        return;
    };

    let created = store.create_session(input()).await.unwrap();
    assert!(store.load_session(&created.id).await.unwrap().is_some());
    assert!(
        store
            .list_sessions()
            .await
            .unwrap()
            .iter()
            .any(|session| session.id == created.id)
    );

    let renamed = store.rename_session(&created.id, "Renamed").await.unwrap();
    assert_eq!(renamed.title, "Renamed");

    store.delete_session(&created.id).await.unwrap();
    assert!(store.load_session(&created.id).await.unwrap().is_none());
    assert!(
        !store
            .list_sessions()
            .await
            .unwrap()
            .iter()
            .any(|session| session.id == created.id)
    );
}

#[tokio::test]
async fn turn_message_events_replace_messages_table_and_replay_in_order() {
    let Some(store) = test_store().await else {
        return;
    };
    let session = store.create_session(input()).await.unwrap();
    let turn_id = format!("turn-{}", Uuid::new_v4().simple());

    store
        .start_turn(
            &turn_id,
            &session.id,
            NewMessage {
                role: Role::User,
                parts: vec![ContentBlock::text("hello")],
            },
        )
        .await
        .unwrap();
    store
        .finish_turn(
            &turn_id,
            &session.id,
            vec![NewMessage {
                role: Role::Assistant,
                parts: vec![ContentBlock::text("world")],
            }],
            TurnOutcome::Completed,
        )
        .await
        .unwrap();

    let messages = store.load_messages(&session.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, Role::User);
    assert_eq!(messages[1].role, Role::Assistant);
    assert_eq!(messages[0].seq, 1);
    assert_eq!(messages[1].seq, 2);
}
