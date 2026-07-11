use std::path::Path;

#[test]
fn providers_source_tree_matches_model_provider_design() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for path in [
        "gateway/mod.rs",
        "gateway/retry.rs",
        "transport/mod.rs",
        "transport/http.rs",
        "transport/sse.rs",
        "adapters/mod.rs",
        "adapters/openai_responses/mod.rs",
        "adapters/openai_responses/request.rs",
        "adapters/openai_responses/response.rs",
        "adapters/openai_responses/stream.rs",
        "adapters/openai_responses/error.rs",
        "adapters/anthropic_messages/mod.rs",
        "adapters/anthropic_messages/request.rs",
        "adapters/anthropic_messages/response.rs",
        "adapters/anthropic_messages/stream.rs",
        "adapters/anthropic_messages/error.rs",
        "adapters/openai_chat/mod.rs",
        "adapters/openai_chat/request.rs",
        "adapters/openai_chat/response.rs",
        "adapters/openai_chat/stream.rs",
        "adapters/openai_chat/dialect/mod.rs",
        "adapters/openai_chat/dialect/deepseek.rs",
        "adapters/openai_chat/dialect/kimi.rs",
        "adapters/openai_chat/dialect/qwen.rs",
        "adapters/openai_chat/dialect/glm.rs",
    ] {
        assert!(
            src.join(path).is_file(),
            "missing providers source file: {path}"
        );
    }
    for legacy in [
        "retry.rs",
        "sse.rs",
        "openai.rs",
        "anthropic.rs",
        "openai_compatible.rs",
    ] {
        assert!(
            !src.join(legacy).exists(),
            "legacy root module remains: {legacy}"
        );
    }
    assert!(
        !src.join("adapters/openai_chat/dialect/standard.rs")
            .exists()
    );
    assert!(!src.join("adapters/openai_chat/error.rs").exists());
}

#[test]
fn adapter_mod_files_only_orchestrate_protocol_codecs() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters");

    let openai_request = std::fs::read_to_string(src.join("openai_responses/request.rs")).unwrap();
    let openai_response =
        std::fs::read_to_string(src.join("openai_responses/response.rs")).unwrap();
    assert!(openai_request.contains("pub(crate) fn encode_request"));
    assert!(openai_response.contains("pub(crate) struct ResponseAccumulator"));

    let anthropic_request =
        std::fs::read_to_string(src.join("anthropic_messages/request.rs")).unwrap();
    let anthropic_response =
        std::fs::read_to_string(src.join("anthropic_messages/response.rs")).unwrap();
    assert!(anthropic_request.contains("pub(crate) fn encode_request"));
    assert!(anthropic_response.contains("pub(crate) struct ResponseAccumulator"));

    let chat_request = std::fs::read_to_string(src.join("openai_chat/request.rs")).unwrap();
    let chat_response = std::fs::read_to_string(src.join("openai_chat/response.rs")).unwrap();
    assert!(chat_request.contains("pub(crate) fn encode_request"));
    assert!(chat_response.contains("pub(crate) struct ResponseAccumulator"));

    for module in [
        "openai_responses/mod.rs",
        "anthropic_messages/mod.rs",
        "openai_chat/mod.rs",
    ] {
        let source = std::fs::read_to_string(src.join(module)).unwrap();
        assert!(
            !source.contains("fn openai_response_content_part")
                && !source.contains("fn anthropic_content_part")
                && !source.contains("fn chat_delta"),
            "adapter orchestration module still owns codec logic: {module}"
        );
    }

    let kimi = std::fs::read_to_string(src.join("openai_chat/dialect/kimi.rs")).unwrap();
    assert!(!kimi.contains("reqwest::Client"));
    assert!(!kimi.contains("consume_sse_response"));
}

#[test]
fn http_transport_lifecycle_is_owned_outside_adapters() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let transport = std::fs::read_to_string(src.join("transport/http.rs")).unwrap();
    let factory = std::fs::read_to_string(src.join("factory.rs")).unwrap();

    assert!(transport.contains("pub struct HttpTransport"));
    assert!(transport.contains("Arc<HttpTransportInner>"));
    assert!(factory.contains("pub struct ProviderFactory"));

    for adapter in [
        "adapters/openai_responses/mod.rs",
        "adapters/anthropic_messages/mod.rs",
        "adapters/openai_chat/mod.rs",
    ] {
        let source = std::fs::read_to_string(src.join(adapter)).unwrap();
        assert!(
            !source.contains("reqwest::Client::new"),
            "adapter creates its own HTTP client instead of receiving shared transport: {adapter}"
        );
    }
}

#[test]
fn provider_streaming_is_pull_based_without_sync_callback_bridge() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let sse = std::fs::read_to_string(src.join("transport/sse.rs")).unwrap();
    let retry = std::fs::read_to_string(src.join("gateway/retry.rs")).unwrap();

    assert!(sse.contains("pub(crate) fn sse_frames"));
    assert!(retry.contains("stream::try_unfold"));
    assert!(!src.join("gateway/client.rs").exists());
    assert!(!src.join("gateway/transport_signal.rs").exists());

    for entry in walk_rs_files(&src) {
        let source = std::fs::read_to_string(&entry).unwrap();
        assert!(!source.contains("std::sync::mpsc"));
        assert!(!source.contains("sync_channel"));
        assert!(!source.contains("EventCallback"));
        assert!(!source.contains("model_stream_from_callback"));
        assert!(!source.contains("ModelTransportSignal"));
    }
}

fn walk_rs_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files
}
