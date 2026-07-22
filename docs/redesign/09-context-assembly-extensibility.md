# OpenWork 上下文窗口分层与请求组装

> 状态：当前实现说明、目标治理边界与未来兼容约束。
>
> 范围：以 V1 已存在的 Agent System Prompt、Session 根部 `AGENTS.md`、Conversation、Tools、Model Request 和只读预算估算为实现基线；同时定义 Memory、Plan、Skill、Compaction 等能力未来接入哪条物化链，以及压缩/恢复时必须保持的边界。后者是兼容约束，不表示已经实现。
>
> 源码对照基线：OpenWork 当前工作树、OpenCode `a19b52e85`、grok-build `b189869`，核对于 2026-07-22。

## 1. 结论

最终选择不是把 OpenCode V2 和 grok-build 的内部结构拼在一起，而是明确采用下面的职责组合：

1. 借鉴 OpenCode V2：三条独立物化链，以及 System Context Source 的生命周期；
2. 借鉴 grok-build：synthetic provenance、稳定前缀、压缩后的上下文恢复和 Prompt Cache 稳定性；
3. 保留 OpenWork 的集中请求组装：Chat State 只拥有 Conversation，不构造完整 `ModelRequest`。

整体骨架为：

```text
System Sources              Conversation State              Tool Capability State
      │                            │                                 │
      ▼                            ▼                                 ▼
System Context              Conversation View                Tool Surface
Materialization             Materialization                 Materialization
      └────────────────────────────┼─────────────────────────────────┘
                                   ▼
                    Central Model Request Builder
        ↓
Provider-neutral ModelRequest
        ↓
Provider Adapter
```

这里有“三条物化链 + 一个组装边界”，不是四种 Context：

- 三条物化链分别产出 System Context、Conversation View 和 Tool Surface；
- `ModelRequestBuilder` 只消费三条链的结果，不拥有任何上游来源；
- Provider Adapter 只做协议编码。

OpenCode V2 和 grok-build 在输入分区上并不冲突，区别主要在所有权和持久化方式：

- OpenCode V2 把 System、Session History、Tool Materialization 分开，在调用点汇合；
- grok-build 把 leading System 和多个 synthetic 前缀放进 Chat State 的 `items`，并由 Chat State 构造 `ConversationRequest`；
- OpenWork 采用前者的边界，同时吸收后者关于 provenance、恢复和字节稳定性的约束，但不采用后者的完整请求所有权。

OpenWork 当前代码已经完成这一骨架：`SystemContextBuilder`、`ConversationView` 和 `FinalizedToolset` 分别提供三块输入，`ModelRequestBuilder` 在 Core 中集中组装请求。旧的 `ContextBundle`、通用 `ContextKind`、`ModelCallPreparer`、`ModelRequestSpec` 和 `RequestAssembler` 已删除。

本文继续区分“当前已实现”和“目标约束”。后续加入 Memory、Plan、Skill 或 Compaction 时，应扩展对应来源和物化链，而不是把它们全部塞进 Chat State 或 `ModelRequestBuilder`。

## 2. 不要混淆两个维度

“Context Window 分层”和“Model Request 构建步骤”不是一回事。

### 2.1 数据分区：模型实际看到了什么

当前一次主模型调用的窗口可以按输入内容分成三块：

```text
Model Context Window
├── Input
│   ├── 1. System Context
│   │   ├── Agent System Prompt
│   │   └── Project Instructions（Session 根部 AGENTS.md）
│   ├── 2. Conversation
│   │   └── User / Assistant / Tool Messages
│   ├── 3. Tool Surface
│   │   └── Tool name / description / JSON Schema
│   └── Provider Framing
│       └── role、tool framing 等协议开销
└── Output Reserve
    └── 为本次模型回答预留
```

前三项是 OpenWork 可以组织和测量的模型输入。Provider Framing 是协议开销估算，不应建模成业务 Context Source；Output Reserve 也不是输入内容。

从模型实际占用看，输入关系近似为：

```text
Actual Provider Input
  ≈ System Context + Conversation + Tool Surface + Provider Overhead

Current ContextBudgetEstimate
  = System Context + Conversation + Tool Surface
```

当前 `ContextBudgetEstimate` 不计算 Provider Overhead，因此它是请求主体的近似值，不能视为 Provider 的权威 token 计数。

### 2.2 处理分层：这些内容如何成为请求

构建过程是另外一个维度：

```text
Resolve System Context
        +
Read Conversation View
        +
Read Tool Definitions / Model Settings
        ↓
Build Provider-neutral ModelRequest
        ↓
Encode Provider Request
```

其中：

- System Context 和 Conversation 是数据；
- Message Conversion 和 Request Preparation 是处理步骤；
- Provider Adapter 是协议转换边界。

不能把这些名称作为同一级 Context Window 分区，否则会把“有什么”和“怎么生成”混在一起。

## 3. 三条物化链与一个组装边界

本文使用“三条物化链”描述运行时职责。这是对 OpenCode V2 当前源码路径的归纳，不是 OpenCode 官方声明的三层模型。

“物化”表示：从某类权威来源读取当前状态，按确定规则生成一次模型调用可消费的只读结果。物化结果不是新的业务真相，也不反向拥有来源。

### 3.1 System Context Materialization

负责取得 Conversation 之外、以 System 身份发送给模型的输入：

```text
Agent Definition
  -> Agent System Prompt

Working Directory
  -> ProjectInstructionLoader
  -> AGENTS.md content

Agent System Prompt + Project Instructions
  -> ResolvedSystemContext
```

这条链可以执行来源读取、生命周期判断、确定性排序和渲染，但不读取 Conversation，不选择 Tools，也不构造完整 `ModelRequest`。

当前 V1 生命周期很简单：

- 每个 Turn 开始时解析一次；
- 同一 Turn 内的多次 Model Call 复用同一份结果；
- 下一个 Turn 重新读取 Session 根部 `AGENTS.md`。

目标生命周期借鉴 OpenCode V2 的 Source 思路：每个可独立变化的 System 来源应有稳定身份、权威数据、读取方式、baseline 渲染，以及必要时的 update/removal 语义。需要跨压缩或跨进程恢复时，再为已接纳的来源增加 durable snapshot/epoch。

这不等于现在就建立通用 Source Registry，也不等于给每个 `SystemContextPart` 堆上 `budget`、`retention` 等未来字段。V1 仍由具体的 `ProjectInstructionLoader` 负责第一个文件来源；只有出现第二个真正需要独立更新和恢复的动态来源时，才抽出通用生命周期接口。

### 3.2 Conversation Materialization

负责把 Chat State 拥有的 Conversation 状态物化为一致视图：

```text
Committed User / Assistant / Tool Messages
+ future Synthetic Conversation Items
+ future Compaction Checkpoint and Recent Tail
  -> Chat State（单写与一致性）
  -> ConversationView
```

当前 `ConversationView` 就是完整的已提交 Conversation，没有其他投影策略。每次 Model Call 都重新读取，因此前一次调用产生的 Assistant Message 和 Tool Result 会进入下一次调用。

未来加入 synthetic item 或压缩后，这条链可以负责：

- 区分真实 User Message 和系统生成的 User-role Message；
- 从 compaction checkpoint、保留的 recent tail 和压缩后的新消息生成当前视图；
- 保证 Tool Call / Tool Result 配对和顺序一致；
- 原子安装新的压缩视图。

Chat State 仍不加载 `AGENTS.md`、Memory、Plan 或 Skill 的权威数据，不接收 Model 或 Tool Definitions，也不构造完整 `ModelRequest`。只有当这些功能产生“应当作为 Conversation 发送且需要重放”的条目时，该条目及其 provenance 才进入 Chat State。

### 3.3 Tool Surface Materialization

负责从本次 Agent、权限和工具注册状态生成“模型可见且实际可调用”的工具表面：

```text
Agent Tool Policy
+ Permission / Runtime Capability
+ Tool Registry
  -> FinalizedToolset
  -> ToolSurface {
       definitions,
       dispatch,
     }
```

当前 OpenWork 已用 `FinalizedToolset` 保证 Definitions 和 Dispatch 来自同一份结果，只是尚未把它命名成独立的 `ToolSurface` 类型。

这条链可以做权限过滤、能力选择和确定性排序；它不读取 Conversation，不加载 System Context，也不构造 `ModelRequest`。未来 Memory Search、Plan Update、Skill Load 或 MCP 工具都通过这条链决定本次是否对模型可见。

### 3.4 Central Model Request Assembly

请求组装不是第四条物化链。它是三条链之后的集中边界，负责把已经物化的结果组装成协议中立请求：

```text
ResolvedSystemContext
+ ConversationView
+ ToolSurface.definitions
+ Resolved Model / Generation Settings
  -> ModelRequest
```

它可以执行以下确定性工作：

- 校验 System Context key 非空且不重复；
- 保证 Conversation 中不出现 System Message；
- 固定 System Context 在 Conversation 之前；
- 计算只读 Context Budget Estimate；
- 生成 `openwork_models::ModelRequest`。

它不能读取项目文件、查询 Memory、修改 Chat State、选择或推进 Tool Call，也不能调用 Provider。

Provider Adapter 位于集中组装之后，只负责把 `ModelRequest` 编码成 OpenAI、Anthropic 或兼容协议。

## 4. OpenCode V2 与 grok-build 的重合关系

### 4.1 OpenCode V2

OpenCode V2 当前调用路径可以归纳为三条物化链在 `SessionRunner` 汇合：

```text
SystemContext Sources
  -> load / combine / reconcile
  -> SessionContextEpoch baseline + snapshot

Persisted Session Messages
  -> SessionHistory
  -> toLLMMessages

Tool Registry + Agent Permissions
  -> tools.materialize
  -> definitions + execution snapshot

System + Messages + Materialized Tools + Model
  -> LLM.request
  -> Provider
```

对应源码：

- `packages/core/src/system-context/index.ts`：System Context Source 的读取和渲染；
- `packages/core/src/session/context-epoch.ts`：System Context baseline 的准备；
- `packages/core/src/session/history.ts`：Conversation History 选择；
- `packages/core/src/session/runner/to-llm-message.ts`：Session Message 到模型消息的转换；
- `packages/core/src/session/runner/llm.ts`：分别加载 System、History 和 Materialized Tools，再组合请求。

其中 `SystemContext.Source` 的重点不只是“加载一段文本”。它用稳定 key 标识来源，用 codec 保存可比较状态，用 `baseline/update/removed` 表达模型可见变化，再由 `SessionContextEpoch` 保存 baseline 与 snapshot。这样压缩、恢复或来源变化时，不需要把所有 System 输入混成一段无法追踪来源的字符串。

OpenWork 采用这一生命周期方向，但分阶段落地：V1 保持具体的 `ProjectInstructionLoader`；直到出现动态来源或跨进程恢复需求，再引入 snapshot/epoch 和 update/removal。本文不要求现在照搬 OpenCode 的 Effect、Schema、SQL 表或 Registry。

### 4.2 grok-build

grok-build 也有独立的 System Prompt 收集链：

```text
AgentBuilder
  -> 收集模板、AGENTS.md 和环境输入
  -> PromptContext
  -> render
  -> System Prompt
```

对应源码：

- `crates/codegen/xai-grok-agent/src/builder.rs`：发现并收集输入；
- `crates/codegen/xai-grok-agent/src/prompt/context.rs`：结构化 `PromptContext`；
- `PromptContext::render`：生成模型可见 System Prompt。

但它对 Conversation 和请求的所有权与 OpenCode 不同：

```text
ChatStateActor.items
  -> leading System
  -> stable synthetic User-role prefix
  -> real User / Assistant / Tool history
  -> build_conversation_request
  -> ConversationRequest
```

对应源码为 `crates/codegen/xai-chat-state/src/actor/request_builder.rs`。

grok-build 对本文最有价值的是四个约束：

1. **Synthetic provenance**：Project Instructions、System Reminder、Compaction Meta、Auto Continue 等虽然可能使用 User role，但带有 `SyntheticReason`，供压缩、rewind、统计和恢复逻辑与真实用户输入区分；
2. **固定前缀**：leading System、环境前缀和 Project Instructions 等按稳定顺序出现，未变化时尽量保持字节不变；
3. **压缩恢复**：压缩不只生成摘要，还重新建立继续任务所需的 System、Project Instructions、最后真实用户请求、近期消息和各类运行提示，并把新 Conversation 原子写回 Chat State；
4. **Prompt Cache 稳定性**：请求构建和裁剪逻辑显式避免每轮重写早期前缀，只有达到阈值或来源确实变化时才替换，从而延长 cache-warm 区间。

OpenWork 借鉴这些性质，不复制它们在 grok-build 中的存放位置：System Context 不需要为了“稳定前缀”写进 Chat State，完整 `ConversationRequest` 也不需要回到 Chat State 构造。

### 4.3 两者不是两套并列方案

三者可以放在同一张职责表中：

| 职责 | OpenCode V2 | grok-build | OpenWork 当前 |
| --- | --- | --- | --- |
| System Context 物化 | `SystemContext` + `SessionContextEpoch` | `AgentBuilder` + `PromptContext`，并把 leading System 放入 Chat State | `SystemContextBuilder` + `ProjectInstructionLoader` |
| Conversation 物化 | `SessionHistory` + `toLLMMessages` | `ChatStateActor.items`，包含 synthetic items | `openwork-chat-state` + `ConversationView` |
| Tool Surface 物化 | `tools.materialize` | Agent/ToolState 后进入请求 | `FinalizedToolset` |
| 单次请求组装 | `SessionRunner` 调用 `LLM.request` | Chat State 的 `build_conversation_request` | Core 的 `ModelRequestBuilder` |
| Provider 协议转换 | `@opencode-ai/llm` routes | sampler/backend | `openwork-models` adapters |
| 来源变化/恢复 | System snapshot/epoch + Session events | synthetic provenance + compacted history replacement | 仅有 Turn 内 System 复用；其他尚未实现 |

所以正确做法不是同时保留“OpenCode 三层”和“grok-build 两阶段”两套结构，而是确定一套 OpenWork 结构：

```text
OpenCode-style ownership and lifecycle
  + grok-build-style provenance, recovery, prefix stability
  + OpenWork centralized request assembly in Core
```

### 4.4 明确采用与不采用

| 主题 | OpenWork 采用 | OpenWork 不采用 |
| --- | --- | --- |
| 三条链 | System、Conversation、Tool Surface 分别物化 | 把所有输入先塞入一个通用 `ContextBundle` |
| 来源生命周期 | 稳定身份、权威来源、baseline、显式变化/移除；需要时再持久化 snapshot/epoch | V1 立即建立通用 Source Registry 或复制 OpenCode 的全部类型 |
| Synthetic provenance | Conversation 内系统生成条目有结构化来源标记 | 仅靠 `<system-reminder>` 文本猜测条目来源 |
| 固定前缀 | 顺序稳定、未变化内容保持字节稳定 | 为了缓存把所有 System 输入永久写入 Conversation |
| 压缩恢复 | 压缩 Conversation，随后重新物化三条链并集中组装请求 | 让摘要承担恢复 `AGENTS.md`、Tools、Memory 等全部职责 |
| 请求组装 | Core 的 `ModelRequestBuilder` 是唯一集中组装点 | grok-build 式把完整请求构造整体搬回 Chat State |

## 5. OpenWork 当前实现

### 5.1 实际调用路径

当前代码已经按以下顺序执行：

```text
TurnRunner::run_loop
  -> SystemContextBuilder::build
       -> Agent system prompt
       -> ProjectInstructionLoader
       -> ResolvedSystemContext

  -> for each Model Call
       -> ChatStateHandle::conversation_view
       -> FinalizedToolset::definitions
       -> ModelRequestBuilder::build
            -> ContextBudgetEstimate::measure
            -> ModelRequest
       -> ModelPort::invoke
       -> Provider Adapter
```

因此，“先收集上下文，再生成模型请求”已经存在：

- `SystemContextBuilder::build` 在 Turn 的模型循环之前执行；
- `ConversationView` 在每次 Model Call 前读取；
- `FinalizedToolset` 是本次 Session 已完成权限与配置选择的 Tool Surface；
- `ModelRequestBuilder` 是 Provider 调用前的唯一集中入口；
- Chat State 已经不再接收 Model、System Prompt 和 Tools 来构造完整请求。

### 5.2 当前 System Context 内容

目前只有两个真实来源：

```text
1. AgentSystem
   key: core/agent-system
   value: selected Agent 的 system prompt

2. ProjectInstruction
   key: project/AGENTS.md
   value: <working_directory>/AGENTS.md
```

`ProjectInstructionLoader` 当前行为：

- 只读取 Session working directory 根部的 `AGENTS.md`；
- 不向父目录或子目录搜索；
- 文件缺失时不产生内容；
- 空白文件不产生内容；
- 拒绝符号链接；
- 上限为 64 KiB。

### 5.3 当前 Conversation

`openwork-chat-state` 是 Conversation 单写者。`ConversationView` 当前只包含：

```rust
struct ConversationView {
    messages: Vec<Message>,
}
```

它不包含 System Context、Tools、Model Settings 或预算信息。

### 5.4 当前 Tool Surface

`FinalizedToolset` 同时提供：

- 发给模型的 Tool Definitions；
- Tool Call 对应的实际 Dispatch。

每次请求从同一份 finalized tool set 取得 Definitions，避免模型看到无法执行的工具。

### 5.5 当前预算估算

`ContextBudgetEstimate` 使用统一的 `bytes / 4` 近似值测量：

- System Context；
- Conversation；
- Tool Definitions；
- 总输入。

当前预算只写入 Trace，用于观测。它没有权威 Context Window 上限，因此不会拒绝请求，也不会改变任何输入。

桌面端的 Context Window 大小是独立的 OpenWork 应用配置，不属于 Provider 或 Model 元数据。当前该配置只作为用量环的分母，与最近一次 Model Call 的输入 Tokens 组合展示；它尚未传入 Core 执行拒绝、裁剪或压缩。未来启用预算治理时，应在 Turn 启动边界把同一配置传给 Core，而不是从模型 ID 推导另一份窗口大小。

### 5.6 当前实现与目标边界

| 边界 | 当前已实现 | 目标中尚未实现 |
| --- | --- | --- |
| System Context | Agent System + 根部 `AGENTS.md`，Turn 内复用 | 通用来源生命周期、snapshot/epoch、update/removal |
| Conversation | 已提交消息的一致 `ConversationView` | synthetic provenance、checkpoint + recent tail 投影、压缩原子替换 |
| Tool Surface | `FinalizedToolset` 同时提供 definitions 与 dispatch | 跨恢复的能力版本；当前不增加空壳 `ToolSurface` 类型 |
| Request Assembly | Core 集中构造 provider-neutral `ModelRequest` | 无；这个所有权边界保持不变 |
| Prompt Cache | Turn 内 System 结果复用、请求顺序固定 | 跨 Turn/压缩/恢复的稳定 generation 与字节一致性验证 |

这个表是当前代码与目标架构的分界。后文描述的 provenance、epoch 和 compaction recovery 都不能被当作已经落地的行为。

## 6. 本轮已经完成的概念收敛

### 6.1 System Context 名称与范围

当前代码使用：

```text
SystemContextBuilder
  -> ResolvedSystemContext
       -> Vec<SystemContextPart>
```

`ResolvedSystemContext` 只保存本 Turn 已解析的 System 输入，不包含 Conversation 或 Tool Surface。`SystemContextPart` 只包含稳定 source key 和模型可见 content；已删除不参与当前行为的通用 `ContextKind` 和排序规则。Agent System、Project Instructions 的顺序由 `SystemContextBuilder` 显式决定。

### 6.2 请求组装只保留一个入口

当前路径已收敛为：

```text
ModelRequestInput
  -> ModelRequestBuilder
  -> PreparedModelCall {
       request,
       context_budget,
     }
```

校验、预算测量和 provider-neutral `ModelRequest` 构造都在 `ModelRequestBuilder::build` 内完成。`PreparedModelCall` 表示同一次构造产生的请求和预算测量，不是 Agent 的 Plan。

### 6.3 预算只对应三块真实输入

`ContextBudgetEstimate` 当前只包含：

```text
system_context_tokens
conversation_tokens
tool_surface_tokens
estimated_input_tokens
+ reserved_output_tokens
```

旧的 Session Context、Turn Context、Turn Reminder 等空分项已经删除。预算仍只是观测值，不执行拒绝、裁剪或压缩。

## 7. 当前结构与未来扩展接缝

```text
openwork-agent / openwork-core::context
  -> SystemContextBuilder
  -> ResolvedSystemContext

openwork-chat-state
  -> ConversationView

FinalizedToolset
  -> definitions + dispatch

openwork-core::model_call
  -> ModelRequestBuilder
  -> PreparedModelCall
```

运行路径为：

```text
SystemContextBuilder::build
  -> ResolvedSystemContext

ChatStateHandle::conversation_view
  -> ConversationView

FinalizedToolset::definitions
  -> Tool Definitions

ResolvedSystemContext
+ ConversationView
+ Tool Definitions
+ Resolved Model
  -> ModelRequestBuilder::build
  -> PreparedModelCall
  -> ModelPort::invoke
```

这保留四个稳定边界：

1. System Context 如何从来源物化；
2. Conversation 由谁拥有并如何物化；
3. Tool Surface 如何从能力和权限物化；
4. 三条物化结果在哪里集中组装成 provider-neutral request。

第四项是组装边界，不是第四条 Context 链。

### 7.1 来源生命周期放在哪里

来源生命周期只描述“某个权威来源如何稳定地变成模型可见内容”，不放进 Chat State，也不由 `ModelRequestBuilder` 管理：

```text
Source Identity
  -> load authoritative state
  -> compare with admitted state（需要时）
  -> render baseline / update / removal
  -> materialized output
```

对当前 `AGENTS.md`，权威来源是文件，`ProjectInstructionLoader` 负责读取，`SystemContextBuilder` 负责顺序和渲染。未来真的需要 resume、动态更新或 Prompt Cache generation 时，再在 System 链内增加 snapshot/epoch。

生命周期状态不应塞进 `SystemContextPart`：

- `SystemContextPart` 是本次调用的模型可见结果；
- source snapshot/epoch 是跨调用比较与恢复状态；
- budget 是对最终物化结果的派生测量。

三者生命周期不同，分开后不会因为加入 Memory 或 Compaction 就反复修改同一个“大而全上下文容器”。

### 7.2 Synthetic provenance 放在哪里

Synthetic provenance 只用于 Conversation 链内“协议角色像 User/Assistant，但语义上不是对应人类或模型自然产物”的条目。例如：

```text
SyntheticConversationItem {
  message,
  reason: ProjectInstructions | SystemReminder | CompactionCheckpoint | AutoContinue | ...
}
```

这里展示的是未来语义，不是要求现在立刻添加该 Rust 枚举。真正实现时，provenance 必须是结构化元数据，不能只靠解析 `<system-reminder>` 字符串。它至少用于：

- 判断最后一个真实用户请求；
- 压缩时跳过或重建可再生提示；
- resume/rewind 时去重；
- UI 和统计区分用户输入与系统注入。

如果 Project Instructions 按 OpenWork 当前设计进入 System Context，它使用 System source key 追踪来源，不需要再复制一份 synthetic Conversation item。借鉴 grok-build 的是 provenance 机制，不是把它的每一种 role/layout 原样照搬。

### 7.3 固定前缀与 Prompt Cache 稳定性

“固定前缀”是请求级不变量，不等于创建一个通用 `prefix: String`，也不等于把整个前缀持久化到 Chat State。

目标稳定结构为：

```text
Stable Message Prefix
├── System Context parts（稳定 source order）
└── Conversation 中已接纳的 checkpoint / stable synthetic items

Changing Message Tail
├── recent Conversation
├── current User input
└── turn-ephemeral synthetic items

Stable Tool Snapshot
└── 稳定顺序的 Tool Definitions
```

Tool Definitions 在 provider-neutral request 中是独立字段，不与 messages 混成一条逻辑序列；某个 Provider 是否把它们计入 Prompt Cache prefix，由 Adapter/Provider 决定。OpenWork 只保证相同 Tool Surface 产生稳定定义和顺序。

稳定性规则：

1. 同一已接纳 generation 中，来源未变化就复用完全相同的渲染文本和顺序；
2. 同一 Turn 的 Tool 循环不重新读取和改写稳定 System 前缀；
3. synthetic item 使用稳定标识去重，不在每次 resume 时重复插入；
4. 裁剪或压缩采用明显的低水位/高水位策略，避免每轮改写最早消息；
5. 来源确实变化时，通过显式 update 或新 generation 变化，不为了 cache 隐藏真实变化。

V1 当前只做到第 2 项和基本确定性顺序。跨 Turn、压缩和恢复的 cache 稳定性要等 source lifecycle 与 compaction 实现后再验证。

### 7.4 压缩与恢复

压缩的直接改写对象是 Conversation 物化链，不是完整 Context Window：

```text
Full Conversation
  -> select compaction boundary
  -> summarize old history
  -> persist checkpoint + boundary
  -> atomically install checkpoint + recent tail
  -> new ConversationView
```

但压缩后的下一次模型调用必须重新得到完整请求，因此恢复协调器要重新汇合三条链：

```text
System Sources  ──materialize──> ResolvedSystemContext ─┐
                                                       │
Conversation State ─materialize──> ConversationView ───┼─> ModelRequestBuilder
                                                       │
Tool Capability ─materialize──> FinalizedToolset ─────────┘
```

这意味着：

- 摘要只恢复不可再生的历史语义，不复制 `AGENTS.md`、Tool Definitions 或稳定 Memory baseline；
- System、Plan、Skill、Memory 和 Tools 分别从自己的权威来源重新物化；
- 同一轮因超限触发压缩后，如果 System/Tool 来源没有变化，可以复用压缩前已经物化的结果，以保持字节稳定；
- 跨进程恢复时则从持久化的来源状态和 checkpoint 重建，不把 `ConversationView` 当成所有功能的数据库；
- Core 的 Compaction/Recovery Coordinator 负责协调，Chat State 只负责 Conversation 的一致快照与原子替换；
- 最终请求仍由 `ModelRequestBuilder` 构造。

所以“压缩只管 Chat State”只在“被缩短的数据是 Conversation”这一层成立；对下一次调用而言，仍要重新物化 System、Conversation、Tool Surface 三条链。

### 7.5 Memory、Plan、Skill 以后填到哪里

一个功能不是只能属于一个输入区。它的权威状态保留在自己的模块中，不同的模型可见投影进入不同物化链：

| 功能 | System Context | Conversation | Tool Surface | 权威状态 |
| --- | --- | --- | --- | --- |
| Memory | 稳定用户偏好或长期规则的 baseline | 当前 query-based recall，带 synthetic provenance | Memory search/write tools | Memory service/store |
| Plan | 稳定的 plan-mode 规则 | 当前计划、进度或提醒，带 synthetic provenance | plan read/update tools | Plan state/store |
| Skill | skill catalog / 使用规则 | 已加载 skill 内容或 tool result | skill discovery/load tools | Skill registry/files |
| Compaction | 不存摘要 | checkpoint summary + recent tail | 可选 compaction tool | Conversation log + checkpoint metadata |
| Reminder | 通常不进入稳定 baseline | turn-ephemeral synthetic item | 无 | 产生 reminder 的功能模块 |

这里的关键不是提前决定每个未来类型，而是固定路由规则：

- 稳定且具有约束性质的模型输入，进入 System Context 物化；
- 与当前任务、查询或历史时序相关的模型输入，进入 Conversation 物化；
- 模型可调用的能力，进入 Tool Surface 物化；
- 业务真相留在功能自己的存储中；
- 三条物化结果只在 Core 的 `ModelRequestBuilder` 汇合。

## 8. 模块所有权

### 8.1 `openwork-agent`

拥有静态 Agent Definition：

- Agent System Prompt；
- 允许的 Tool Set；
- Model Call 上限和静态策略。

它不读取 `AGENTS.md`，不拥有 Conversation，也不调用 Provider。

### 8.2 `openwork-chat-state`

拥有：

- Conversation 单写；
- Draft 生命周期；
- User / Assistant / Tool Message；
- `ConversationView` 一致快照；
- 未来 Conversation 内 synthetic provenance、compaction checkpoint 投影和原子替换。

它不加载 System Context、Memory、Plan 或 Skill 的权威来源，不接收 Model/Tools 来构造完整请求。

### 8.3 `openwork-core::context`

拥有：

- Agent System Prompt 与 Project Instructions 的显式编排；
- `AGENTS.md` 文件读取边界；
- System Context 的顺序和来源标识；
- 未来 System Source 的 baseline、update/removal 与 snapshot/epoch 生命周期。

它不读取 Conversation，不构造 Provider 请求。

### 8.4 `openwork-core::model_call`

拥有：

- System Context、Conversation、Tools、Model 的最终组合；
- 请求前确定性校验；
- 只读预算估算；
- `ModelRequest` 构造。

它不推进 Turn，不执行 Tools，不直接编码 Provider 协议。

### 8.5 `openwork-core` 的未来恢复协调器

Compaction 真正实现后，Core 应拥有协调流程：

- 读取稳定的 Conversation snapshot；
- 调用 summarizer；
- 持久化 checkpoint 与边界；
- 请求 Chat State 原子安装压缩后的 Conversation；
- 重新物化 System、Conversation 和 Tool Surface；
- 继续通过 `ModelRequestBuilder` 发起调用。

协调器不应把摘要逻辑、System Source 加载或 Tool Materialization 下沉到 Chat State。

### 8.6 `openwork-models`

拥有：

- Provider-neutral `ModelRequest`；
- `ModelPort`；
- OpenAI/Anthropic 等 Provider Adapter。

Adapter 不知道 `AGENTS.md` 路径、Chat State 或 System Context 来源。

## 9. 不变量

### 9.1 当前已经成立

1. `SessionActor` 是唯一推进 Model -> Tool/Permission -> Model 循环的地方；
2. Chat State 是 Conversation 单写者；
3. System Context 不写入 Conversation；
4. `ConversationView` 不允许包含 System Message；
5. Agent System Prompt 始终位于 Project Instructions 之前；
6. System Context 始终位于 Conversation 之前；
7. Tool Definitions 和实际 Dispatch 来自同一 `FinalizedToolset`；
8. Provider Adapter 不加载 Context Source；
9. Context Budget 当前只测量，不改变请求；
10. Trace 只记录诊断信息，不参与请求构建或 Turn 推进。

### 9.2 未来能力必须继续满足

1. System Context、Conversation、Tool Surface 分别物化，只在 Core 的请求构造边界汇合；
2. Chat State 可以拥有 synthetic Conversation item，但不拥有其上游 Memory、Plan、Skill 或 Project Instruction 来源；
3. 系统生成的 User-role item 必须带结构化 provenance，不能与真实用户输入混淆；
4. Compaction 只替换 Conversation 投影，不能让摘要成为 System Source 或 Tool Surface 的权威副本；
5. Compaction/resume 后必须重新物化三条链，再构造完整请求；
6. 来源与顺序未变化时，稳定前缀应保持字节一致；来源变化时必须显式产生 update 或新 generation；
7. Tool Definitions 与 Dispatch 始终来自同一次 Tool Surface 物化；
8. `ModelRequestBuilder` 保持纯组装，不因加入 Memory、Plan、Skill 或 Compaction 而读取它们的存储。

## 10. 验证范围

当前分层应通过以下测试证明：

### 10.1 System Context

- Agent System Prompt 在 Project Instructions 之前；
- 根部 `AGENTS.md` 存在时被加载；
- 缺失或空白时不会产生空 System Message；
- 符号链接和超过 64 KiB 的文件被拒绝；
- 同一个 key 重复时确定性失败。

### 10.2 Conversation

- Chat State 返回完整已提交 Conversation；
- `ConversationView` 不包含 Draft；
- Assistant Message 和 Tool Result 写入后会进入下一次 Model Call；
- Chat State 不接收 Model、System Prompt 或 Tool Definitions。

### 10.3 Tool Surface

- Tool Definitions 与 Dispatch 来自同一 `FinalizedToolset`；
- 权限过滤后不可见的工具既不广告也不可执行；
- 工具定义顺序在输入未变化时保持确定。

### 10.4 Request Assembly

- System Context、Conversation 和 Tools 按固定边界进入 `ModelRequest`；
- Conversation 中出现 System Message 时失败；
- `ModelRequest` 保持 Provider-neutral；
- OpenAI 与 Anthropic Adapter 的请求编码回归测试继续通过；
- Budget Estimate 与实际准备的同一份请求对应。

### 10.5 当前回归

- 无 `AGENTS.md` 时，发送给模型的有效内容与重构前一致；
- 有 `AGENTS.md` 时，只新增预期的 Project Instruction；
- Model -> Tool -> Model 多调用链保持不变；
- Permission、Doom-loop、Cancellation、Trace 和 Storage 行为不变。

### 10.6 未来能力落地时新增的契约测试

- 同一 source generation 在普通调用、压缩后重试和 resume 后产生相同稳定前缀；
- source 变化或移除时只产生预期 update/new generation，不静默复用旧内容；
- synthetic User-role item 不会被识别成最后一个真实用户请求；
- 同一 synthetic item 在 resume/rewind 后不会重复注入；
- 压缩后 Conversation 变为 checkpoint + recent tail，而 System Context 和 Tool Surface 从各自来源重新物化；
- 压缩前后未变化的 System 和 Tool 前缀保持字节一致；
- Chat State API 仍不接收 Model、System Sources 或 Tool Definitions 来构造完整请求。

## 11. 当前不实现，但已定义兼容边界

本文不会仅为了未来可能性就预建以下能力的字段、枚举、Registry、数据库表或状态机：

- Memory；
- Plan；
- Skill；
- Compaction；
- MCP；
- Plugin；
- Context Epoch；
- 跨进程 Turn 恢复。

但这不再表示“以后再决定整体结构”。第 7 节已经固定它们的兼容边界：权威状态留在各自模块，模型可见内容进入对应物化链，压缩后重建三条链，最终请求继续由 Core 组装。

以后实现这些能力时仍会新增具体接口和持久化结构，但不应修改以下架构骨架：

```text
System Materialization
+ Conversation Materialization
+ Tool Surface Materialization
  -> Central ModelRequestBuilder
```

因此“无需修改整体结构”指不再搬迁所有权和请求组装边界，不是指未来完全不增加类型或接口。

## 12. 当前结论与后续代码范围

当前已经完成：

- Chat State 只返回 `ConversationView`，不再构造完整 `ModelRequest`；
- `ProjectInstructionLoader` 加载 Session 根部 `AGENTS.md`；
- `SystemContextBuilder` 在 Turn 开始时生成 `ResolvedSystemContext`；
- `FinalizedToolset` 同时提供模型可见 definitions 与实际 dispatch；
- `ModelRequestBuilder` 集中组装请求并生成只读预算估算；
- 预算字段已收敛为 System Context、Conversation、Tool Surface 和总输入；
- Provider Adapter 继续只负责协议编码。

本轮没有新增 Source Registry、Context Epoch、synthetic message、Compaction 状态机或 `ToolSurface` 空壳类型，也没有改变现有 Session、Tool、Provider 和 Storage 所有权。

未来功能按需求分阶段加入：

1. 第一个需要跨调用追踪的动态 System Source 出现时，引入最小 source lifecycle；
2. 第一个系统生成且需要持久化/重放的 Conversation item 出现时，引入 synthetic provenance；
3. 实现 Compaction 时，再加入 checkpoint、Conversation 原子替换和 Core recovery coordinator；
4. 实现跨进程恢复或稳定 cache generation 时，再持久化 source snapshot/epoch；
5. Memory、Plan、Skill 分别从自己的权威状态向三条链投影，不改回 Chat State 完整请求组装。

最终原则可以压缩为一句话：

> OpenCode 决定三条物化链和来源生命周期；grok-build 提供 provenance、固定前缀、压缩恢复和 Prompt Cache 稳定性；OpenWork 的 Core 继续集中构造最终请求，Chat State 只负责 Conversation。
