use std::sync::Arc;

mod common;

use common::{connect_test_pool, test_config};
use openwork_persistence::{
    NewMessage, PostgresEventJournal, PostgresPersistence, SessionError, SessionInput,
    SessionStore, TurnOutcome,
};
use openwork_protocol::model::{ContentBlock, Role};
use openwork_protocol::{
    approval::ApprovalRequested,
    domain::{ApprovalId, StepId, ToolRunId, TurnId},
    turn::{
        AssistantMessageRecorded, StepStarted, ToolRunRequested, TurnRecordedEvent,
        TurnRecorderPort,
    },
};
use serde_json::json;
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
    assert_eq!(messages[0].turn_id, turn_id);
    assert_eq!(messages[1].turn_id, messages[0].turn_id);
    assert_eq!(messages[0].seq, 1);
    assert_eq!(messages[1].seq, 2);
}

#[tokio::test]
async fn step_messages_and_pending_approval_replay_from_the_same_turn_stream() {
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
                parts: vec![ContentBlock::text("run tests")],
            },
        )
        .await
        .unwrap();
    let recorder = store.turn_recorder(&turn_id, &session.id);
    let step_id = StepId::new("step-1");
    let tool_run_id = ToolRunId::new("tool-run-1");
    recorder
        .append(vec![
            TurnRecordedEvent::StepStarted(StepStarted {
                step_id: step_id.clone(),
                step_index: 1,
            }),
            TurnRecordedEvent::AssistantMessageRecorded(AssistantMessageRecorded {
                message_id: "message-2".to_string(),
                step_id: step_id.clone(),
                parts: vec![ContentBlock::text("I will run the tests")],
            }),
            TurnRecordedEvent::ToolRunRequested(ToolRunRequested {
                step_id: step_id.clone(),
                tool_run_id: tool_run_id.clone(),
                provider_tool_call_id: "call-1".to_string(),
                tool_name: "bash".to_string(),
                input: json!({"command": "cargo test"}),
            }),
            TurnRecordedEvent::ApprovalRequested(ApprovalRequested {
                approval_id: ApprovalId::new("approval-1"),
                turn_id: TurnId::new(&turn_id),
                step_id,
                tool_run_id,
                tool_name: "bash".to_string(),
                input: json!({"command": "cargo test"}),
                reason: "process execution requires approval".to_string(),
            }),
        ])
        .await
        .unwrap();

    let messages = store.load_messages(&session.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].role, Role::Assistant);
    assert_eq!(messages[0].step_id, None);
    assert_eq!(messages[1].step_id.as_deref(), Some("step-1"));
    assert_eq!(messages[1].tool_run_id, None);
    let lifecycle = store
        .load_turn_lifecycle(&turn_id)
        .await
        .unwrap()
        .expect("turn lifecycle");
    assert_eq!(
        lifecycle
            .pending_approval
            .expect("pending approval")
            .provider_tool_call_id,
        "call-1"
    );
}
