use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use openwork_core::{
    API_KEY_ENCRYPTION_KEY_ENV, ClientRequestId, CredentialResolver, ModelCapabilities, ModelInput,
    OpenWorkCore, OpenWorkCoreConfig, PermissionDecision, ProviderInput, ResolvedModel,
    RuntimeTurnId, SessionId, SessionInput, SessionStorage, SessionUpdate, SessionUpdateEnvelope,
    SubAgentHost, SubAgentSessionInput, SubAgentSpec, ToolCallId, TurnOutcome,
};
use openwork_models::model::{Message, Role};
use openwork_models::provider::{ApiCredential, ModelTier, ProviderKind, ProviderModel};
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

struct FixedCredential;

#[async_trait]
impl CredentialResolver for FixedCredential {
    async fn resolve(&self, _reference: &str) -> Result<ApiCredential, String> {
        Ok(ApiCredential::new("test-credential"))
    }
}

async fn wait_for_turn_finished(
    updates: &mut tokio::sync::broadcast::Receiver<SessionUpdateEnvelope>,
    session_id: &SessionId,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let update = updates.recv().await.expect("runtime update");
            if update.session_id == *session_id
                && matches!(update.update, SessionUpdate::TurnFinished { .. })
            {
                return;
            }
        }
    })
    .await
    .expect("parent turn must finish");
}

#[tokio::test]
async fn production_host_persists_and_starts_an_idle_explorer_session() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let storage = Arc::new(
        openwork_core::PostgresStorage::connect(Some(&database_url))
            .await
            .expect("storage"),
    );
    let model_id = unique("model-sub-agent-host");
    let model_name = unique("sub-agent-host-model");
    storage
        .upsert_model(&ModelInput {
            id: model_id.clone(),
            display_name: "Sub-agent host test".to_string(),
            provider_kind: "deepseek".to_string(),
            model_name: model_name.clone(),
            base_url: format!("https://example.invalid/{model_name}"),
            credential_ref: Some("test:credential".to_string()),
            enabled: true,
            capabilities: test_capabilities(),
            config: serde_json::json!({}),
        })
        .await
        .expect("model");
    let core = OpenWorkCore::from_storage_with_credentials(
        Arc::clone(&storage),
        Arc::new(FixedCredential),
    )
    .await
    .expect("core");
    let parent_session_id = SessionId::new(unique("session-sub-agent-parent"));
    core.create_session(&SessionInput {
        id: parent_session_id.clone(),
        title: Some("Sub-agent parent".to_string()),
        working_directory: std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned(),
        default_model_id: Some(model_id),
    })
    .await
    .expect("parent session");
    let child_session_id = SessionId::new(unique("session-sub-agent-child"));

    SubAgentHost::start_sub_agent(
        core.as_ref(),
        SubAgentSpec {
            session_id: child_session_id.clone(),
            parent_session_id: parent_session_id.clone(),
            task_name: "inspect_runtime".to_string(),
            agent_role: "explorer".to_string(),
            spawn_span_id: Some("span-parent-tool".to_string()),
        },
    )
    .await
    .expect("production sub-agent host");

    let handle = SubAgentHost::session_handle(core.as_ref(), &child_session_id)
        .await
        .expect("same Core session map");
    assert!(matches!(
        handle.snapshot().await.expect("snapshot").runtime,
        openwork_core::session::SessionRuntimeSnapshot::Idle
    ));
    let children = core
        .list_sub_agent_sessions(&parent_session_id)
        .await
        .expect("children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, child_session_id.to_string());
    assert_eq!(children[0].task_name.as_deref(), Some("inspect_runtime"));
    assert_eq!(children[0].agent_role.as_deref(), Some("explorer"));
    assert_eq!(
        children[0].spawn_span_id.as_deref(),
        Some("span-parent-tool")
    );

    let child_context = core
        .inspect_context_window(&child_session_id)
        .await
        .expect("inspect child context");
    let child_tool_names = child_context
        .tool_surface
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(child_tool_names, ["read", "grep", "glob", "list", "bash"]);

    let retained_child_handle = handle.clone();
    core.delete_session(&parent_session_id)
        .await
        .expect("cleanup parent and child");
    assert!(matches!(
        retained_child_handle.snapshot().await,
        Err(openwork_core::SessionError::ActorStopped)
    ));
    assert!(
        SubAgentHost::session_handle(core.as_ref(), &child_session_id)
            .await
            .is_err(),
        "deleting a parent must remove its child handle from the Core map"
    );
}

#[tokio::test]
async fn parent_next_turn_reconciles_restart_results_exactly_once() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let storage = Arc::new(
        openwork_core::PostgresStorage::connect(Some(&database_url))
            .await
            .expect("storage"),
    );
    storage.migrate().await.expect("migrations");
    let model_id = unique("model-reconciliation");
    storage
        .upsert_model(&ModelInput {
            id: model_id.clone(),
            display_name: "Reconciliation test".to_string(),
            provider_kind: "deepseek".to_string(),
            model_name: unique("reconciliation-model"),
            base_url: "http://127.0.0.1:9".to_string(),
            credential_ref: Some("test:credential".to_string()),
            enabled: true,
            capabilities: test_capabilities(),
            config: serde_json::json!({}),
        })
        .await
        .expect("model");
    let parent_session_id = SessionId::new(unique("session-reconciliation-parent"));
    storage
        .create_session(&SessionInput {
            id: parent_session_id.clone(),
            title: Some("Reconciliation parent".to_string()),
            working_directory: std::env::current_dir()
                .expect("cwd")
                .to_string_lossy()
                .into_owned(),
            default_model_id: Some(model_id),
        })
        .await
        .expect("parent");

    let completed_session_id = SessionId::new(unique("session-reconciliation-completed"));
    let completed = storage
        .create_sub_agent_session(&SubAgentSessionInput {
            id: completed_session_id.clone(),
            parent_session_id: parent_session_id.clone(),
            task_name: "completed_lookup".to_string(),
            agent_role: "explorer".to_string(),
            working_directory: std::env::current_dir()
                .expect("cwd")
                .to_string_lossy()
                .into_owned(),
            default_model_id: None,
            spawn_span_id: None,
        })
        .await
        .expect("completed child");
    let completed_turn_id = RuntimeTurnId::new(unique("turn-reconciliation-completed"));
    storage
        .begin_turn(
            &SessionId::new(completed.id),
            &completed_turn_id,
            &ClientRequestId::new(unique("request-reconciliation-completed")),
            &ResolvedModel::new(None::<String>, "test", "test-model", test_capabilities()),
            &[],
            &Message::text(Role::User, "inspect completion"),
        )
        .await
        .expect("completed turn start");
    storage
        .append_assistant_message(
            &completed_turn_id,
            &Message::text(Role::Assistant, "persisted completed result"),
            None,
        )
        .await
        .expect("completed assistant");
    storage
        .finish_turn(
            &completed_turn_id,
            &TurnOutcome::Completed {
                final_text: "persisted completed result".to_string(),
            },
            None,
        )
        .await
        .expect("completed turn finish");

    let interrupted_session_id = SessionId::new(unique("session-reconciliation-interrupted"));
    let interrupted = storage
        .create_sub_agent_session(&SubAgentSessionInput {
            id: interrupted_session_id.clone(),
            parent_session_id: parent_session_id.clone(),
            task_name: "interrupted_lookup".to_string(),
            agent_role: "explorer".to_string(),
            working_directory: std::env::current_dir()
                .expect("cwd")
                .to_string_lossy()
                .into_owned(),
            default_model_id: None,
            spawn_span_id: None,
        })
        .await
        .expect("interrupted child");
    let interrupted_turn_id = RuntimeTurnId::new(unique("turn-reconciliation-interrupted"));
    storage
        .begin_turn(
            &SessionId::new(interrupted.id),
            &interrupted_turn_id,
            &ClientRequestId::new(unique("request-reconciliation-interrupted")),
            &ResolvedModel::new(None::<String>, "test", "test-model", test_capabilities()),
            &[],
            &Message::text(Role::User, "inspect interruption"),
        )
        .await
        .expect("interrupted turn start");

    let core = OpenWorkCore::from_storage_with_credentials(
        Arc::clone(&storage),
        Arc::new(FixedCredential),
    )
    .await
    .expect("core restart");
    let orphan_session_id = SessionId::new(unique("session-reconciliation-orphan"));
    SubAgentHost::start_sub_agent(
        core.as_ref(),
        SubAgentSpec {
            session_id: orphan_session_id.clone(),
            parent_session_id: parent_session_id.clone(),
            task_name: "orphan_lookup".to_string(),
            agent_role: "explorer".to_string(),
            spawn_span_id: None,
        },
    )
    .await
    .expect("orphan child session");
    let orphan_handle = SubAgentHost::session_handle(core.as_ref(), &orphan_session_id)
        .await
        .expect("orphan handle");
    let parent_turns_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE session_id = $1")
            .bind(parent_session_id.as_str())
            .fetch_one(storage.pool())
            .await
            .expect("parent turn count");
    assert_eq!(
        parent_turns_before, 0,
        "restart alone must not start a parent turn"
    );

    let mut updates = core.subscribe_updates();
    core.start_turn(
        &parent_session_id,
        ClientRequestId::new(unique("request-reconciliation-parent-first")),
        vec![openwork_core::UserInput::text("continue after restart")],
    )
    .await
    .expect("first parent turn");
    assert!(matches!(
        orphan_handle.snapshot().await,
        Err(openwork_core::SessionError::ActorStopped)
    ));
    assert!(
        SubAgentHost::session_handle(core.as_ref(), &orphan_session_id)
            .await
            .is_err()
    );
    assert!(
        storage
            .load_session(&orphan_session_id)
            .await
            .expect("load orphan after reconciliation")
            .is_none()
    );
    wait_for_turn_finished(&mut updates, &parent_session_id).await;

    core.start_turn(
        &parent_session_id,
        ClientRequestId::new(unique("request-reconciliation-parent-second")),
        vec![openwork_core::UserInput::text("continue again")],
    )
    .await
    .expect("second parent turn");
    wait_for_turn_finished(&mut updates, &parent_session_id).await;

    for message_id in [
        format!(
            "agent-msg:{}:{}:final_answer",
            completed_session_id, completed_turn_id
        ),
        format!(
            "agent-msg:{}:{}:interrupted",
            interrupted_session_id, interrupted_turn_id
        ),
    ] {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
            .bind(message_id)
            .fetch_one(storage.pool())
            .await
            .expect("agent message count");
        assert_eq!(count, 1, "reconciliation result must be persisted once");
    }

    core.delete_session(&parent_session_id)
        .await
        .expect("cleanup parent");
}

#[tokio::test]
#[ignore = "requires a stored DeepSeek credential and network access"]
async fn stored_deepseek_v4_flash_completes_a_real_turn() {
    let database_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    std::env::var(API_KEY_ENCRYPTION_KEY_ENV).expect(API_KEY_ENCRYPTION_KEY_ENV);
    let core = OpenWorkCore::bootstrap(OpenWorkCoreConfig {
        database_url: Some(database_url),
        ..OpenWorkCoreConfig::default()
    })
    .await
    .unwrap();
    let provider = core
        .list_providers()
        .await
        .unwrap()
        .providers
        .into_iter()
        .rev()
        .find(|provider| {
            provider.kind == ProviderKind::Deepseek
                && provider.enabled
                && provider
                    .models
                    .iter()
                    .any(|model| model.model_id == "deepseek-v4-flash" && model.enabled)
        })
        .expect("stored enabled deepseek-v4-flash provider");
    let session_id = SessionId::new(unique("session-deepseek-stored-live"));
    core.create_session(&SessionInput {
        id: session_id.clone(),
        title: Some("Stored DeepSeek smoke test".to_string()),
        working_directory: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        default_model_id: Some(format!("model:{}:deepseek-v4-flash", provider.id)),
    })
    .await
    .unwrap();

    let run_result: Result<TurnOutcome, String> = async {
        let mut updates = core.subscribe_updates();
        core.start_turn(
            &session_id,
            ClientRequestId::new(unique("request-deepseek-stored-live")),
            vec![openwork_core::UserInput::text(
                "Reply with exactly: OPENWORK_FRONTEND_OK. Do not call tools.",
            )],
        )
        .await
        .map_err(|error| error.to_string())?;

        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let envelope = updates.recv().await.map_err(|error| error.to_string())?;
                match envelope.update {
                    SessionUpdate::PermissionRequested { request } => {
                        core.resolve_permission(
                            &session_id,
                            RuntimeTurnId::new(envelope.turn_id.to_string()),
                            ToolCallId::new(request.tool_call_id.to_string()),
                            PermissionDecision::Deny,
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    }
                    SessionUpdate::TurnFinished { outcome } => break Ok(outcome),
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| "DeepSeek stored-provider turn timed out".to_string())?
    }
    .await;

    core.delete_session(&session_id).await.unwrap();
    let outcome = run_result.expect("stored DeepSeek turn completes");
    match outcome {
        TurnOutcome::Completed { final_text } => {
            assert!(final_text.contains("OPENWORK_FRONTEND_OK"));
        }
        other => panic!("unexpected stored DeepSeek outcome: {other:?}"),
    }
}

#[tokio::test]
async fn bootstrapped_core_persists_a_provider_and_creates_a_session_from_its_model() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    if std::env::var(API_KEY_ENCRYPTION_KEY_ENV).is_err() {
        return;
    }

    let core = OpenWorkCore::bootstrap(OpenWorkCoreConfig {
        database_url: Some(database_url),
        ..OpenWorkCoreConfig::default()
    })
    .await
    .unwrap();
    let model_name = unique("provider-host-model");
    let provider = core
        .create_provider(ProviderInput {
            name: unique("provider-host-test"),
            base_url: format!("https://example.invalid/{model_name}"),
            api_key: "test-secret-never-logged".to_string(),
            kind: ProviderKind::Deepseek,
            models: vec![ProviderModel {
                model_id: model_name.clone(),
                display_name: None,
                model_tier: ModelTier::Lite,
                enabled: true,
                capabilities: Some(test_capabilities()),
            }],
            enabled: true,
            extra_body: None,
        })
        .await
        .unwrap();

    let index = core.list_providers().await.unwrap();
    let listed = index
        .providers
        .iter()
        .find(|candidate| candidate.id == provider.id)
        .unwrap();
    assert_eq!(listed.models[0].model_tier, ModelTier::Lite);

    let session_id = SessionId::new(unique("session-host-test"));
    let model_id = format!("model:{}:{model_name}", provider.id);
    let session = core
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Core host flow".to_string()),
            working_directory: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            default_model_id: Some(model_id.clone()),
        })
        .await
        .unwrap();
    assert_eq!(session.default_model_id.as_deref(), Some(model_id.as_str()));

    let context = core.inspect_context_window(&session_id).await.unwrap();
    assert_eq!(context.schema_version, 2);
    assert_eq!(context.budget.auto_compaction_threshold_percent, 85);
    assert_eq!(context.session_id, session_id.to_string());
    assert_eq!(context.resolved_model_name, model_name);
    assert_eq!(context.system_context[0].source_key, "core/agent-system");
    assert!(context.conversation.is_empty());
    assert!(!context.tool_surface.is_empty());
    assert_eq!(
        context.budget.estimated_input_tokens,
        context
            .budget
            .system_context_tokens
            .saturating_add(context.budget.conversation_tokens)
            .saturating_add(context.budget.tool_surface_tokens),
    );

    core.delete_session(&session_id).await.unwrap();
    core.delete_provider(&provider.id).await.unwrap();
}
