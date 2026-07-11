# OpenWork Model Provider 集成设计

Last reviewed: 2026-07-11

> Status: core contract and primary provider paths implemented; infrastructure consolidation and full model-specific coverage remain. 本文是模型厂商集成的专题设计，受 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md) 约束。当前代码已完成 streaming-first Port、结构化错误、SSE framing/背压/取消、Secret/Profile 查询隔离、共享 HTTP Transport 生命周期、OpenAI Responses/Anthropic Tool 流式主链、Anthropic opaque thinking 往返、主要 Dialect 精确错误码和 Retry-After 解析。全库唯一 PostgreSQL composition root、有序 output block、模型级 Thinking 参数和 M7 Attempt Projection 仍未完成。

### 2026-07-11 implementation checkpoint

- `openwork-protocol::model` 已成为协议类型所有者；旧 `src/ai` 已物理删除，类型拆入 `message/request/response/event/error/port`。
- `ModelPort` 只保留 `invoke -> ModelStream`，请求中不再含 `stream`；事件带 block index/id。
- `ModelError` 已合并为单一结构化表达，包含 phase、delivery、retry 和厂商诊断字段。
- Provider Stream 使用有界桥接并在 Drop 时取消生产任务；Retry 受总 deadline、attempt budget 和 semantic-output gate 约束。
- SSE transport 只负责 framing；`[DONE]` 和厂商终态由对应 Adapter 解释，不再累积 raw event 数组。
- `ProviderProfile` 与不可序列化的 `ProviderRuntimeConfig/ApiCredential` 已分离；普通列表/详情 SQL 不读取 `api_key`。
- Provider Registry 已由 `PostgresPersistence` 持有 Pool/Migration，`PostgresProviderRepository::new(PgPool)` 不再自行连接或迁移；列表使用固定两次批量查询。但 Session Store 仍有独立数据库生命周期，全库唯一 composition root 尚未完成。
- 三个 crate 已建立 `domain/model/provider`、`gateway/transport/adapters`、`postgres/migrations/provider_registry` 物理边界；三类 Adapter 的 request/response/stream codec 已从 `mod.rs` 拆出。
- Desktop Composition Root 只创建一个 `ProviderFactory`；Factory 持有 `HttpTransport`，其 Clone 通过 `Arc<HttpTransportInner>` 共享同一个 `reqwest::Client` 和连接池。Chat Runtime 与 Provider Test 使用同一 Factory 生命周期，Adapter 构造函数不再自行创建 Client。
- Kimi 默认 endpoint 已对齐 `/v1/chat/completions`，并使用 `max_completion_tokens`；Kimi、Qwen、DeepSeek、GLM 流式请求显式获取 usage。
- Kimi 已收敛为 OpenAI Chat 的薄 Dialect，不再复制 HTTP Client、headers、SSE 消费和响应累计；厂商文件只保留构造配置、错误码分类与方言 fixture。
- OpenAI Responses 与 Anthropic Messages 已支持函数工具声明、工具历史和流式参数拼接；Anthropic `thinking.signature` 与 `redacted_thinking` 使用同 Driver opaque block 原样回传。
- OpenAI、Anthropic、DeepSeek、Kimi、Qwen、GLM 已建立精确错误分类入口；未知错误才回退到公共 HTTP/文本分类。
- `lite/plus/pro` 仍只是用户分类，不参与自动路由；未新增 capability、price、fallback 或 attempt 表。

## 1. 决策摘要

本专题冻结以下方向：

1. `openwork-protocol` 只定义厂商无关的模型语义、Provider 配置领域类型和 Port，不包含 HTTP、SSE、SQLx、Tauri DTO 或厂商 DTO。
2. `openwork-providers` 只实现模型协议适配和 Transport Reliability；内部按 `gateway / transport / adapters` 分层，不再把所有共享函数堆在 crate 根目录。
3. `openwork-persistence` 负责 PostgreSQL 连接、迁移、Provider Registry Repository，以及后续由 Recorded Event 投影得到的 Model Attempt 查询表；它不决定重试。
4. “厂商身份”和“线协议/Driver”分开。DeepSeek、Kimi、Qwen、GLM 使用 OpenAI-compatible envelope，不代表它们具有相同错误码、Thinking、Usage 和流式终态。
5. `ModelPort` 采用单一 streaming-first 调用合同；`stream` 不再同时存在于请求字段和两个方法中。
6. `ModelEvent` 只表达规范化模型输出；Transport Attempt、Retry 和 Telemetry 使用独立事件类型，不伪装成模型输出。
7. Provider 层只有在尚未向上层提交任何语义输出时才允许透明重试。
8. Provider 配置列表、Tauri 返回值和普通日志不得携带 API Key。V1 仍可暂存 PostgreSQL 明文凭据，但必须用非序列化运行时类型隔离。
9. 不默认持久化完整原始响应、SSE event 数组或错误 body；只保存经过白名单、截断和脱敏的诊断字段。
10. 本次只重构现有模型调用能力，不增加 Embedding、自动路由、静默 Fallback、价格同步或动态模型发现。

## 2. 要解决的问题

### 2.1 当前代码已经正确的部分

- `openwork-providers` 已不再拥有 SQLx Repository。
- `openwork-persistence` 已提供 `PostgresProviderRepository`。
- `provider_models` 已从不可约束的 `models_json` 拆成 PostgreSQL 子表。
- 已有 `ModelErrorCode`、`RetryHint` 和流式输出后禁止重试的基础测试。
- OpenAI Responses、Anthropic Messages 和 OpenAI-compatible Chat 已有独立入口。

这些实现应保留并渐进迁移，不重新从零构建。

### 2.2 重构前实现与蓝图冲突的部分

| 问题 | 重构前证据 | 后果 |
| --- | --- | --- |
| `model` 不是真正所有者 | `openwork-protocol/src/model/mod.rs` 只 re-export `ai` | 新旧入口长期并存，协议边界名存实亡 |
| 请求存在非法组合 | `ModelRequest.stream` 与 `generate/stream_generate` 同时存在 | 调用方法和字段可能互相矛盾 |
| 流式回调没有背压 | `ModelEventCallback` + Runtime unbounded channel | 长输出可无界积压，取消和错误传递困难 |
| Event 缺少 block identity | `TextDelta/ReasoningDelta` 没有 `index/id` | 多 block 输出无法可靠重放或归并 |
| 错误模型重复 | 旧枚举 variant 与 `Http(HttpModelError)` 并存 | 同一种失败存在两种表达和两套 retry 逻辑 |
| 厂商与协议混为一体 | `ProviderKind` 同时包含 OpenAI、DeepSeek 和 OpenAICompatible | Factory、持久化值和 Adapter 复用关系不清晰 |
| 厂商错误仍是中央猜测 | `openwork-providers/src/adapters/error.rs` 曾主要依赖 HTTP status 和英文子串 | Kimi/Qwen/GLM 的 429 可能把欠费误判为限流；现已加入主要精确码，仍需继续补官方 fixture |
| SSE framing 知道厂商终态 | 通用 `sse.rs` 判断 `message_stop/response.completed` | Transport 层反向拥有 Adapter 语义 |
| SSE parser 不满足完整 framing | 每个网络 chunk 独立 `from_utf8_lossy`，多 `data:` 行逐行解析 | UTF-8 跨 chunk 或 multiline event 可能损坏 |
| OpenAI-compatible 扩展可覆盖稳定字段 | `extra_body` 在基础 body 后覆盖任意 key | 可绕过 model、messages、tools、stream 等统一合同 |
| 原始流被完整保留 | 流式响应把所有 raw event 放入 `provider_metadata` | 内存增长、敏感信息泄漏和持久化误用风险 |
| Adapter 能力不对齐 | OpenAI Responses 和 Anthropic 当前拒绝 tools/thinking | “支持某厂商”不等于可运行 Coding Agent 主链 |
| Repository 同时返回 Secret 与 UI 数据 | `ProviderConfig` 可序列化且包含 `api_key` | Provider 列表可能把密钥带到不必要的边界 |
| Provider 列表存在 N+1 查询 | 每条 Provider 分别调用 `models_for` | Provider 增长后产生不必要的数据库往返 |
| Repository 自己连接并迁移 | `PostgresProviderRepository::connect` 持有完整 `Database` | 后续 Journal、Projection、Artifact Repository 会重复基础设施生命周期 |
| Attempt 记录不是可靠事实 | Runtime 通过 detached task best-effort 写 `llm_events` | 崩溃时无法审计某次 HTTP attempt、重试和计费风险 |

## 3. 范围与非目标

### 3.1 本专题范围

- OpenAI Responses API。
- Anthropic Messages API。
- OpenAI-compatible Chat Completions 协议族。
- DeepSeek、Kimi、Qwen、GLM 的 Dialect 差异。
- 文本、推理展示、Tool Calling、流式响应和 Usage。
- HTTP/SSE 错误归一化、Transport Retry 和 Attempt Trace 边界。
- Provider 配置及模型列表的 PostgreSQL Repository。
- 三个 crate 的内部目标结构和迁移路线。

### 3.2 本专题不做

- Embedding、向量数据库和 RAG。
- 自动模型路由、Fallback 和负载均衡。
- Provider SDK Plugin 系统。
- 在线价格表、账单结算和完整成本平台。
- 模型列表自动同步或 Capability 远程探测。
- SecretStore/Keychain 实现；只先修正 Secret 在进程内和 DTO 中的边界。
- SQLite 或多数据库兼容层。
- Recorded Event 总协议；本文只定义 Model Attempt 需要提供的事实，最终事件名由 Core 专题冻结。

## 4. 术语

| 术语 | 含义 |
| --- | --- |
| Provider Profile | 用户配置的一条模型服务连接资料，不含可公开返回的密钥 |
| Provider Driver | 决定使用哪种 wire protocol 和 Dialect 的稳定类型 |
| Model Attempt | Core 发起的一次语义模型调用，拥有一个 `ModelAttemptId` |
| Transport Attempt | 同一 Model Attempt 内的一次实际 HTTP 尝试，以 `attempt_no` 区分 |
| Provider Response ID | 厂商响应对象 ID，例如 message/response ID |
| Provider Request ID | 厂商用于诊断 HTTP 请求的 request ID |
| Semantic Output | 已提交给 Core 的文本、推理、Tool Call ID/name 或参数增量 |
| Adapter | 厂商/协议 DTO 与 OpenWork Model Contract 的双向转换器 |
| Transport | 只负责 HTTP 和 SSE framing，不理解厂商模型语义 |
| Dialect | 同属 OpenAI Chat envelope 下某个厂商的参数、错误码和流式差异 |

`ModelAttemptId` 标识一次 Core 语义调用；Provider 内部透明重试不创建新的 Model Attempt，只增加 `transport_attempt_no`。这样既能保持同一请求的因果关系，又能分别记录厂商 request ID。

## 5. 总体调用链

```text
openwork-core
  -> persist ModelAttempt requested
  -> openwork-protocol::model::ModelPort.invoke(request, options)
  -> openwork-providers::gateway::RetryingModelClient
       -> emit TransportAttempt signal
       -> selected Adapter
            -> encode vendor request
            -> transport::HttpTransport
            -> transport::SseFramer
            -> decode vendor event/error
       -> normalize ModelEvent / ModelError
  -> Core records accepted output and attempt outcome
  -> openwork-persistence journal/projector
  -> model_attempts query projection (Recorded Event 稳定后)
```

Provider Registry 是另一条独立链路：

```text
openwork-app Provider command/query
  -> openwork-protocol::provider::ProviderRepository
  -> openwork-persistence::PostgresProviderRepository
  -> providers + provider_models

openwork-app Composition Root
  -> ProviderRepository.load_runtime(provider_id)
  -> openwork-providers::factory
  -> Arc<dyn ModelPort>
```

Core 不读取 PostgreSQL Provider Row，也不根据 `ProviderKind` 构造 Adapter。Composition Root 先完成选择和注入。

## 6. `openwork-protocol` 目标结构

```text
crates/openwork-protocol/src/
├── lib.rs
├── domain/
│   ├── mod.rs
│   └── ids.rs                    # ProviderId、ModelAttemptId 等
├── model/
│   ├── mod.rs                    # 最小稳定 re-export
│   ├── message.rs                # Role、Message、ContentBlock
│   ├── request.rs                # ModelRequest、GenerationOptions、ToolDefinition
│   ├── response.rs               # ModelResponse、OutputBlock、FinishReason、Usage
│   ├── event.rs                  # ModelEvent、block identity
│   ├── error.rs                  # ModelError、phase、delivery、retry directive
│   └── port.rs                   # ModelPort、ModelStream、ModelCallOptions
├── provider/
│   ├── mod.rs
│   ├── driver.rs                 # ProviderDriver、OpenAiChatDialect
│   ├── profile.rs                # Profile、RuntimeConfig、Credential wrapper
│   └── repository.rs             # ProviderRepository Port/Error
```

目录按领域组织；`request/response/event` 是 Model 领域内部稳定子边界，不创建额外 crate。

### 6.1 Model Request

目标结构：

```rust
pub struct ModelRequest {
    pub model: ModelName,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: ToolChoice,
    pub generation: GenerationOptions,
}

pub struct GenerationOptions {
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub reasoning: ReasoningIntent,
}
```

约束：

- `stream` 不属于请求语义，移入调用方式；目标 Port 永远返回 Stream。
- Provider ID、Base URL、API Key、Retry Policy 不进入 `ModelRequest`。
- `system` 是稳定语义，但具体 Adapter 可映射为顶层字段或 message role。
- `temperature` 和 reasoning 只是 capability-gated intent；Adapter 不得静默忽略。
- `extra_body` 不进入 Model Request。
- Tool 参数 Schema 是 JSON Schema；最终 Tool Call 参数使用 `serde_json::Value`，流式阶段仍使用 JSON 字符串 delta。

### 6.2 Message 与 reasoning 往返

纯 `ThinkingBlock { thinking: String }` 只能用于 UI 展示，无法表达 Anthropic signed/redacted thinking 或 Kimi preserved thinking。目标结构区分：

```rust
pub enum ModelContentBlock {
    Text(TextBlock),
    Data(DataBlock),
    Reasoning(ReasoningBlock),
    ToolCall(ToolCallBlock),
    ToolResult(ToolResultBlock),
    ProviderOpaque(ProviderOpaqueBlock),
}

pub struct ProviderOpaqueBlock {
    pub driver: ProviderDriver,
    pub kind: String,
    pub payload: serde_json::Value,
}
```

规则：

- `ReasoningBlock` 只保存允许展示或 Trace 的规范化文本/摘要。
- `ProviderOpaqueBlock` 只允许原 Adapter 生成，并只回传给同一 Driver；Core 不解析、不编辑。
- Opaque payload 必须有大小上限、来源标记和序列化测试，不进入 UI 默认响应。
- 不允许用户输入伪造 Opaque block。
- 不做静默跨 Provider Fallback，因此无需把某厂商 opaque state 翻译给另一厂商。

### 6.3 Streaming-first Model Port

目标合同：

```rust
pub type ModelStream = Pin<
    Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send + 'static>
>;

#[async_trait]
pub trait ModelPort: Send + Sync {
    async fn invoke(
        &self,
        request: ModelRequest,
        options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError>;
}
```

`ModelCallOptions` 至少包含：

```text
model_attempt_id
total_timeout
max_transport_attempts
```

取消由 Core 停止轮询并 drop Stream/Future 触发；Provider 的 HTTP 请求和 retry sleep 必须随 Future drop 取消。若后续需要显式 Cancellation Port，再由 Runtime Contract 专题加入，不能让 Protocol 直接依赖 Tauri。

该设计替代：

- `generate` 与 `stream_generate` 两套方法。
- `ModelRequest.stream` 布尔值。
- `FnMut` Callback、`Mutex` 和 unbounded channel 桥接。

Buffered 调用只是上层收集 Stream 的便利函数，不是第二套 Provider API。

### 6.4 Model Event

目标事件必须携带 block identity：

```rust
pub enum ModelEvent {
    ResponseStarted {
        response_id: Option<String>,
        provider_request_id: Option<String>,
        model: Option<String>,
    },
    OutputBlockStarted {
        index: u32,
        id: Option<String>,
        kind: OutputBlockKind,
    },
    TextDelta {
        index: u32,
        delta: String,
    },
    ReasoningDelta {
        index: u32,
        delta: String,
    },
    ToolCallStarted {
        index: u32,
        call_id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        index: u32,
        call_id: String,
        delta: String,
    },
    OutputBlockCompleted {
        index: u32,
    },
    UsageUpdated {
        usage: TokenUsage,
    },
    ResponseCompleted {
        response: ModelResponse,
    },
}
```

不把以下内容放入 `ModelEvent`：

- `TransportAttemptStarted`
- `RetryScheduled`
- `TransportAttemptFailed`
- Recorded Event sequence
- UI `done/error`

这些内容使用单独的 `ModelTransportSignal`，并由 Core/App 映射到 Recorded Event 或 Telemetry。Provider 不直接写数据库。

### 6.5 Model Response

```rust
pub struct ModelResponse {
    pub response_id: Option<String>,
    pub provider_request_id: Option<String>,
    pub model: Option<String>,
    pub output: Vec<ModelOutputBlock>,
    pub finish_reason: FinishReason,
    pub raw_finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
}
```

规则：

- `output` 保持块顺序，不再让 `text/reasoning/tool_calls` 三个字段丢失交错关系。
- 可在 App 层提供 `text()`、`reasoning_text()`、`tool_calls()` 便利读取器。
- 成功响应也保留 `provider_request_id`。
- `TokenUsage` 字段全部可空；流式 Adapter 对外提供“当前累计快照”。
- `raw_finish_reason` 必须保留；`insufficient_system_resource`、`sensitive`、partial/failed 不得映射为 `Stop`。
- 删除无限制 `provider_metadata: Value`。需要的诊断值进入明确字段或受限 Trace metadata。

### 6.6 Model Error

目标错误只保留一种结构化表达：

```rust
pub struct ModelError {
    pub kind: ModelErrorKind,
    pub phase: ModelFailurePhase,
    pub delivery: DeliveryState,
    pub retry: RetryDirective,
    pub message: String,
    pub http_status: Option<u16>,
    pub provider_code: Option<String>,
    pub provider_request_id: Option<String>,
}
```

`ModelFailurePhase`：

```text
request_encode | connect | response_headers | response_body |
stream_decode | response_decode | cancelled
```

`DeliveryState`：

```text
not_sent | possibly_sent | accepted_no_semantic_output | semantic_output_emitted
```

`ModelErrorKind` 至少包含：

```text
authentication | permission_denied | invalid_request | model_not_found |
capability_unsupported | rate_limited | quota_exhausted | overloaded |
timeout | network | server_error | content_filtered | protocol_error |
cancelled | unknown
```

错误正文必须是安全、截断后的诊断信息。Library 使用 `thiserror`，不在 Adapter 内 `unwrap` 或丢弃未知厂商错误码。

### 6.7 Provider 配置类型

当前 `ProviderKind` 同时表达厂商和 wire protocol。目标改为：

```rust
pub enum ProviderDriver {
    OpenAiResponses,
    AnthropicMessages,
    OpenAiChat(OpenAiChatDialect),
}

pub enum OpenAiChatDialect {
    Standard,
    DeepSeek,
    Kimi,
    Qwen,
    Glm,
}
```

这能表达：DeepSeek 是一个 Vendor Dialect，但实际使用 OpenAI Chat wire protocol。新增 Adapter 时必须明确是新增协议族还是已有协议族的新 Dialect。

Provider 类型拆成：

```rust
pub struct ProviderProfile {
    pub id: ProviderId,
    pub name: String,
    pub driver: ProviderDriver,
    pub base_url: String,
    pub models: Vec<ModelName>,
    pub enabled: bool,
    pub active: bool,
}

pub struct ProviderRuntimeConfig {
    pub profile: ProviderProfile,
    pub credential: ApiCredential,
    pub adapter_options: OpaqueAdapterOptions,
}
```

- `ProviderProfile` 可用于 Query/UI，不含 API Key。
- `ProviderRuntimeConfig` 不实现 `Serialize`，`Debug` 必须脱敏。
- `ApiCredential` 是内存边界，不等同于本次实现 Keychain。
- Tauri 的 Create/Update DTO 位于 Desktop/App，并映射为 Repository command；不能直接复用 RuntimeConfig。
- `adapter_options` 只能由选定 Adapter 校验，禁止覆盖 `model/messages/tools/stream` 等保留字段。

Provider Repository 至少区分普通查询和特权加载：

```rust
#[async_trait]
pub trait ProviderRepository: Send + Sync {
    async fn list_profiles(&self) -> Result<Vec<ProviderProfile>, ProviderRepositoryError>;
    async fn get_profile(&self, id: &ProviderId)
        -> Result<Option<ProviderProfile>, ProviderRepositoryError>;
    async fn load_runtime(&self, id: &ProviderId)
        -> Result<Option<ProviderRuntimeConfig>, ProviderRepositoryError>;
    async fn create(&self, command: CreateProvider)
        -> Result<ProviderProfile, ProviderRepositoryError>;
    async fn update(&self, command: UpdateProvider)
        -> Result<ProviderProfile, ProviderRepositoryError>;
    async fn delete(&self, id: &ProviderId) -> Result<(), ProviderRepositoryError>;
    async fn activate(&self, id: &ProviderId) -> Result<(), ProviderRepositoryError>;
}
```

`ProviderIndex` 是 App Query 形状，不是 Core 的模型合同，最终应移到 `openwork-app` 或 Desktop DTO。

## 7. `openwork-providers` 目标结构

```text
crates/openwork-providers/src/
├── lib.rs                         # 只公开 factory 和必要构造类型
├── factory.rs                     # RuntimeConfig -> Arc<dyn ModelPort>
├── gateway/
│   ├── mod.rs
│   ├── client.rs                  # ModelPort 入口/decorator 组合
│   ├── retry.rs                   # Retry executor，不解析厂商 body
│   └── transport_signal.rs        # Attempt/Retry 信号
├── transport/
│   ├── mod.rs
│   ├── http.rs                    # reqwest client、header/body limit
│   └── sse.rs                     # 只做 SSE framing
└── adapters/
    ├── mod.rs
    ├── openai_responses/
    │   ├── mod.rs
    │   ├── request.rs
    │   ├── response.rs
    │   ├── stream.rs
    │   └── error.rs
    ├── anthropic_messages/
    │   ├── mod.rs
    │   ├── request.rs
    │   ├── response.rs
    │   ├── stream.rs
    │   └── error.rs
    └── openai_chat/
        ├── mod.rs
        ├── request.rs
        ├── response.rs
        ├── stream.rs
        ├── error.rs               # family fallback
        └── dialect/
            ├── mod.rs
            ├── standard.rs
            ├── deepseek.rs
            ├── kimi.rs
            ├── qwen.rs
            └── glm.rs
```

### 7.1 为什么这样分

- `gateway` 拥有调用级可靠性，不拥有厂商 JSON。
- `transport` 拥有网络字节，不拥有 `message_stop`、`finish_reason` 等语义。
- `adapters` 拥有 vendor DTO、终态、Tool/Thinking 映射和 exact error code。
- 每个 Adapter 的 `request.rs` 只负责编码请求，`response.rs` 只负责累计/解码响应，`stream.rs` 只维护需要跨 event 的 Tool/Thinking 分片状态；`mod.rs` 只编排 HTTP、SSE 和这些 codec。
- OpenAI Chat 共用 envelope；Dialect 只覆盖真实差异，不为每个厂商复制完整 HTTP 调用。
- OpenAI Responses 与 OpenAI Chat 是不同 wire protocol，不能因为同一厂商合并进一个大文件。

### 7.2 Factory 和 Client 生命周期

- Desktop Composition Root 在应用启动时创建一个 `ProviderFactory`；Factory 持有共享 `HttpTransport` 和 Retry Policy。
- `HttpTransport` 使用 `Arc<HttpTransportInner>` 管理一个 `reqwest::Client`；Factory、Chat Runtime、Provider Test 和各 Adapter 的 Clone 共用其连接池与配置。
- Factory 根据 `ProviderRuntimeConfig` 返回套有 Retry decorator 的 `Box<dyn ModelPort>`；重新加载 Provider 配置不会重新创建 HTTP Client。
- Adapter 构造函数必须接收 `HttpTransport`，不得自行创建默认 `reqwest::Client`。App 后续可在 Composition Root 统一配置代理、TLS、User-Agent 和 Timeout 后注入。
- Provider connectivity test 当前是 `ProviderFactory::test`；它复用同一 Transport，但作为应用用例后续仍应迁移到 `openwork-app`。

### 7.3 Transport 边界

`HttpTransport` 只处理：

- 构造并发送 HTTP 请求。
- 连接池、TLS、代理、Timeout 和响应体大小上限。
- 返回 status、headers 和 byte stream。
- 将 reqwest 错误映射为 transport phase/delivery facts。

它不处理：

- Provider 业务错误码。
- JSON 请求或响应模型。
- SSE 终态。
- Tool/Reasoning/Usage。
- 是否应该重试。

`SseFramer` 只把任意 chunk 边界转换为标准 SSE frame：

```text
event name
event id
joined data
retry field(optional, ignored for LLM POST reconnect)
```

它必须正确处理 UTF-8 跨 chunk、多 `data:` 行、comments、CRLF/LF、空事件和 EOF。禁止自动重连 LLM POST。

### 7.4 Adapter 和 Dialect

每个 Adapter 负责：

1. 校验自身已实现的能力。
2. `ModelRequest -> vendor request DTO`。
3. `vendor response/event -> ModelEvent/ModelResponse`。
4. 提取 response ID、request ID、usage 和 raw finish reason。
5. exact provider code 到 `ModelErrorKind + RetryDirective` 的映射。
6. 生成/恢复同 Driver 的 Opaque Provider Block。

OpenAI Chat Dialect 只覆盖：

- endpoint 和额外 header。
- reasoning/thinking 请求字段。
- usage/finish reason 扩展。
- 厂商业务错误码表。
- 特殊 partial response 和流式终态。

任意 Dialect 不得通过通用 `extra_body` 重写基础请求。保留字段至少包括：

```text
model | messages | tools | tool_choice | stream |
temperature | max_tokens | max_completion_tokens
```

### 7.5 Capability 语义

Capability 分为两层：

1. **Adapter capability**：当前代码真正实现了什么，例如 text/stream/tool/reasoning round-trip。
2. **Model capability**：具体模型是否支持该能力。

本阶段只冻结第一层，不新增 capability table 或远程探测。Adapter 必须保守声明；未知模型能力由请求失败映射为 `CapabilityUnsupported/InvalidRequest`，不能静默丢参数。

当前 OpenAI Responses 和 Anthropic Adapter 已完成函数工具主链；Anthropic opaque thinking 可往返。但 OpenAI reasoning opaque item、Anthropic 按模型区分的 adaptive/manual thinking 请求参数仍未冻结，不能宣称所有模型的 Thinking 能力完全兼容。

### 7.6 厂商错误分类

公共层只提供 HTTP fallback：

```text
401 -> authentication
403 -> permission_denied
404 -> model_not_found/unknown resource
408 -> timeout candidate
429 -> unknown rate/quota candidate
5xx -> server_error candidate
```

最终分类由 Adapter/Dialect 的 exact error code 决定：

| Driver/Dialect | 可重试示例 | 不可重试示例 |
| --- | --- | --- |
| OpenAI | rate limit、temporary server error | insufficient quota、auth、invalid request |
| Anthropic | rate limit、5xx、529 overload | auth、permission、invalid request |
| DeepSeek | 429 rate、500/503 | 402 balance、invalid request |
| Kimi | `rate_limit_reached_error`、`engine_overloaded_error` | `exceeded_current_quota_error` |
| Qwen | 明确窗口型 throttling、ModelServingError | 欠费、未开通、账号错误 |
| GLM | `1302` 限流、`1305` 模型过载 | `1113` 欠费、`1308-1321` 中的套餐/额度/权限类错误 |

自由文本匹配只用于未知错误的最后诊断，不能独立决定重试或计费行为。

### 7.7 Retry Executor

Provider Transport Retry 仍位于 `openwork-providers/gateway/retry.rs`，但策略和机制分离：

```text
Adapter: ModelError + RetryDirective
RetryPolicy: budget/deadline/output gate -> RetryDecision
RetryExecutor: sleep/cancel/start next transport attempt
```

硬约束：

> 任何 Text、Reasoning、Tool Call ID/name 或 Tool Arguments 被提交给上层后，禁止透明重试。

默认预算保持：

```text
max_transport_attempts = 3
base_delay = 500 ms
max_backoff_delay = 8 s
full_jitter = true
```

Retry 同时受：

- 厂商 `RetryDirective`。
- 总 deadline。
- attempt budget。
- Stream 是否已提交语义输出。
- 调用是否被取消。

`Retry-After` 规则：

- 支持 delta seconds 和 HTTP date。
- Adapter 可支持 `retry-after-ms` 等厂商扩展。
- 若服务端要求的等待时间超过总 deadline 或允许等待上限，返回错误给 Core。
- 不能把 120 秒截成 30 秒后提前重试。

Retry 不进行静默模型切换，也不改写请求语义。

### 7.8 Attempt Signal 与可观测性

Provider Retry Executor 产生独立的低频信号：

```text
TransportAttemptStarted
ResponseHeadersReceived
FirstSemanticEvent
RetryScheduled
TransportAttemptFailed
TransportAttemptCompleted
```

每个信号至少携带：

```text
model_attempt_id
transport_attempt_no
provider_id
model_name
provider_request_id(optional)
phase
delivery_state
http_status(optional)
error_code(optional)
retry_delay_ms(optional)
timestamp
```

信号通过独立 Observer/Port 交给 Core/App；Providers 不依赖 Persistence。Observer 的持久化时序和失败策略在 Recorded Event 专题冻结前不写死进 Adapter。

### 7.9 原始数据和 Metadata

- 成功响应不再携带完整 raw JSON/SSE 数组。
- 默认只保留 response ID、request ID、model、usage、finish reason 和允许的 rate-limit headers。
- 错误 body 诊断片段最大 4 KiB，先 Redact 再截断。
- API Key、Authorization、Cookie、完整 Prompt、Tool Output 不进入 metadata。
- 若未来支持原始协议诊断，必须显式启用、设置 retention，并存入受控 Artifact，不进入普通 Provider Response。

## 8. `openwork-persistence` 目标结构

```text
crates/openwork-persistence/src/
├── lib.rs
└── postgres/
    ├── mod.rs
    ├── persistence.rs             # pool 生命周期、统一 migrate_all
    ├── migrations/
    │   ├── mod.rs                 # 全库有序 migration registry
    │   └── provider_registry.rs
    ├── provider_registry/
    │   ├── mod.rs
    │   ├── record.rs              # SQLx Row only
    │   └── repository.rs          # ProviderRepository 实现
    └── model_attempts/            # Recorded Event 稳定后加入
        ├── mod.rs
        ├── record.rs
        ├── projector.rs
        └── query.rs
```

### 8.1 PostgreSQL 生命周期

- `PostgresPersistence` 统一拥有 `PgPool` 和全库 migration registry。
- `PostgresProviderRepository::new(PgPool)` 只接收共享 Pool，不自行读取环境变量或决定何时迁移。
- `connect_from_env_or_local` 属于 App Composition Root/兼容入口，最终不留在单个 Repository。
- 不再为每个 Repository 创建数据库 facade。
- `openwork-database` 的通用能力逐步收进 Persistence；迁移期间允许薄兼容，不能形成两个长期持久化中心。

### 8.2 Provider Registry 表

当前仍是允许清空开发数据库的阶段，Provider Registry 使用单一干净基线，不保留旧表值兼容：

```text
providers
provider_models
```

目标字段语义：

#### `providers`

```text
id
name
driver_code            # wire protocol + dialect 的稳定数据库编码
base_url
api_key                # V1 技术债；普通 Query 永不选择/返回
enabled
active                 # 暂保留全局选择语义
adapter_options_json   # 替代可覆盖任意请求字段的 extra_body_json
created_at
updated_at
```

Driver 数据库值：

```text
openai_responses
anthropic_messages
openai_chat_standard
openai_chat_deepseek
openai_chat_kimi
openai_chat_qwen
openai_chat_glm
```

数据库通过命名 CHECK constraint 拒绝空名称、空 Base URL 和未知 Driver。`adapter_options_json` 仍是 V1 兼容边界，必须由选定 Adapter 校验且不得覆盖稳定请求字段。

#### `provider_models`

继续保留：

```text
provider_id
model_id
display_name(optional)
model_tier             # lite | plus | pro；仅用户分类，不触发自动路由
position
enabled
created_at
updated_at
PRIMARY KEY(provider_id, model_id)
UNIQUE(provider_id, position)
```

`model_tier` 只用于 UI 标签、筛选和分组。用户仍明确选择 `provider_id + model_id`；Provider/Runtime 不根据 tier 自动选模、重试或 Fallback。

本专题不新增 capability、price、embedding 或 fallback 表。

### 8.3 Repository 查询规则

- `list_profiles/get_profile` 的 SELECT 列表禁止包含 `api_key`。
- `load_runtime` 是唯一读取 credential 的特权方法。
- Provider 和 Models 写入使用同一事务。
- `list_profiles` 使用单次 Join/Aggregate 或固定两次批量查询，消除按 Provider 查询 Models 的 N+1。
- 任何 Repository Error 不包含 API Key 或完整 adapter options。
- active Provider 并发切换需要锁或可验证的事务策略，不能依赖偶发 unique violation。

### 8.4 Model Attempt Projection

`model_attempts` 不在本次目录重构中立即建表。它必须等 `RecordedEventV1`、`TurnId` 和 `ModelAttemptId` 冻结后，作为 Event Journal 的幂等查询投影建立，而不是由 `openwork-providers` 直接 INSERT。

目标投影字段：

```text
model_attempts
- model_attempt_id
- turn_id
- provider_id
- model_name
- status
- started_at
- first_semantic_event_at
- completed_at
- response_id
- provider_request_id
- finish_reason
- error_code
- provider_error_code
- input_tokens
- output_tokens
- cached_input_tokens
- reasoning_tokens
```

Transport 重试明细：

```text
model_transport_attempts
- model_attempt_id
- attempt_no
- started_at
- completed_at
- provider_request_id
- phase
- delivery_state
- http_status
- error_code
- provider_error_code
- retry_delay_ms
- semantic_output_emitted
UNIQUE(model_attempt_id, attempt_no)
```

两张表都是可重建 Projection；事实源仍是版本化 Recorded Event。Prompt、完整 Response、API Key 和原始 SSE 不进入这些表。

## 9. 厂商实现矩阵

| Driver | Wire API | 当前代码 | 目标最低能力 | 主要缺口 |
| --- | --- | --- | --- | --- |
| OpenAI Responses | `/v1/responses` | text/stream/function tools/usage/request ID | ordered output + reasoning opaque | output item 顺序、encrypted reasoning |
| Anthropic Messages | `/v1/messages` | text/stream/tools/usage/opaque thinking | model-aware thinking request | adaptive/manual thinking 参数差异 |
| OpenAI Chat Standard | `/chat/completions` | text/stream/tools/usage/request ID | 保持现有能力 | 任意兼容服务对 `stream_options` 的差异 |
| DeepSeek Dialect | OpenAI Chat | reasoning/tools/usage/exact HTTP errors | partial resource finish | `insufficient_system_resource` 的恢复策略 |
| Kimi Dialect | `/v1/chat/completions` + thinking | thinking/tools/usage/exact errors | preserved partial state | Partial Mode 与更多多模态 fixture |
| Qwen Dialect | OpenAI Chat compatible mode | thinking/tools/usage/exact errors | partial HTTP 200 | `x-dashscope-partialresponse` |
| GLM Dialect | OpenAI Chat compatible mode | thinking/stream tools/usage/exact codes | sensitive stream finish | 异常 `finish_reason` 精确映射 |

“已接入厂商”的完成标准不是 Factory 能构造 struct，而是 Coding Agent 所需的 text、stream、tool、error 和 retry 合同均通过 Fixture 测试。

## 10. 失败与重试状态机

```text
ModelAttemptRequested (Core recorded)
  -> TransportAttemptStarted(n)
  -> request_encoded
  -> request_sent / possibly_sent
  -> response_headers
  -> zero or more normalized ModelEvent
  -> ResponseCompleted
  -> ModelAttemptCompleted

failure before semantic output
  -> Adapter classifies error
  -> RetryPolicy checks budget/deadline
  -> RetryScheduled
  -> TransportAttemptStarted(n + 1)

failure after semantic output
  -> StreamInterrupted / ModelAttemptFailed
  -> no transparent retry
  -> Core decides semantic recovery
```

决策矩阵：

| 失败 | 首个语义事件前 | 首个语义事件后 |
| --- | --- | --- |
| DNS/TCP/TLS | 可重试 | 禁止 |
| 等待响应头超时 | 可重试，标记 possibly sent | 禁止 |
| HTTP 408 | 可重试 | 禁止 |
| 明确临时 rate/concurrency 429 | 可重试 | 禁止 |
| billing/quota/account 429/402 | 禁止 | 禁止 |
| 500/502/503/504、Anthropic 529 | 可重试 | 禁止 |
| SSE error event | 按厂商错误分类 | 禁止 |
| EOF 且无协议终态 | 无输出时可重试 | 禁止 |
| content filter/refusal | 禁止 | 禁止 |
| protocol decode error | 默认禁止 | 禁止 |
| 用户取消 | 禁止 | 禁止 |

Provider Retry 失败后，Core 可以选择重新规划、修改 Context、切换用户明确选择的模型或终止；这些都不是 Provider 的透明重试。

## 11. 安全约束

1. API Key 不实现 `Serialize`，普通 `Debug` 固定输出 `[REDACTED]`。
2. Provider list/get Profile 不访问 API Key 列。
3. HeaderValue 构造失败不得把原始密钥写入错误文本。
4. Adapter options 使用保留字段 denylist/typed decoder，不能覆盖统一请求。
5. Error body 和 metadata 先脱敏，再按字符/字节上限截断。
6. Provider 返回内容、Tool Call 参数和 Opaque block 都是不可信输入。
7. Tool Call 只有在 block completed 且 JSON 校验成功后才能交给 Core 执行。
8. Partial stream 只进入实时展示/Trace，不与重试后的新响应拼接。
9. 完整 Prompt/Response 的持久化由 Context/Recorded Event 专题决定，不由 Adapter 偷偷完成。
10. 不因 Retry 发生静默 Provider/Model 切换。

## 12. 测试和 Eval 门禁

### 12.1 Protocol 合同测试

- `ModelRequest/Response/Event/Error` serde fixture。
- Event block index/id 完整性。
- Tool arguments delta -> final JSON。
- `ProviderOpaqueBlock` 同 Driver 往返和跨 Driver 拒绝。
- Provider Profile 序列化不含 Secret。
- `ProviderDriver` 旧 kind 迁移映射。
- Error enum/struct exhaustive match，不允许业务逻辑 wildcard。

### 12.2 Transport 合同测试

- UTF-8 字符跨任意 network chunk。
- 多行 `data:` 拼接。
- LF、CRLF、comments、event/id fields。
- `[DONE]` 只由对应 Adapter 解释。
- EOF 前后终态差异。
- 大响应和错误 body 上限。
- Drop Stream 后 HTTP/Retry Future 被取消。

### 12.3 Adapter Fixture

每个 Driver 至少提供：

- Request JSON fixture。
- Buffered response fixture。
- Stream fixture，chunk 边界随机拆分。
- Tool Call fixture。
- Usage/finish reason fixture。
- Auth、rate limit、quota、overload、server error fixture。
- 成功/失败 request ID 提取。
- Provider 特有 partial/safety/opaque reasoning fixture。

测试只使用本地 mock HTTP server，不依赖真实 API Key 或在线模型。真实 Provider smoke test 是手动/受控测试，不作为默认 CI。

### 12.4 Retry 确定性测试

- 注入 Clock/Sleeper/Jitter source，不依赖系统纳秒做不可重复断言。
- 第一次成功、最终失败、Retry-After、deadline 超限。
- output gate 覆盖 Text/Reasoning/Tool ID/name/arguments。
- Retry-After 超上限时返回错误，不提前重试。
- 每个 attempt 保持同一 `ModelAttemptId`，递增 attempt number。

### 12.5 PostgreSQL 测试

- 干净基线不包含 `models_json`、旧 `kind` 或旧 `extra_body_json` 列。
- Driver 和 `lite/plus/pro` 约束拒绝非法值。
- Provider Profile 查询不会返回 API Key。
- `load_runtime` 才返回 Credential。
- Provider + Models 事务和约束。
- 批量列表无 N+1。
- active 并发切换结果唯一且确定。
- 后续 Projection 从同一 Recorded Events 重建相同 attempt 表。

## 13. Adopt / Extend / Build

| 能力 | 决策 | 原因 |
| --- | --- | --- |
| HTTP | Adopt 当前 `reqwest 0.12` | 已在项目使用，连接池/流式/TLS 足够；不为 retry 单独升级大版本 |
| Async/Stream | Adopt Tokio + `futures-core/util` | 支撑 streaming-first Port 和背压 |
| SSE framing | 倾向 Adopt `eventsource-stream 0.2`，包装在 `transport::sse` | 修复自研 parser 的 UTF-8/multiline 风险；库较旧，必须以 Fixture 合同测试兜底 |
| Vendor event decode | Build | 厂商终态、Tool、Usage 和 Error 不属于通用 SSE 库能力 |
| Retry executor | Build | 必须理解业务码、语义输出闸门、deadline、取消和 Attempt signal |
| Backoff library | 暂不采用 | BackON 不能替代 Retry-After、输出闸门和 attempt trace，当前收益不足 |
| Tower/reqwest middleware retry | Reject 当前引入 | HTTP status 中间件无法正确解析厂商 body，且 stream retry 语义不匹配 |
| 官方 Rust Provider SDK | 不采用为核心边界 | 当前主要厂商官方合同仍以 HTTP API/官方非 Rust SDK 为准；OpenWork 需要统一的跨厂商 Port |

若 `eventsource-stream` 原型无法满足所需控制或维护风险不可接受，则保留自研 `SseFramer`，但必须先通过第 12.2 节的全部协议测试。该选择记录到实现阶段 Decision Log。

## 14. 迁移路线

以下是专题实现的参考顺序，不等同于蓝图 S0-S8 的固定排期。

### M0：冻结 Fixture 与当前行为

- 为当前 OpenAI/Anthropic/OpenAI Chat 请求、响应、SSE 和错误建立 Fixture。
- 增加 API Key 不泄漏测试。
- 增加 SSE UTF-8/multiline 失败用例，先证明问题。

退出条件：不访问外网即可重复当前行为和目标失败用例。

### M1：Protocol 真正迁入 `model/`

- 把实际类型从 `ai` 移入 `model` 子模块。
- `ai` 仅保留 deprecated compatibility re-export。
- 合并旧 Error variant 与 `HttpModelError`。
- 引入 ProviderProfile/RuntimeConfig 分离。

退出条件：新代码只从 `openwork_protocol::model/provider` 导入，旧调用仍可编译。

### M2：Providers 只做内部搬迁，不改语义

- [已完成] 建立 `transport/adapters/gateway` 目录。
- [已完成] 将三类 Adapter 的 request/response/stream codec 移出编排模块，并把 Kimi 改为薄 Dialect。
- [已完成] Shared Http Client 由 `ProviderFactory` 持有并注入，Desktop/Runtime/Test 共享同一 Transport 生命周期。
- `test_provider` 移到 App use case。

退出条件：Fixture 输出与 M0 相同，crate 公共 surface 变小。

### M3：修复 Transport 与厂商错误

- [已完成] 引入/实现正确 SSE framing。
- [已完成主要官方码] 公共错误只做 fallback；每个 Dialect 建 exact code 表，后续随官方文档继续补 fixture。
- [已完成] Retry-After 支持 delta-seconds、HTTP-date 与 `retry-after-ms`。
- [已完成] 停止累积完整 raw SSE。

退出条件：第 12.2、12.3、12.4 节测试通过。

### M4：Streaming-first Port

- 加入 block identity 和统一 Stream。
- 删除 Callback + unbounded bridge。
- 将 `stream` 从 ModelRequest 删除。
- Runtime/Agent 迁移到 Stream polling 和结构化取消。

退出条件：长流具有背压，取消不会遗留 HTTP/Retry 任务，流式 Tool JSON 可验证完成。

### M5：Provider 主链能力补齐

- [已完成函数工具主链] OpenAI Responses Tool Calling。
- [已完成] Anthropic Tool Use 和 Opaque Thinking 往返。
- [部分完成] DeepSeek/Kimi/Qwen/GLM 的 usage、Thinking 请求和主要错误码；特殊 partial/异常 finish 继续补齐。

退出条件：每个标记为可用的 Driver 都通过 Coding Agent 最小 Tool Loop Fixture。

### M6：Persistence 内部重组

- `PostgresPersistence` 统一 Pool/Migration 生命周期。
- Provider Registry 子模块化并消除 N+1。
- 普通 Profile 与 Runtime Secret 查询分离。
- 使用开发期单一干净 Provider Registry 基线，不保留旧表值兼容。

退出条件：PostgreSQL 测试通过，Provider UI 不再接收保存后的 API Key。

### M7：Attempt Recorded Event 与 Projection

- 等 Core Recorded Event 合同冻结。
- Provider signal 映射为 Recorded Event/Telemetry。
- 建立幂等 `model_attempts/model_transport_attempts` Projector。

退出条件：崩溃后能从 Journal 重建相同 Attempt Projection；Provider crate 没有 SQLx 依赖。

## 15. 回滚策略

- M1-M3 保留旧 re-export 和 Factory facade，可按 commit 回滚，不迁移用户数据。
- Streaming-first 通过 Feature Flag/兼容 Adapter 接入旧 Agent，达到 parity 前不删除旧 callback 方法。
- 当前开发库允许重建；进入正式环境后 migration 只追加，不再修改已登记版本。
- Attempt Projection 删除后可从 Journal 重建；不能反向删除事实事件。
- 任一厂商新 Adapter 未通过 Fixture 时，只标记为 unsupported，不静默回退到 Generic Dialect。

## 16. 完成标准

本专题完成必须同时满足：

- `openwork-protocol::model` 是真实所有者，`ai` 只剩兼容层或已删除。
- Model Port 只有一个 streaming-first 调用入口。
- Model Event 可表达多个有序 block。
- Tool Call 只有在完整 JSON 后才交给 Core。
- Reasoning 展示与厂商 opaque 往返状态分离。
- Provider Driver 能区分 OpenAI Responses、Anthropic Messages 和 OpenAI Chat Dialect。
- `openwork-providers` 的 Transport 不含厂商终态或业务错误知识。
- 每个厂商业务码由对应 Adapter/Dialect 映射。
- Retry-After、deadline、取消和 semantic output gate 有确定性测试。
- 完整 raw stream 不再进入 ModelResponse。
- Provider Profile/普通 Query 不含 API Key。
- `openwork-persistence` 统一 Pool/Migration 生命周期，Provider 列表无 N+1。
- Attempt 表只作为 Recorded Event Projection，Providers 不直接写数据库。
- Embedding、Fallback、Router、Capability 表未被顺带加入。

## 17. 官方依据

- [OpenAI Responses API](https://platform.openai.com/docs/api-reference/responses)
- [OpenAI streaming events](https://platform.openai.com/docs/api-reference/responses-streaming/response/queued)
- [OpenAI request IDs and rate-limit headers](https://platform.openai.com/docs/api-reference/introduction)
- [OpenAI SDK retry behavior](https://github.com/openai/openai-python#retries)
- [Anthropic Messages API](https://platform.claude.com/docs/en/api/messages/create)
- [Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)
- [Anthropic errors, request IDs and retry behavior](https://platform.claude.com/docs/en/api/errors)
- [Anthropic Tool Use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/how-tool-use-works)
- [DeepSeek Chat API](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek error codes](https://api-docs.deepseek.com/quick_start/error_codes)
- [Kimi Chat API](https://platform.kimi.com/docs/api/chat)
- [Kimi error types](https://platform.kimi.com/docs/api/errors)
- [Qwen/Model Studio error codes](https://help.aliyun.com/en/model-studio/error-code)
- [GLM API error codes](https://docs.bigmodel.cn/cn/api/api-code)
- [RFC 9110 Retry-After](https://www.rfc-editor.org/rfc/rfc9110.html)
- [reqwest](https://docs.rs/crate/reqwest/latest)
- [eventsource-stream](https://docs.rs/crate/eventsource-stream/latest)
- [BackON](https://docs.rs/crate/backon/latest)
- [Tower Retry](https://docs.rs/tower/latest/tower/retry/index.html)

## 18. Decision Log

| 决策 | 结论 | 原因 |
| --- | --- | --- |
| 三个 crate 是否合并 | 拒绝 | 稳定合同、厂商 IO 和 PostgreSQL 生命周期是三个不同变化轴 |
| 每个厂商完全独立复制 Adapter | 拒绝 | OpenAI Chat family 可复用 envelope，但 Dialect 必须保留真实差异 |
| 所有 Provider 统一成 OpenAI-compatible | 拒绝 | Anthropic/OpenAI Responses 的 block、tool、thinking 和 error 语义不同 |
| 保留 `generate + stream_generate + stream bool` | 拒绝 | 能构造互相矛盾的调用状态，且 callback 无背压 |
| reasoning 只存纯文本 | 拒绝作为最终合同 | Anthropic/Kimi 需要同 Provider opaque round-trip |
| 中央错误分类器按英文文本判断 | 拒绝 | 会把欠费、限流和账号错误混为一类 |
| Retry 使用通用 HTTP middleware | 拒绝 | 无法理解 body、stream output gate 和 attempt trace |
| Model Attempt 由 Providers 直接写 PG | 拒绝 | 违反 Port 和 Persistence 边界，无法成为 Core 事实源 |
| Provider 列表返回完整 RuntimeConfig | 拒绝 | 不必要地扩大 API Key 暴露面 |
| `lite/plus/pro` 驱动自动路由 | 拒绝当前引入 | tier 仅是用户模型分类，调用仍明确指定 Provider 和 Model |
| 立即建立 capability/price/fallback 表 | 拒绝 | 当前没有功能和 Eval 证据 |
| SSE framing 采用现有实现 | 暂不接受为稳定 | 先比较 `eventsource-stream` 原型和完整协议 Fixture |

## 19. 实现前仍需确认

以下问题不会改变三 crate 的职责，但会影响具体代码：

1. `eventsource-stream 0.2` 原型是否通过全部 SSE Fixture，还是保留自研 Framer。
2. `ModelTransportSignal` 通过 Stream side channel、async Observer 还是 Core Reporter 传递；需与 Recorded Event 专题共同冻结。
3. 旧 Callback Port 保留一个版本还是一个 Feature Flag 周期。

这些选择必须写入实现阶段 Decision Log；不得再次把临时兼容实现描述为稳定协议。
