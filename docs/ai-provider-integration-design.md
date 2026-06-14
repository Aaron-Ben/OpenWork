# AI Provider Integration Design

Status: Partially Implemented, Needs Ongoing Refresh
Original date: 2026-06-07
Last reviewed: 2026-06-14

## 1. Purpose

This document describes Anvil's provider integration direction and the current implementation shape.

The long-term design principle is still capability-oriented: application code should ask for model capabilities such as chat, embedding, vision, reasoning, streaming, or tool calling instead of binding directly to one vendor API shape.

The current desktop app is not fully at that target state yet. It has a working provider configuration store, provider adapters, basic streaming, and a chat UI, but it does not yet route requests through `ModelRegistry`.

## 2. Current Code Structure

```text
crates/anvil-core/
  src/ai/types.rs      # Shared AI request/response/message/model types
  src/ai/traits.rs     # Provider traits
  src/ai/error.rs      # Normalized provider errors

crates/anvil-providers/
  src/provider_config.rs     # ProviderKind, ProviderInput, build_provider
  src/store.rs               # ProviderStore persisted as providers.json
  src/presets.rs             # Built-in provider templates for the UI
  src/openai.rs              # OpenAI Responses + embeddings
  src/openai_compatible.rs   # OpenAI-compatible chat adapter
  src/anthropic.rs           # Anthropic Messages
  src/glm.rs                 # GLM wrapper around OpenAI-compatible chat
  src/kimi.rs                # Kimi-specific adapter
  src/deepseek.rs            # DeepSeek wrapper around OpenAI-compatible chat
  src/qwen.rs                # Qwen chat + OpenAI-compatible embeddings
  src/sse.rs                 # Shared SSE parsing helper

crates/anvil-runtime/
  src/registry.rs            # ModelRegistry, capability checks, defaults, fallbacks

apps/desktop/
  src/api/providers.ts       # Thin Tauri invoke/event wrapper
  src/stores/providerStore.ts
  src/components/chat/
  src/components/markdown/
  src-tauri/src/lib.rs       # Tauri command bridge
```

## 3. Current Implementation Snapshot

### 3.1 Provider Configuration

The desktop app stores real user provider configuration through `ProviderStore`.

`ProviderInput` currently contains:

```rust
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: ProviderKind,
    pub models: Vec<String>,
    pub enabled: bool,
    pub extra_body: Option<Map<String, Value>>,
}
```

`ProviderKind` is vendor-oriented, not protocol-oriented:

```rust
pub enum ProviderKind {
    Openai,
    Glm,
    Kimi,
    Deepseek,
    Qwen,
    Anthropic,
    OpenaiCompatible,
}
```

Compatibility aliases exist for older persisted data:

- `openai_responses` deserializes as `Openai`
- `openai_chat` deserializes as `OpenaiCompatible`

### 3.2 Presets

`presets.rs` is not a model registry and not a runtime router. It only provides provider creation templates for the desktop UI.

Current built-in presets:

| Preset | Kind | Default models |
| --- | --- | --- |
| OpenAI | `openai` | `gpt-4.1`, `gpt-4.1-mini` |
| Anthropic | `anthropic` | `claude-sonnet-4-5`, `claude-haiku-4-5` |
| DeepSeek | `deepseek` | `deepseek-chat`, `deepseek-reasoner` |
| Kimi | `kimi` | `kimi-k2.6`, `moonshot-v1-128k` |
| Qwen | `qwen` | `qwen-plus`, `qwen-vl-plus` |
| GLM | `glm` | `glm-4.6`, `glm-4-air` |
| Custom | `openai_compatible` | none |

These defaults are only form defaults. Users can edit the `models` list after creating a provider.

### 3.3 Provider Construction

`build_provider(config)` maps `ProviderKind` to concrete adapter implementations:

| Kind | Adapter |
| --- | --- |
| `openai` | `OpenAiProvider` |
| `glm` | `GlmProvider` |
| `kimi` | `KimiProvider` |
| `deepseek` | `DeepSeekProvider` |
| `qwen` | `QwenProvider` |
| `anthropic` | `AnthropicProvider` |
| `openai_compatible` | `OpenAiCompatibleChatProvider` |

This is intentionally explicit. The app presents provider identity to the user, while adapters can internally share protocol code.

## 4. Core Types and Traits

`anvil-core` owns the shared AI contract. The main request and response types are:

```rust
pub struct GenerateRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub thinking: Option<ThinkingConfig>,
}

pub struct GenerateResponse {
    pub text: String,
    pub reasoning_text: Option<String>,
    pub usage: Option<TokenUsage>,
    pub raw: serde_json::Value,
}

pub enum GenerateStreamEvent {
    TextDelta { delta: String },
    ReasoningDelta { delta: String },
}
```

The provider trait currently keeps streaming on `LlmProvider` rather than a separate `StreamingLlmProvider` trait:

```rust
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError>;

    async fn stream_generate(
        &self,
        req: GenerateRequest,
        on_event: GenerateStreamCallback,
    ) -> Result<GenerateResponse, ProviderError>;
}
```

The default `stream_generate` implementation returns an unsupported-streaming error. Providers that support SSE override it.

## 5. Provider Adapter Notes

### 5.1 OpenAI

`OpenAiProvider` uses:

- `/v1/responses` for generation
- `/v1/embeddings` for embeddings

Current generation supports text, image, and file-style content mapping where implemented in `openai_response_content_part`.

Streaming is implemented by parsing OpenAI Responses SSE events such as:

- `response.output_text.delta`
- `response.reasoning_text.delta`
- `response.reasoning_summary_text.delta`

### 5.2 OpenAI-Compatible Chat

`OpenAiCompatibleChatProvider` is the shared adapter for OpenAI-style `/chat/completions` APIs.

Responsibilities:

- Build `messages` from internal `Message`
- Map text, image URL/base64, video URL/base64, audio URL/base64, and file id content where supported
- Merge `extra_body`
- Parse `choices[0].message.content`
- Parse `choices[0].message.reasoning_content`
- Parse streaming `choices[0].delta.content`
- Parse streaming `choices[0].delta.reasoning_content`

It intentionally rejects provider-specific `thinking` request options. Use a dedicated provider adapter when a vendor exposes non-standard behavior.

### 5.3 Kimi

`KimiProvider` wraps the compatible chat request body but supports provider-specific thinking:

```json
{
  "thinking": {
    "type": "enabled"
  }
}
```

`ThinkingMode::Disabled` maps to `"type": "disabled"`.

Kimi reasoning is normalized to `GenerateResponse.reasoning_text` and streaming `ReasoningDelta`.

### 5.4 DeepSeek

`DeepSeekProvider` currently wraps `OpenAiCompatibleChatProvider`.

DeepSeek-specific options can be passed through `extra_body`, for example reasoning or effort options when the target model supports them.

### 5.5 Qwen / DashScope

`QwenProvider` currently:

- delegates chat generation to `OpenAiCompatibleChatProvider`
- implements text embeddings through the OpenAI-compatible `/embeddings` endpoint
- supports configurable base URL, so regional and workspace endpoints can be used

Do not hardcode a single global Qwen endpoint. DashScope has region-specific and workspace-specific base URLs.

Native DashScope endpoints should be added only when the OpenAI-compatible adapter is insufficient.

### 5.6 GLM

`GlmProvider` currently wraps `OpenAiCompatibleChatProvider`.

It exists as a vendor-level provider so future GLM-specific request or response behavior can be isolated without changing UI configuration semantics.

### 5.7 Anthropic

`AnthropicProvider` uses the Messages API.

Current behavior:

- system messages are merged into Anthropic `system`
- user/assistant messages are converted to Anthropic `messages`
- image URL/base64 content is mapped where supported
- unsupported tool blocks are rejected
- streaming is implemented through Anthropic SSE text and thinking deltas

Anthropic does not currently implement `EmbeddingProvider`.

## 6. Streaming Design

The current desktop streaming path is:

```text
Provider SSE response
  -> provider.stream_generate(...)
  -> GenerateStreamEvent
  -> apps/desktop/src-tauri/src/lib.rs chat_generate_stream
  -> Tauri event: "chat-stream-event"
  -> apps/desktop/src/api/providers.ts listenToChatStream
  -> App.tsx appends deltas to the active assistant message
```

Tauri event payload:

```ts
export type ChatStreamEventName =
  | 'text_delta'
  | 'reasoning_delta'
  | 'done'
  | 'error'

export interface ChatStreamEventPayload {
  requestId: string
  event: ChatStreamEventName
  delta?: string | null
  message?: string | null
}
```

`invoke('chat_generate_stream')` still returns a final `ChatGenerateResponse` so the frontend can reconcile with the complete response after streaming ends.

Current limitations:

- no cancellation / stop generation
- no retry policy
- no streaming usage aggregation
- no tool call delta model
- no backpressure model beyond the Tauri event channel

## 7. Model Registry

`ModelRegistry` exists in `anvil-runtime` and supports:

- provider-qualified model keys, such as `openai:gpt-4.1`
- model capability checks
- defaults by capability alias, such as `chat` or `embedding`
- fallback chains

Example registry shape:

```toml
[[models]]
provider = "openai"
model = "gpt-4.1"
capabilities = ["chat", "vision", "tool_calling", "streaming"]

[[models]]
provider = "qwen"
model = "qwen-plus"
capabilities = ["chat", "tool_calling", "streaming"]

[defaults]
chat = "openai:gpt-4.1"

[[fallbacks.chat]]
primary = "openai:gpt-4.1"
fallbacks = ["anthropic:claude-sonnet-4-5", "deepseek:deepseek-chat"]
```

Current gap: the desktop chat path does not yet use `ModelRegistry`. The UI currently sends `providerId + model` directly to Tauri, and Tauri resolves the provider from `ProviderStore`.

Recommended next integration:

1. Keep `ProviderStore` as the source of credentials and endpoints.
2. Use `ModelRegistry` for capability checks, defaults, and fallback policy.
3. Introduce a runtime resolver that maps `provider:model` to a concrete `ProviderConfig`.
4. Decide whether model names in the UI should be provider-qualified or scoped by active provider.

## 8. Desktop Bridge

`apps/desktop/src-tauri/src/lib.rs` is a bridge layer, not a provider implementation layer.

It exposes Tauri commands:

- `provider_list`
- `provider_presets`
- `provider_create`
- `provider_update`
- `provider_delete`
- `provider_activate`
- `provider_test`
- `chat_generate`
- `chat_generate_stream`

Frontend API wrappers live in `apps/desktop/src/api/providers.ts`. They are thin wrappers around Tauri `invoke` and `listen`, not independent backend logic.

## 9. Error Model

Provider errors are normalized in `ProviderError`:

```rust
pub enum ProviderError {
    Authentication,
    PermissionDenied,
    RateLimited { retry_after_ms: Option<u64> },
    Timeout,
    InvalidRequest { message: String },
    ModelUnavailable { model: String },
    CapabilityUnsupported {
        model: String,
        capability: ModelCapability,
    },
    ProviderServerError { status: u16 },
    Network { message: String },
    Serialization { message: String },
}
```

Adapters should avoid leaking secrets or sensitive user content in errors. HTTP status and summarized provider messages are acceptable; full prompts, API keys, and full raw payloads should not be persisted by default.

## 10. Security

Current state:

- The desktop UI accepts API keys in provider forms.
- Provider configs are persisted through `ProviderStore`.
- This is acceptable only as an early local prototype.

Target state:

- API keys should move to OS keychain or another secret storage layer.
- `providers.json` should store references or metadata, not raw API keys.
- Authorization headers must be redacted in logs.
- Prompts, files, base64 payloads, and raw provider responses should not be logged by default.
- Raw provider responses should be sampled or redacted before persistent storage.
- Request timeouts should be configurable.

## 11. Observability

Every provider request should eventually record:

- provider kind
- model name
- capability
- request id if returned by provider
- latency
- HTTP status
- token usage if available
- retry count
- normalized error category

Do not record by default:

- API key
- full Authorization header
- raw user input
- image/file/base64 payloads

Current gap: structured telemetry is not implemented yet.

## 12. Testing Strategy

Current tests cover:

- request body mapping
- response parsing
- streaming delta extraction
- provider config serialization
- provider store persistence
- registry capability checks
- desktop bridge unit tests

Still needed:

- mocked HTTP integration tests per provider
- live smoke tests gated by environment variables and explicit cost controls
- streaming end-to-end tests through Tauri events
- security tests for credential redaction once secret storage exists

Live tests must not run by default in CI.

## 13. Implementation Status

| Area | Status |
| --- | --- |
| Core request/response types | Implemented |
| `LlmProvider` / `EmbeddingProvider` | Implemented |
| Basic streaming event model | Implemented |
| OpenAI generation | Implemented |
| OpenAI embeddings | Implemented |
| OpenAI-compatible chat | Implemented |
| Kimi provider-specific thinking | Implemented |
| DeepSeek provider wrapper | Implemented |
| Qwen chat + text embeddings | Implemented |
| GLM provider wrapper | Implemented |
| Anthropic generation | Implemented |
| ModelRegistry | Implemented but not wired into desktop chat |
| ProviderStore | Implemented |
| Desktop provider settings | Implemented |
| Desktop streaming chat | Implemented |
| Stop generation | Not implemented |
| Tool calling | Not implemented |
| Credential manager / keychain | Not implemented |
| Telemetry / metrics | Not implemented |
| Mocked HTTP integration tests | Not implemented |

## 14. Recommended Next Steps

1. Add stop generation / cancellation for streaming requests.
2. Move API key storage from raw provider config to OS keychain or a secret manager.
3. Wire `ModelRegistry` into chat routing for capability checks and fallback policy.
4. Add mocked HTTP integration tests for every provider adapter.
5. Add a tool call event model after the non-tool streaming path is stable.
6. Add structured request telemetry with redaction.
7. Revisit provider docs before changing model defaults or adding vendor-specific advanced features.

## 15. Open Questions

- Should `stream_generate` stay on `LlmProvider`, or should streaming be split into a separate trait before tool call deltas are introduced?
- Should desktop chat use provider-qualified model IDs such as `openai:gpt-4.1`, or keep model selection scoped by active provider?
- Should fallback be automatic in chat, explicit per request, or disabled until observability is stronger?
- Should `ProviderKind::OpenaiCompatible` remain a first-class kind, or should custom providers become typed by protocol plus declared capabilities?
- Which embedding provider should be default when the active chat provider does not expose embeddings?
- What is the minimum acceptable credential storage model before broader distribution?
