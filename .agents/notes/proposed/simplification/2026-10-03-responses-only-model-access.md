# Agent Note: 模型接入照搬 Codex，只保留 Responses 协议

Status: proposed

## 问题

`openwork-models` 有 5,644 行，实现了三种线协议：OpenAI Responses（786 行）、OpenAI Chat（1,090 行）、Anthropic Messages（1,076 行）。厂商也写成了代码：

- `ProviderKind` 枚举列出 Openai、Glm、Kimi、Deepseek、Qwen、Anthropic（`provider/driver.rs`）。`OpenAiChatDialect` 与 `ErrorDialect` 又各列一遍。
- 厂商差异写成 `match`。`adapters/openai_chat/request.rs` 中，Kimi 用 `max_completion_tokens`，Qwen 用 `enable_thinking`，DeepSeek、GLM、Kimi 用 `thinking: {type}`，GLM 流式加 `tool_stream`。
- 预设写在 `openwork-core/src/provider.rs` 中，每个厂商一段代码。`core.rs` 的 `parse_provider_kind` 另接受 `openai_chat_*` 旧别名，与 `ProviderKind::parse` 不一致。
- 前端 `providerContracts.ts` 再写一份联合类型。

新增一个厂商要改约 7 个文件。

Codex 只保留 Responses 一种协议，厂商与模型都是数据（`/Volumes/Extreme SSD/Code/codex/codex-rs`）：

- `WireApi` 只有 `Responses`（`model-provider-info/src/lib.rs`）。配置 `wire_api = "chat"` 时直接报错。弃用提示见提交 `43e6e75317`，删除见提交 `d2394a2494`（“nuke chat/completions API”，−2,931 行）。理由见 [discussions/7782](https://github.com/openai/codex/discussions/7782)：Chat Completions 来自 GPT-3.5 时代，不是为 agentic 场景设计的，兼容它增加复杂度与回归。
- 连接是 `ModelProviderInfo`：`base_url`、`env_key`、请求头、重试与超时，用户在 `config.toml` 的 `[model_providers.<id>]` 中声明。
- 模型是 `models-manager` 的 `ModelInfo`：随应用打包的 `models.json`。目录中找不到的模型按最长前缀匹配；仍找不到时用默认元数据，并记 warn（`model_info.rs` 的 `model_info_from_slug`）。

OpenWork 接入的国内厂商都已提供 Responses 兼容接口（2026-10-03 核实官方文档）：

| 厂商 | Responses 接口 | 是否有状态 |
|---|---|---|
| DeepSeek | `https://api.deepseek.com/responses`（[文档](https://api-docs.deepseek.com/guides/responses_api)） | 无状态，不支持 `previous_response_id` |
| Kimi 国内站 | `https://api.moonshot.cn/v1/responses`（[文档](https://platform.kimi.com/docs/api/responses.md)） | 无状态 |
| Kimi 国际站 | `https://api.moonshot.ai/v1/responses`（[文档](https://platform.kimi.ai/docs/api/responses.md)） | 无状态 |
| 智谱 GLM 国内站 | `https://open.bigmodel.cn/api/v1/responses`（[文档](https://docs.bigmodel.cn/cn/guide/develop/responses/introduction)） | 支持 `store` 与 `previous_response_id` |
| 智谱 z.ai 国际站 | `https://api.z.ai/api/v1/responses`（[文档](https://docs.z.ai/guides/llm/glm-5.3)） | 字段与国内站是否相同未确认 |

## 提议

只保留 Responses 一种线协议（用户决定，2026-10-03）。照 Codex 的做法：

### 连接是配置

Provider 写在 `~/.openwork/config.json`（见 [去掉 Postgres 与 Redis 的提议](2026-10-03-drop-postgres-and-redis.md)）。字段对应 Codex 的 `ModelProviderInfo`：

| 字段 | 含义 | 默认值 |
|---|---|---|
| `name` | 显示名 | provider id |
| `baseUrl` | Responses 接口的基础地址 | — |
| `apiKey` / `envKey` | 直接写 API key，或写环境变量名（二选一） | — |
| `httpHeaders`、`queryParams` | 附加的请求头与查询参数 | 无 |
| `requestMaxRetries` | 建立连接阶段的重试次数 | 4 |
| `streamMaxRetries` | 流断开后的重试次数 | 5 |
| `streamIdleTimeoutMs` | 两个流事件之间的最长间隔 | 300,000 |
| `models` | 可用的模型 id，可逐项覆盖模型目录中的字段 | — |

内置预设是一份随应用发布的默认配置：OpenAI、DeepSeek、Kimi、智谱 GLM。不适配通义千问（用户决定，2026-10-03）。界面上，用户只选预设并填 API key（用户决定，2026-10-03）。预设 id 就是 Provider id，每个预设只能添加一次。新增或覆盖 Provider 时，直接改 `config.json`。配置中没有 `wireApi` 字段，因为只有一种协议。

### 模型元数据与连接分开

- 应用打包一份默认模型目录：上下文窗口、最大输出、支持的推理档位、是否支持并行 Tool Call 等。
- 目录中找不到的模型按最长前缀匹配；仍找不到时用默认值，记 warn，并在界面上提示。
- 不照搬 Codex 从 `/models` 拉取模型目录：各厂商 `/models` 的返回格式不同，Codex 的格式是 OpenAI 后端专有的。

### 模型目录声明显示名与推理档位

用户只给 API key，就能选模型和推理档位。模型列表与档位都来自目录，不来自用户输入。DSH 与 Codex 都这样做：

- DSH：`packages/llm/llm-deepseek/src/models.ts` 写模型 id 与显示名；`model-info.ts` 给每个模型声明 `reasoning.efforts` 与 `defaultEffort`；`llm-deepseek-api-key/src/index.ts` 的 `discoverModels` 直接返回目录。
- Codex：`codex-rs/models-manager/models.json` 的每个模型有 `display_name`、`supported_reasoning_levels`、`default_reasoning_level`。

目录条目的字段：

| 字段 | 含义 |
|---|---|
| `modelId` | 线上的模型 id |
| `displayName` | 界面显示名 |
| `contextWindowTokens`、`maxOutputTokens`、`maxReasoningTokens`、`acceptsDataBlocks` | 现有的能力字段 |
| `reasoningEfforts` | 可选的档位，按文档顺序列出 `reasoning.effort` 的取值。空列表表示这个模型不推理，界面不显示档位选择 |
| `defaultReasoningEffort` | 默认档位，必须在 `reasoningEfforts` 中 |

规则：

- `effort` 是 Responses 协议 `reasoning.effort` 的取值，原样发送。OpenWork 不定义自己的档位枚举，也不按厂商映射。
- 每个模型的档位取值来自厂商文档。实施时，在本 Note 中列出每个值的来源。
- 模型有档位时，请求带 `reasoning: {effort}`。没有档位时，不带 `reasoning`。
- 请求不带 `include`。OpenAI 在 `store: false` 时默认返回 `encrypted_content`，`include` 只为兼容保留（[reasoning 指南](https://developers.openai.com/api/docs/guides/reasoning.md)）。其他三家不支持 `include`。
- 回传条目时，保留服务端给的带前缀 id（`rs_…`、`msg_…`），去掉没有前缀的 id，与 Codex 的 `prepare_response_items_for_request` 一致。
- `ModelRequest` 用 `reasoning_effort: Option<String>` 替换 `ThinkingConfig`。上下文压缩的摘要请求传 `None`。
- 预设列出这家的全部模型 id。每个 id 必须在目录中，单元测试检查这一点。
- Session 保存用户选的模型与档位（`sessions.reasoning_effort`）。没有选，或选的档位不在新模型的列表中时，用目录中的默认档位。
- Session 空闲时才能换模型或档位。换完后卸载运行时，下一个 Turn 按新模型重建。子代理沿用父 Session 派生时的模型与档位。
- Model Call 的 Trace 属性 `reasoningEffort` 记录实际发送的档位。
- `config.json` 中模型的 `displayName` 优先于目录，供目录以外的模型使用。
- 聊天输入框的选择器分两级：模型、推理档位，照 DSH 的界面。

### 内置模型目录（2026-10-03 核实）

只收文档写明支持 Responses、并写明档位的模型。数字与档位都来自下列来源，没有实际调用验证。

| 预设 | 模型 id | 显示名 | 上下文 | 最大输出 | 图片 | 档位 | 默认档位 |
|---|---|---|---|---|---|---|---|
| openai | `gpt-6-astra` | GPT-6 Astra | 1,050,000 | 128,000 | 是 | low、medium、high、xhigh、max | medium（未确认） |
| openai | `gpt-6.1-sol` | GPT-6.1 Sol | 1,050,000 | 128,000 | 是 | low、medium、high、xhigh、max | medium |
| openai | `gpt-6-luna` | GPT-6 Luna | 1,050,000 | 128,000 | 是 | none、low、medium、high、xhigh、max | medium |
| deepseek | `deepseek-flash` | DeepSeek-V4.1-Flash | 1,048,576 | 393,216 | 是 | none、low、high、max | high |
| deepseek | `deepseek-v4-pro` | DeepSeek-V4-Pro | 1,048,576 | 393,216 | 否 | none、low、high、max | high |
| kimi | `kimi-k3` | Kimi K3 | 1,048,576 | 131,072 | 是 | low、high、max | max |
| glm | `glm-5.3` | GLM-5.3 | 1,048,576 | 131,072 | 否 | low、high、max | max |

来源：

- OpenAI：[gpt-6-astra](https://developers.openai.com/api/docs/models/gpt-6-astra.md)、[gpt-6.1-sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol.md)、[gpt-6-luna](https://developers.openai.com/api/docs/models/gpt-6-luna.md)。Astra 不接受 `none`，传入时返回 400（[reasoning 指南](https://developers.openai.com/api/docs/guides/reasoning.md)）。
- DeepSeek：[模型与价格](https://api-docs.deepseek.com/quick_start/pricing)、[GET /models 示例](https://api-docs.deepseek.com/api/list-models)、[思考模式](https://api-docs.deepseek.com/guides/thinking_mode)。`none` 关闭思考。旧 id `deepseek-v4-flash` 已转给 V4.1-Flash。
- Kimi：[Responses](https://platform.kimi.ai/docs/api/responses.md)、[K3 快速开始](https://platform.kimi.ai/docs/guide/kimi-k3-quickstart.md)。Responses 只支持 `kimi-k3`。K3 始终思考。最大输出取默认值 131,072，上限是 1,048,576。
- 默认档位：文档没有写 GPT-6 Astra 的默认值。目录暂取同系列的 `medium`（用户决定，2026-10-03）。
- GLM：[GLM-5.3](https://docs.bigmodel.cn/cn/guide/models/text/glm-5.3)、[Codex 接入](https://docs.bigmodel.cn/cn/coding-plan/tool/codex)、[创建 Response](https://docs.bigmodel.cn/api-reference/response/创建-response)。GLM-5.3 始终思考。

各家共同点：

- 只有 OpenAI 返回 `encrypted_content`。DeepSeek、Kimi、GLM 返回明文推理（`content` 或 `summary`），回传时原样放回 `input`。
- 都接受 `reasoning.effort`。不在列表中的值，有的厂商映射到相近档位，有的返回 400。目录只列文档写明支持的值。

没有收入目录的模型：

- Kimi K2.6、K2.7 Code：文档没有说它们支持 Responses。
- GLM-5.2、GLM-5.3-Flash：模型页只列 Chat 接口。

### 请求照 Codex 构造

- `store: false`、`stream: true`，不使用 `previous_response_id`。每次请求带完整的 input。DeepSeek、Kimi 本来就是无状态的。
- 模型返回的 reasoning 条目写进会话历史，下一次请求原样放回 input。
- `prompt_cache_key` 取 Session id。
- 工具用 function 工具描述。

### 重试与错误照 Codex 分两层

- 连接阶段：重试 5xx 与传输错误，不重试 429。退避从 200ms 起指数增长，带 ±10% 抖动。
- 流阶段：流断开或空闲超时后，重试可重试的错误。服务端给出等待时间时按它等待。
- 错误按 `response.failed` 的 `error.code` 与 HTTP 状态码分类：上下文超限、额度用尽、限流（从消息中解析等待时间）、服务过载、认证失败。

### 删除

- `adapters/openai_chat/` 与 `adapters/anthropic_messages/`，共 2,166 行。
- `ProviderKind`、`OpenAiChatDialect`、`ErrorDialect` 与它们的全部 `match`。
- `openwork-core/src/provider.rs` 的预设代码与 `core.rs` 的 `parse_provider_kind`。
- 前端 `providerContracts.ts` 中的厂商联合类型。前端类型由生成器产生（见 [内核加插件的提议](../architecture/2026-10-03-kernel-and-plugins.md)）。

### 不照搬

WebSocket 传输、ChatGPT 登录、Amazon Bedrock 与 SigV4 签名、用命令获取 token。这些只服务 OpenAI 自己的后端或云业务，OpenWork 用不上。

## 考虑过的方案

**保留 Chat 协议，把厂商差异改成 `compat` 开关（pi-ai 的做法）。** 这是本 Note 的上一版提议。没有采用：用户决定只保留 Responses（2026-10-03），而且接入的国内厂商都已提供 Responses 接口。

**同时保留 Anthropic Messages。** 没有采用：只要多一种协议，就多一套请求、流解析与错误分类。接入的国内厂商也都提供 Anthropic 兼容接口，但没有一家只支持 Anthropic 而不支持 Responses。

**OpenWork 定义固定的档位枚举，再按厂商翻译（DSH 的做法）。** DSH 定义 `off`、`low`、`high`、`max`，在 `packages/llm/llm-deepseek/src/serialize.ts` 中翻译成 Anthropic Messages 的 `thinking` 与 `output_config.effort`。没有采用：各家都实现 Responses，`reasoning.effort` 已经是统一字段。再加一层翻译，就又回到按厂商写代码。

**让用户在设置页手填模型与档位。** 这是上一版表单的做法。没有采用：用户决定表单只留预设与 API key（2026-10-03）。

**用 Rust 多厂商库（`genai`、`rig-core`、`llm`）。** 没有采用：只剩一种协议，Codex 也是自己实现，没有必要再引入一层库。

## 验收条件

- `openwork-models` 只剩 Responses 一个线协议实现。代码中没有按厂商分支的 `match`。
- 新增一个提供 Responses 接口的厂商，只改 `config.json`。
- 每个内置预设有一条录制的请求与流式响应样例，用契约测试固定请求体与事件解析。
- 每个内置预设有一次真实调用的手动记录：普通回答、Tool Call 往返、reasoning 回传，各一次。
- 重试、空闲超时与错误分类有单元测试，覆盖 429 不重试、流断开后重试、上下文超限。
- 目录测试：每个预设模型都在目录中，并且有 `displayName`；`defaultReasoningEffort` 在 `reasoningEfforts` 中。
- 请求测试：选中的档位原样写入 `reasoning.effort`；档位为 `None` 时，请求不带 `reasoning`；回传时保留带前缀的条目 id。
- Session 测试：保存的档位在重新加载后不变；没有保存档位时，用目录默认值。
- 手动：只填 API key，就能在聊天输入框选模型与档位；Trace 显示发送的 `reasoning.effort`。

## 风险

- 各家 Responses 实现在流式事件与工具字段上可能与 OpenAI 不同。核实时只细读了 DeepSeek 的文档，例如它的 custom 工具只接受 `apply_patch`。每个预设都要先做真实调用记录，再定稿。
- 智谱文档说订阅过 GLM Coding Plan 的账号暂时只能用 Chat 协议，与它的 Codex 接入指南矛盾。这类账号可能不能接入。
- Kimi K3 要求账户累计充值后才开放。
- 只能用 Chat 协议的模型或网关需要用户自己经转换代理接入，例如 LiteLLM。Codex 的讨论区里也有用户这样做。
