//! Storage-level contract for sub-agent Sessions.
//!
//! Covers the P0 acceptance items in `docs/multi-agent.md` §11 that only the
//! database can answer: list filtering, name uniqueness, name format, depth, and
//! cascade delete.

use openwork_core::{
    ClientRequestId, ModelCapabilities, PostgresStorage, ResolvedModel, SessionId, SessionInput,
    SessionStorage, SubAgentSessionInput, TurnOutcome, session::TurnId,
};
use openwork_models::model::{Message, Role};
use uuid::Uuid;

fn test_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

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

async fn storage() -> Option<PostgresStorage> {
    let database_url = test_database_url()?;
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    Some(storage)
}

async fn root(storage: &PostgresStorage) -> SessionId {
    let session_id = SessionId::new(unique("session-root"));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Root".to_string()),
            working_directory: "/tmp/openwork-sub-agent".to_string(),
            default_model_id: None,
        })
        .await
        .unwrap();
    session_id
}

fn sub_agent(parent: &SessionId, task_name: &str) -> SubAgentSessionInput {
    SubAgentSessionInput {
        id: SessionId::new(unique("session-sub")),
        parent_session_id: parent.clone(),
        task_name: task_name.to_string(),
        agent_role: "explorer".to_string(),
        working_directory: "/tmp/openwork-sub-agent".to_string(),
        default_model_id: None,
        spawn_span_id: Some("span-spawn-1".to_string()),
    }
}

#[tokio::test]
async fn a_sub_agent_session_records_its_parent_task_name_and_role() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;

    let record = storage
        .create_sub_agent_session(&sub_agent(&parent, "find_auth_flow"))
        .await
        .unwrap();

    assert!(record.is_sub_agent());
    assert_eq!(record.parent_session_id.as_deref(), Some(parent.as_str()));
    assert_eq!(record.task_name.as_deref(), Some("find_auth_flow"));
    assert_eq!(record.agent_role.as_deref(), Some("explorer"));
    assert_eq!(record.spawn_span_id.as_deref(), Some("span-spawn-1"));

    let parent_record = storage.load_session(&parent).await.unwrap().unwrap();
    assert!(
        !parent_record.is_sub_agent(),
        "a root session must stay a root session"
    );
    assert_eq!(parent_record.task_name, None);
}

#[tokio::test]
async fn list_sessions_hides_sub_agents_but_the_parent_can_list_them() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;
    let first = storage
        .create_sub_agent_session(&sub_agent(&parent, "find_auth_flow"))
        .await
        .unwrap();
    let second = storage
        .create_sub_agent_session(&sub_agent(&parent, "read_config"))
        .await
        .unwrap();

    let top_level = storage.list_sessions().await.unwrap();
    assert!(
        top_level
            .iter()
            .any(|session| session.id == parent.as_str()),
        "the root session must still be listed"
    );
    assert!(
        top_level.iter().all(|session| !session.is_sub_agent()),
        "no sub-agent may reach the top-level session list"
    );

    let children = storage.list_sub_agent_sessions(&parent).await.unwrap();
    let ids: Vec<&str> = children.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&first.id.as_str()));
    assert!(ids.contains(&second.id.as_str()));
}

#[tokio::test]
async fn a_duplicate_task_name_under_the_same_parent_is_rejected() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;
    storage
        .create_sub_agent_session(&sub_agent(&parent, "find_auth_flow"))
        .await
        .unwrap();

    let error = storage
        .create_sub_agent_session(&sub_agent(&parent, "find_auth_flow"))
        .await
        .expect_err("the unique index must reject the second one");
    assert!(
        error.to_string().contains("uq_sessions_parent_task_name")
            || error.to_string().contains("duplicate key"),
        "unexpected error: {error}"
    );

    // The same name under a different parent is fine: uniqueness is per parent.
    let other_parent = root(&storage).await;
    storage
        .create_sub_agent_session(&sub_agent(&other_parent, "find_auth_flow"))
        .await
        .expect("task names are scoped to their parent");
}

#[tokio::test]
async fn malformed_task_names_are_rejected() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;

    let too_long = "a".repeat(49);
    for name in [
        "",
        "Find_Auth",
        "9lives",
        "find-auth",
        "find auth",
        "_leading",
        too_long.as_str(),
    ] {
        let error = storage
            .create_sub_agent_session(&sub_agent(&parent, name))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("task_name"),
            "{name:?} produced {error}"
        );
    }

    let longest = format!("a{}", "b".repeat(47));
    storage
        .create_sub_agent_session(&sub_agent(&parent, &longest))
        .await
        .expect("48 characters is the documented maximum");
}

#[tokio::test]
async fn a_sub_agent_cannot_own_a_sub_agent() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;
    let child = storage
        .create_sub_agent_session(&sub_agent(&parent, "find_auth_flow"))
        .await
        .unwrap();

    let grandchild = SubAgentSessionInput {
        parent_session_id: SessionId::new(child.id.clone()),
        ..sub_agent(&parent, "deeper")
    };
    let error = storage
        .create_sub_agent_session(&grandchild)
        .await
        .expect_err("nesting depth is capped at one");
    assert!(
        error.to_string().contains("nesting depth"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn deleting_the_parent_cascades_to_its_sub_agents() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;
    let child = storage
        .create_sub_agent_session(&sub_agent(&parent, "find_auth_flow"))
        .await
        .unwrap();
    let child_id = SessionId::new(child.id.clone());
    let child_turn_id = TurnId::new(unique("turn-cascade-child"));
    storage
        .begin_turn(
            &child_id,
            &child_turn_id,
            &ClientRequestId::new(unique("request-cascade-child")),
            &ResolvedModel::new(None::<String>, "test", "test-model", test_capabilities()),
            &[],
            &Message::text(Role::User, "persist child rows"),
        )
        .await
        .unwrap();
    storage
        .append_assistant_message(
            &child_turn_id,
            &Message::text(Role::Assistant, "child result"),
            None,
        )
        .await
        .unwrap();
    storage
        .finish_turn(
            &child_turn_id,
            &TurnOutcome::Completed {
                final_text: "child result".to_string(),
            },
            None,
        )
        .await
        .unwrap();
    let trace_span_id = unique("span-cascade-child");
    sqlx::query(
        "INSERT INTO trace_spans (
             id, trace_id, session_id, turn_id, kind, name, status,
             started_at, ended_at, attributes
         ) VALUES (
             $1, $2, $3, $4, 'model_call', 'cascade test', 'succeeded',
             CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             '{}'::jsonb
         )",
    )
    .bind(&trace_span_id)
    .bind(unique("trace-cascade-child"))
    .bind(child_id.as_str())
    .bind(child_turn_id.as_str())
    .execute(storage.pool())
    .await
    .unwrap();

    storage.delete_session(&parent).await.unwrap();

    assert!(storage.load_session(&parent).await.unwrap().is_none());
    assert!(
        storage.load_session(&child_id).await.unwrap().is_none(),
        "the sub-agent session must not outlive its parent"
    );
    let remaining_turns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE id = $1")
        .bind(child_turn_id.as_str())
        .fetch_one(storage.pool())
        .await
        .unwrap();
    let remaining_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE turn_id = $1")
            .bind(child_turn_id.as_str())
            .fetch_one(storage.pool())
            .await
            .unwrap();
    let remaining_spans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM trace_spans WHERE id = $1")
        .bind(trace_span_id)
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(
        (remaining_turns, remaining_messages, remaining_spans),
        (0, 0, 0)
    );
}

#[tokio::test]
async fn a_missing_parent_is_reported_rather_than_violating_the_foreign_key() {
    let Some(storage) = storage().await else {
        return;
    };
    let absent = SessionId::new(unique("session-absent"));
    let error = storage
        .create_sub_agent_session(&sub_agent(&absent, "find_auth_flow"))
        .await
        .expect_err("the parent does not exist");
    assert!(
        error.to_string().contains(absent.as_str()),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn reconciliation_finds_terminal_results_and_removes_zero_turn_orphans() {
    let Some(storage) = storage().await else {
        return;
    };
    let parent = root(&storage).await;
    let completed = storage
        .create_sub_agent_session(&sub_agent(&parent, "completed_lookup"))
        .await
        .unwrap();
    let interrupted = storage
        .create_sub_agent_session(&sub_agent(&parent, "interrupted_lookup"))
        .await
        .unwrap();
    let orphan = storage
        .create_sub_agent_session(&sub_agent(&parent, "orphan_lookup"))
        .await
        .unwrap();

    let completed_turn = TurnId::new(unique("turn-completed"));
    storage
        .begin_turn(
            &SessionId::new(completed.id.clone()),
            &completed_turn,
            &ClientRequestId::new(unique("request-completed")),
            &ResolvedModel::new(None::<String>, "test", "test-model", test_capabilities()),
            &[],
            &Message::text(Role::User, "inspect completion"),
        )
        .await
        .unwrap();
    storage
        .append_assistant_message(
            &completed_turn,
            &Message::text(Role::Assistant, "persisted final answer"),
            None,
        )
        .await
        .unwrap();
    storage
        .finish_turn(
            &completed_turn,
            &TurnOutcome::Completed {
                final_text: "persisted final answer".to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let interrupted_turn = TurnId::new(unique("turn-interrupted"));
    storage
        .begin_turn(
            &SessionId::new(interrupted.id.clone()),
            &interrupted_turn,
            &ClientRequestId::new(unique("request-interrupted")),
            &ResolvedModel::new(None::<String>, "test", "test-model", test_capabilities()),
            &[],
            &Message::text(Role::User, "inspect interruption"),
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE turns
         SET status = 'interrupted',
             ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             error_code = 'process_restart',
             error_message = 'process restarted before turn completed'
         WHERE id = $1",
    )
    .bind(interrupted_turn.as_str())
    .execute(storage.pool())
    .await
    .unwrap();

    let reconciliation = storage.reconcile_sub_agent_sessions(&parent).await.unwrap();

    assert_eq!(reconciliation.deleted_orphans.len(), 1);
    assert_eq!(reconciliation.deleted_orphans[0].session_id, orphan.id);
    assert_eq!(reconciliation.deleted_orphans[0].task_name, "orphan_lookup");
    assert!(
        storage
            .load_session(&SessionId::new(orphan.id))
            .await
            .unwrap()
            .is_none()
    );
    storage
        .create_sub_agent_session(&sub_agent(&parent, "orphan_lookup"))
        .await
        .expect("orphan cleanup must make its task name reusable");

    assert_eq!(reconciliation.undelivered.len(), 2);
    let completed_result = reconciliation
        .undelivered
        .iter()
        .find(|result| result.task_name == "completed_lookup")
        .unwrap();
    assert_eq!(completed_result.child_session_id.as_str(), completed.id);
    assert_eq!(completed_result.child_turn_id, completed_turn);
    assert_eq!(completed_result.status, "completed");
    assert_eq!(
        completed_result.final_text.as_deref(),
        Some("persisted final answer")
    );

    let interrupted_result = reconciliation
        .undelivered
        .iter()
        .find(|result| result.task_name == "interrupted_lookup")
        .unwrap();
    assert_eq!(interrupted_result.child_turn_id, interrupted_turn);
    assert_eq!(interrupted_result.status, "interrupted");
    assert_eq!(
        interrupted_result.error_code.as_deref(),
        Some("process_restart")
    );

    let parent_turns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE session_id = $1")
        .bind(parent.as_str())
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(
        parent_turns, 0,
        "reconciliation must not start a parent turn"
    );
}
