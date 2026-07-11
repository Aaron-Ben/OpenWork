use openwork_protocol::{
    model::{
        ContentBlock, FinishReason, ModelCallOptions, ModelCapabilities, ModelError,
        ModelErrorCode, ModelEvent, ModelRequest, ModelResponse, ProviderOpaqueBlock, RetryHint,
    },
    provider::{
        ApiCredential, ModelTier, ProviderKind, ProviderModel, ProviderProfile,
        ProviderRuntimeConfig,
    },
};

#[test]
fn model_request_uses_generation_only_contract() {
    let request = ModelRequest {
        model: "deepseek-v4-flash".to_string(),
        messages: Vec::new(),
        temperature: Some(0.0),
        max_output_tokens: Some(1024),
        thinking: None,
        tools: Vec::new(),
    };

    let json = serde_json::to_value(request).unwrap();
    assert_eq!(json["max_output_tokens"], 1024);
    assert!(json.get("embedding").is_none());
    assert!(json.get("stream").is_none());
}

#[test]
fn streaming_contract_has_call_options_and_block_identity() {
    let options = ModelCallOptions::new("attempt-1");
    assert_eq!(options.model_attempt_id, "attempt-1");

    let event = ModelEvent::TextDelta {
        index: 2,
        delta: "hello".to_string(),
    };
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(json["index"], 2);
}

#[test]
fn response_and_error_keep_normalized_metadata() {
    let response = ModelResponse {
        response_id: Some("resp-1".to_string()),
        provider_request_id: Some("req-1".to_string()),
        model: Some("claude-sonnet".to_string()),
        text: "done".to_string(),
        reasoning_text: None,
        tool_calls: Vec::new(),
        provider_opaque_blocks: Vec::new(),
        finish_reason: FinishReason::Stop,
        raw_finish_reason: Some("end_turn".to_string()),
        usage: None,
    };
    assert_eq!(response.finish_reason, FinishReason::Stop);
    assert_eq!(response.provider_request_id.as_deref(), Some("req-1"));
    assert_eq!(response.raw_finish_reason.as_deref(), Some("end_turn"));

    let error = ModelError::http(
        ModelErrorCode::RateLimited,
        429,
        "rate limit",
        Some("rate_limit_reached".to_string()),
        Some("req-1".to_string()),
        RetryHint::AfterMillis(1_500),
    );
    assert_eq!(error.code(), ModelErrorCode::RateLimited);
    assert_eq!(error.retry_hint(), RetryHint::AfterMillis(1_500));
    assert_eq!(error.provider_request_id(), Some("req-1"));
}

#[test]
fn capabilities_do_not_include_embedding_in_v1() {
    let capabilities = ModelCapabilities::generation_defaults();
    assert!(capabilities.chat);
    assert!(capabilities.streaming);
    assert!(!capabilities.tool_calling);
}

#[test]
fn custom_openai_compatible_kind_is_not_a_public_provider_contract() {
    let decoded = serde_json::from_str::<ProviderKind>("\"openai_compatible\"");
    assert!(decoded.is_err());
}

#[test]
fn provider_profile_remains_vendor_explicit_and_round_trips() {
    let profile = ProviderProfile {
        id: "prov-1".to_string(),
        name: "DeepSeek".to_string(),
        base_url: "https://api.deepseek.com".to_string(),
        kind: ProviderKind::Deepseek,
        models: vec![ProviderModel {
            model_id: "deepseek-v4-flash".to_string(),
            display_name: None,
            model_tier: ModelTier::Pro,
            enabled: true,
        }],
        enabled: true,
    };

    let json = serde_json::to_string(&profile).unwrap();
    let decoded: ProviderProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, profile);
    assert!(json.contains("\"modelTier\":\"pro\""));
}

#[test]
fn public_provider_profile_never_serializes_credentials() {
    let runtime = ProviderRuntimeConfig {
        profile: ProviderProfile {
            id: "prov-1".to_string(),
            name: "OpenAI".to_string(),
            base_url: "https://api.openai.com".to_string(),
            kind: ProviderKind::Openai,
            models: Vec::new(),
            enabled: true,
        },
        credential: ApiCredential::new("secret"),
        adapter_options: None,
    };

    let json = serde_json::to_string(&runtime.profile).unwrap();
    assert!(!json.contains("secret"));
    assert!(!json.contains("apiKey"));
    assert!(!format!("{runtime:?}").contains("secret"));
}

#[test]
fn provider_opaque_blocks_round_trip_without_losing_vendor_state() {
    let block = ContentBlock::ProviderOpaque(ProviderOpaqueBlock {
        driver: openwork_protocol::provider::ProviderDriver::AnthropicMessages,
        kind: "thinking".to_string(),
        payload: serde_json::json!({
            "type": "thinking",
            "thinking": "summary",
            "signature": "signed-state"
        }),
    });

    let json = serde_json::to_string(&block).unwrap();
    let decoded: ContentBlock = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, block);
}
