use std::time::Duration;

use openwork_core::{
    API_KEY_ENCRYPTION_KEY_ENV, ClientRequestId, OpenWorkCore, OpenWorkCoreConfig,
    PermissionDecision, ProviderInput, RuntimeTurnId, SessionId, SessionInput, SessionUpdate,
    ToolCallId, TurnOutcome,
};
use openwork_models::provider::{ModelTier, ProviderKind, ProviderModel};
use uuid::Uuid;

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
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
