use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use openwork_core::{
    API_KEY_ENCRYPTION_KEY_ENV, ClientRequestId, CredentialResolver, ModelInput, OpenWorkCore,
    OpenWorkCoreConfig, PermissionDecision, ProviderInput, RuntimeTurnId, SessionId, SessionInput,
    SessionUpdate, SubAgentHost, SubAgentSpec, ToolCallId, TurnOutcome,
};
use openwork_models::provider::{ApiCredential, ModelTier, ProviderKind, ProviderModel};
use uuid::Uuid;

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

struct FixedCredential;

#[async_trait]
impl CredentialResolver for FixedCredential {
    async fn resolve(&self, _reference: &str) -> Result<ApiCredential, String> {
        Ok(ApiCredential::new("test-credential"))
    }
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
    let children = storage
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

    core.delete_session(&parent_session_id)
        .await
        .expect("cleanup parent and child");
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
            None,
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
