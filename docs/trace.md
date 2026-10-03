# Trace

Trace 回答**「模型看到了什么、说了什么、用了多少 token、人怎么评价」**。它是质量追踪，不是运维埋点。

Trace 写入仍是 best-effort。丢失一条 Span 只会让排查更难，不改变任何业务结果。

## 1. 定位

早期版本把 Trace 当作运维 trace。它记录字节数、重试轨迹和持久化耗时，用来回答「runtime 是不是在正常工作」。这个定位是错的。

这个定位有两处错误。**一是收益前提不成立。** 运维型 trace 的价值来自规模：成千上万个用户在运行时，需要知道哪个 Provider 不稳定。OpenWork 是本地单机应用，没有这种规模。**二是答不了用户真正会问的问题。** 用户会问「这次回答为什么不对」，字节数和重试次数都回答不了这个问题。

**Trace 能回答：**

| 问题 | 靠什么 |
|---|---|
| 模型这次收到的完整请求是什么 | `request` / `system_context` / `tool_definitions` 三个正文槽位 |
| 模型回复了什么 | `response_message_id` 指向的 Message；失败时用 `response` 槽位 |
| 用了多少 token | token 四列，Span 与 Turn 两级 |
| 用什么参数调用的 | `temperature` / `topP` / `maxOutputTokens` / `thinkingMode` |
| 这次结果好不好 | `trace_annotations` |
| 哪里慢、哪里出错 | 分段耗时、`errorPhase`、`deliveryState` |
| 压缩为什么触发、节省了多少 | Compaction Span 的触发证据与前后 token |

**Trace 不能做：**

- 恢复未完成的 Turn；
- 推导 Tool Call 的副作用是否发生；
- 决定是否重试；
- 让 Turn 失败。

PostgreSQL 中的领域 Trace 是本地产品契约。Rust `tracing` 只做运行时 instrumentation。OpenTelemetry/OTLP 只是未来的可选出口。**三者不能互相替代。** `attempt_count`、`denied`、`outcome_unknown`、Complete/Partial 都是 OpenWork 的领域语义。类型化 Guard 必须显式产生这些值，不能靠 Subscriber 反推。

## 2. 三层标识

Trace 只有三个**结构标识**，其余标识都是业务标签：

| 标识 | 作用 | 外键 | 可空 |
|---|---|---|---|
| `trace_id` | **结构根**。一次用户请求的全部 Span 共享它；一次无 Turn 的独立操作也一样 | 无 | 否 |
| `id` | Span 自身 | 主键 | 否 |
| `parent_span_id` | **发起关系**：父 Span 发起了子 Span | **有意不建** | 是 |

`session_id` 和 `turn_id` 是**业务标签**。`turn_id` 表示"这个操作是否发生在某个 Agent Loop 内部"。手动压缩和 rewind 不在 Agent Loop 内部，所以它们的 `turn_id` 为空。

**判别法：** 一件事没有对应的业务行时，能不能直接给它分配一个这样的 id？`trace_id` 能：手动压缩没有 Turn，也会开一个 `trace_id`。`turn_id` 不能：分配前要先往 `turns` 插一行，而那一行要求 `client_request_id`、`sequence > 0` 和 `resolved_provider_kind`。

由此得出外键规则：**结构标识不建外键，业务标签建外键。** 结构标识必须能指向"可能不存在"的东西。例如，队列满时，父 Span 可能根本没有落库。业务标签指向真实的业务行。业务标签要建外键，还要随业务生命周期级联删除。

### `trace_id` 由谁生成

| 场景 | 取值 |
|---|---|
| Turn 内的任何 Span | **等于该 Turn 的 `turn_id`** |
| 手动压缩、rewind | 新生成 `trace-<uuid>` |
| 任何子 Span | **继承父 Span 的 `trace_id`**，不重新生成 |

Turn 内直接复用 `turn_id`，因为两者一一对应，这样可以省去一次映射。**这不表示两者语义相同。** `turn_id` 仍可为空并带外键；`trace_id` 必填且没有外键。

`trace_id` 在**操作开始时**生成。发起该操作的执行体分配它：`TurnRunner` 在 Turn 开始时分配，`SessionActor` 在手动压缩或 rewind 开始时分配。一次操作中途不得更换 `trace_id`。

## 3. 形状

```text
trace_id = turn-abc                    ← 一次用户请求
├── Compaction（threshold，turn_id = turn-abc）
│   └── Model Call（摘要采样，parent = compaction）
├── Model Call #1（parent = NULL）
│   ├── Tool Call: read_file（parent = Model Call #1）
│   └── Tool Call: bash（parent = Model Call #1）
├── Model Call #2（可先失败于 overflow）
├── Compaction（overflow，triggerModelSpanId → Model Call #2）
└── Model Call #3

trace_id = trace-xyz                   ← 一次手动压缩，没有 Turn
└── Compaction（turn_id = NULL）
    └── Model Call（摘要采样）
```

**一条规则**决定 Span 之间的关系：

> `parent_span_id` 表示"**谁发起了谁**"，不是"谁包含谁"。

| 关系 | 为什么 |
|---|---|
| Tool Call 的父是那次 Model Call | 模型在那次响应里明确请求了这次 Tool Call |
| 摘要采样的父是 Compaction | 压缩流程发起了这次采样 |
| Compaction 没有父 | 它是 Core 的策略决定，模型和工具都没有请求它。把它挂到某个 Model Call 下会给出错误的关系 |
| overflow 的失败 Model Call **不是**父 | 它只**导致**压缩，没有**发起**压缩。它的 id 记在 `attributes.triggerModelSpanId` |

**这条规则有意不同于 OTel 惯例。** 在 OTel 里，父 Span 通常在时间上包含子 Span。这里的 Tool Span 在 Model Span 关闭之后才开始，因为 `call_model` 先 finish 再返回。四种情形里，只有"摘要采样在 Compaction 内部"是真正的时间嵌套。

选择因果语义的理由如下。`started_at` 已经能表达时间关系。但"哪次模型响应要求了这次 Tool Call"没有别的地方可以记录。并发 Tool Call 的时间区间互相重叠，无法从区间反推出这个关系。

### 没有根 Span

一条 Trace 的顶层是若干平级 Span，没有代表整次请求的根 Span。要看"用户问了什么、最终回答了什么"，读 `turns` 中的那一行和它的首尾 Message。

理由见 §6：那两段内容已经是业务真相，Trace 不复制它们。

## 4. 不设 `sequence`，按 `started_at` 排序

`started_at` 决定 Span 之间的先后，`id` 作为 tiebreak。**不设 `sequence` 列，也不设 `UNIQUE (turn_id, sequence)`。**

序号方案在单条串行 Agent Loop 下可行。但它把"同一时刻只有一个执行体在写"编码进了唯一约束。第二个执行体（并发工具、subagent、后台任务）向同一 Trace 写入时，两个独立分配的序号必然冲突。Trace writer 的冲突子句只覆盖主键，所以唯一约束冲突会让整个批次事务回滚。**一次最多丢失 64 条 Span，只增加一个计数器，不重试也不告警。**

删掉序号就消除了这类故障。代价只是失去"写入顺序"这一项冗余信息，而 `started_at` 已经表达了它。

## 5. 一次 Turn 记几行

例：2 次 Model Call、3 次 Tool Call、1 次 threshold 压缩，摘要一次成功：

```text
trace_id = turn-abc
├── compaction-1        kind=compaction   parent=NULL
│   └── span-model-s1   kind=model_call   parent=compaction-1
├── span-model-1        kind=model_call   parent=NULL
│   ├── span-tool-1     kind=tool_call    parent=span-model-1
│   ├── span-tool-2     kind=tool_call    parent=span-model-1
│   └── span-tool-3     kind=tool_call    parent=span-model-1
└── span-model-2        kind=model_call   parent=NULL
```

**7 行。** 摘要重试 3 次则是 9 行。

分界线是：**重发同一个请求 = 属性；发起一个新请求 = 新行。**

| 产生一行 | 不产生一行 |
|---|---|
| 每次逻辑 Model Call | Turn 本身（`turns` 表已经是它） |
| 每次 Tool Call | Transport 重试（只增加 `attempt_count` 列） |
| 每次压缩 | 权限等待（记在 `permission_wait_ms` 列） |
| 压缩内每次摘要采样 | |

每次摘要重试都重新构造并提交一个独立请求，各有自己的 `provider_request_id` 和 usage，所以每次是一行。transport 重试重发同一个 payload，所以是属性。

注意计数的后果。上例中 `COUNT(*) WHERE kind='model_call'` 是 **3**，而不是 2。所以计算完整度时必须加 `AND parent_span_id IS NULL`（见 §12）。

### 摘要采样为什么必须是子 Span

如果把它们压平成一个 JSON 数组，token 统计就会漏掉一部分开销：

```sql
SELECT sum(input_tokens) FROM trace_spans WHERE kind = 'model_call'
```

这条查询算不出会话的真实开销。压缩使用的 token 在另一个 kind 里，而且失败尝试的 usage 没有地方落库。改成子 Span 后，每次采样都有自己的行和 token 列。

## 6. 内容

本节是质量追踪的核心，也是与早期设计差别最大的一节。

### 只记 `messages` 回答不了的

> **凡是已经落进 `messages` 或 `conversation_compactions` 的内容，Trace 不复制，只留指针。**

理由不是节省空间，而是**避免同一份内容出现两个可能不一致的版本**。`messages` 是业务真相，只增不改不删，永远比 Trace 完整。如果再复制一份到 best-effort 的表里，两边一旦不一致，就无法判断该信哪一边。

| 内容 | 在哪 | Trace 怎么做 |
|---|---|---|
| 用户输入 | `messages`（role=user） | 不记 |
| 成功调用的响应 | `messages`（role=assistant） | 记 `response_message_id` 指针 |
| 工具参数 | assistant message 的 tool_use 块 | 不记 |
| 工具结果 | `messages`（role=tool），`(turn_id, provider_call_id)` 唯一确定它 | 不记 |
| 成功的摘要 | `conversation_compactions.summary` | 记 `checkpointId` 指针 |
| **组装后的请求** | **不在任何地方** | **记 `request` 槽位** |
| **System Context** | **不在任何地方** | **记 `system_context` 槽位** |
| **工具定义** | **不在任何地方** | **记 `tool_definitions` 槽位** |
| **失败调用的响应** | **不在任何地方**（没有产生 Message） | **记 `response` 槽位** |

**组装结果是本节存在的全部理由。** 压缩之后，模型看到的 Conversation 和原始 `messages` 不再相同。它是 checkpoint 的摘要加上边界之后的原始消息，再加上 System Context 和 runtime reminder。这个投影结果是"模型实际看到了什么"的唯一答案，而且**没有任何别的表保存它**。

### 为什么不用描述符重建投影

理论上，可以从 `checkpoint_id + message 序号区间` 重建投影，这样几乎不占空间。**否决**：System Context 和 reminder 会随版本变化，重建出的是今天的组装结果，而不是当时的结果。排查"模型为什么答错"时，95% 忠实的重建比没有更糟：排查者会对着一份没人见过的输入找原因。

### 去重

`trace_payloads` 以内容哈希为主键，`trace_span_payloads` 把正文挂载到 Span 上。

在一个 Session 内，System Context 和工具定义几乎不变，但每次 Model Call 都重复发送它们。20 KB 的工具定义经过 50 个 Turn、400 次调用后，按行存储是 8 MB，去重后是 20 KB。

`request` 槽位无法去重，因为它每次都在变长。它是体积的主要来源。这是有意接受的代价，截断和保留策略限制它的体积（§14）。

`trace_payloads` **没有 `session_id`**。同样的工具定义在不同 Session 之间本来就相同，加上 `session_id` 等于放弃去重。代价是**删除 Session 不级联删除正文**，必须用孤儿清扫完成删除。见 §14：这是隐私相关的必做项，不是优化。

### 外键为什么这次可以建

`trace_span_payloads.span_id` 建了外键。这看起来和 §10「Trace 有损所以不建外键」矛盾，其实不矛盾：

- `parent_span_id` 指向**另一个** Span，那个 Span 可能单独丢失。建外键会把单点丢失放大成级联丢失；
- `trace_span_payloads.span_id` 指向**同一批写入的自己**，不可能指向不存在的行。

判别法：引用发生时，它指向的行有没有可能还不存在或已经丢失？

### 内容始终完整记录

Recorder 没有正文记录档位，始终处理全部四个支持的正文槽位。Desktop 不在本地持久化正文策略，也不在启动时同步正文策略。

始终完整记录的理由是**内容不出本机**。Postgres 在用户自己的机器上。所以云端 observability 厂商必须默认关闭内容记录，而 OpenWork 可以直接记录。

永远不记录以下内容：API Key、解密后的凭证、HTTP Header、未脱敏的 Provider 错误 Body。

## 7. Token 口径

Trace 记录 token，**不记录金额**。理由见 §18。

四个 token 列（`input_tokens` / `output_tokens` / `cached_input_tokens` / `reasoning_tokens`）的语义**跨 Provider 不统一**。聚合前要了解这一点。

### `cached_input_tokens` 是不是 `input_tokens` 的子集

| Provider kind | `input` 含 `cached` 吗 | 依据 |
|---|---:|---|
| `anthropic` | **否** | usage 把 `input_tokens`、`cache_read_input_tokens`、`cache_creation_input_tokens` 分成三个独立桶 |
| `openai` | 是 | `cached_tokens` 是 `prompt_tokens` 的明细 |
| `deepseek` | 是 | `prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens` |
| `qwen` | 是 | OpenAI-compatible 响应中 `cached_tokens` 是 `prompt_tokens` 的子集 |
| `glm` | 是 | 官方示例以 `cached_tokens / prompt_tokens` 计算命中比例 |
| `kimi` | **未验证** | 使用 OpenAI-compatible 解析，但没有查到官方对集合关系的明确说明。**不要当成已验证事实** |

**后果：`sum(input_tokens)` 跨 Provider 不可比。** Anthropic 的 input 不含缓存读取，其他 Provider 的含。计算"缓存命中率"或"总输入量"前，先按 `resolved_provider_kind` 分组。否则数字是错的，而且不会报错。

Anthropic 另有 `cache_creation_input_tokens`（写缓存）。它不在 `input_tokens` 里，也不是 `cached_input_tokens`。当前 adapter 不发送 `cache_control`，所以这个桶实际总是 0。解析层仍保留了它。

`reasoning_tokens` 是 `output_tokens` 的子集；`total_tokens` 定义为 `input + output`，不额外加缓存或推理 token。

> 这张表是开发成本功能时查到的。成本功能已经撤除，结论保留。它描述的是 token 语义，与计价无关。

## 8. 人工标注

`trace_annotations` 是**业务真相**，尽管它指向 best-effort 的 Span。丢失一条标注，就是丢失一条用户输入。

```sql
rating   good | bad | unsure
span_id  为空 = 评价整条 Trace，非空 = 评价其中一次调用
```

一个目标只有一条标注。修改评价用 upsert，不追加一条相反的标注。

由"它是业务真相"得出一条保留策略约束：**带标注的 Trace 不参与自动清理**（§14）。标为 `bad` 的调用最需要长期保留，因为它是将来做回归的样本。

**不做的**：自动打分、评测集、A/B 对照。它们需要一整套离线运行与对照基线，给 Span 加几列做不到。这里只提供一个信号：人的判断。

## 9. 什么时候才该新增一个 `kind`

门槛是**三条同时成立**：

1. 有自己的起止时刻和成败（不是一个状态字段）；
2. 不属于任何 Model Call 或 Tool Call：Core 自主发起它，模型不知道它发生了；
3. 可能脱离 Turn 发生。

`compaction` 满足全部三条。相反，**模型通过工具触发**的能力（Plan、Todo、Memory 的读写）对模型的接口就是工具。它们本来就是 `tool_call`，不需要新 kind。这些能力的接入位置见 [context-window.md](context-window.md)。

新增一个 kind 的实际成本：

| | 要动什么 |
|---|---|
| 1 | migration 改 `kind` 的 CHECK 与独占列的判别式约束 |
| 2 | 新的 `XxxTraceAttributesV1` 类型、版本与白名单 |
| 3 | 新 Guard，以及 `Drop` 后备处理 |
| 4 | 完整度语义决策：算不算 expected |
| 5 | **正文槽位决策**：它有没有 `messages` 回答不了的内容 |
| 6 | 前端 kind 分支（树、时间线、详情）、图标、颜色、属性白名单、**三语 i18n**（有测试强制） |
| 7 | **若它可能没有 Turn，必须同时给出读取入口**（见 §13） |

第 7 条最容易遗漏：Span 已经写入，但读取入口以 Turn 为根，结果只写不读。

**默认答案是"用现有 kind 表达"。** 如果某个能力将来变成"Core 自动进入某模式、自己运行一个多轮子流程"，正确形态更可能是**子 Session**（见 §16），而不是新 kind。

## 10. 有损写入决定的两条硬规则

Trace 是 best-effort：队列满时，`try_send` 直接丢弃 Span；批量写入失败时，丢弃整批。由此得出两条不可协商的规则：

1. **`parent_span_id` 不建外键。** 父 Span 丢失时，外键会让所有子 Span 的插入一起失败，把单点丢失放大成级联丢失。孤儿 Span 仍有独立的诊断价值。保存孤儿 Span，并把它计入采集缺口。
2. **Trace 写入失败不改变任何业务结果。** 队列满、数据库不可用、Flush 超时，都不能让 Turn 失败，也不能让压缩回滚。**正文写入失败时 Span 本身仍须落库。** 这时它退化成没有内容的 Span，而不是连 Span 一起丢失。

## 11. Span 语义

### 11.1 Model Call

**开始**：请求已构建完成、即将调用 `ModelPort::invoke`。
**结束**：流已完整消费，或调用明确返回错误/取消。

请求构建发生在 Span 开始**之前**，单独测量为 `requestBuildMs`。这样，`started_at → ended_at` 仍对应 Provider 调用生命周期，本地组装耗时也仍可诊断。

标准列：`resolved_model_name`、`model_id`、`status`、`attempt_count`、`provider_request_id`、token 系列、`response_message_id`、`started_at/ended_at`、`error_code/error_message`。

正文槽位：`request`、`system_context`、`tool_definitions`；仅当没有产生 Assistant Message 时才有 `response`。

`reasoning_tokens` 是 `output_tokens` 的子集；`total_tokens` 定义为 `input + output`，不额外加缓存或推理 token。

`attempt_count` 是该调用内 Transport 的总尝试数。它**必须等于实际开始过的尝试数**，不得用配置的最大值代替。不要为每个尝试建子 Span。

关键属性：

| 属性 | 语义 |
|---|---|
| `modelCallIndex` | 当前 Turn 内第几次 Model Call |
| `temperature` / `topP` | 采样参数。**参数改了，行为就会变；没有这两个值就无法对照** |
| `maxOutputTokens` / `thinkingMode` / `toolChoice` | 其余请求参数 |
| `requestBuildMs` | 构建完整请求的耗时 |
| `ttftMs` | 首次 Transport Attempt 到第一个**有效模型事件**的耗时 |
| `streamMs` | 从第一个有效模型事件到流结束的耗时 |
| `finishReason` | `stop/tool_use/length/content_filter/refusal` |
| `errorPhase` | `request_encode/connect/response_headers/response_body/stream_decode/response_decode/cancelled` |
| `deliveryState` | `not_sent/possibly_sent/accepted_no_semantic_output/semantic_output_emitted` |
| `httpStatus` / `providerCode` | **最后一次**尝试的传输结果 |
| `requestMessageCount` / `toolDefinitionCount` | 规模概览。列表页不必加载正文就能显示规模 |

"第一个有效模型事件"指 Text、Reasoning 或 Tool Call 的首个语义事件，**不包括**连接、响应头和心跳。如果调用在产生语义事件前失败，`ttftMs` 和 `streamMs` **保持为空，不用 `0` 冒充**已观察到的耗时。这时用 `errorPhase`、`deliveryState` 和 `httpStatus` 说明失败位置。

**不记录逐次 Transport Attempt 的明细。** 重试只增加 `attempt_count` 列，见 §15。

### 11.2 Tool Call

**开始**：完整 Provider Tool Call 已组装，即将解析参数、验证 Schema、决策权限。
**结束**：Tool Result Message 的持久化尝试完成，或 Tool Call 在形成可持久化结果前明确失败/拒绝/取消。**不等待下一次 Model Call。**

标准列：`provider_call_id`、`requested_tool_name`、`resolved_tool_name`、`status`、`permission_wait_ms`。

**正文槽位通常为空。** 参数在 assistant message 的 tool_use 块里，结果在 role=tool 的 Message 里。`(turn_id, provider_call_id)` 可以定位这两者。只有 Tool Call 失败或权限拒绝、且没有产生结果 Message 时，才写 `response` 槽位。

**不记录未截断的完整工具输出。** 模型看到的是截断后的结果，质量分析需要的正是这个输入。完整输出是运维关注点，而且一条 `bash` 就可能产生上百 MB。

`requested_tool_name` 是模型给出的名称，`resolved_tool_name` 是 alias 路由后的真实实现。**二者不能互相覆盖。**

**权限不是独立 Span。** Tool Span 覆盖完整生命周期。`permission_wait_ms` 只记录等待用户决定的累计耗时。

关键属性：`permissionDecision`、`permissionDecisionSource`、`sandboxMode`、`sandboxDenied`、`escalationPaths`、`dangerMatch`、`executionMs`、`artifactCount`、`artifactTypes`、`errorRetryable`、`resultPersisted`、`outputTruncated`。权限与沙箱属性的完整定义见 [permissions.md §7](permissions.md)。

`status` 表示**工具执行**的结果。不要把持久化失败记成工具失败：工具成功但 Message 写入失败时，Span 保持 `succeeded`，并记录 `resultPersisted=false`。

### 11.3 Compaction

**开始**：Core 已接受一次压缩、即将读取当前 Conversation。
**结束**：checkpoint 已持久化且新投影已安装，或任一阶段明确失败/取消。

```text
kind        = compaction        name = session.compact
trace_id    = threshold/overflow 复用触发它的请求；manual/rewind 新开
turn_id     = threshold/overflow 必填；manual/rewind 为空
parent_span_id = NULL（Compaction 总是 Trace 内的顶层操作）
model_id    = rewind 为空（没有摘要调用）
attempt_count = 实际摘要尝试次数
input_tokens/output_tokens = 成功摘要响应的 usage
```

属性分三组：

- **触发证据**：`trigger`、`contextWindowTokens`、`thresholdPercent`、`triggerEstimatedInputTokens`、`triggerPercent`；`overflow` 另记 `triggerModelSpanId` / `triggerErrorCode`。**`manual` 不针对窗口度量，所以不记录策略与触发估算，也不用默认值伪造这些值。**
- **压缩效果**：`conversationTokensBefore/After`、`reclaimedConversationTokens`。三者只度量 Conversation 区域，口径与 `ContextBudgetEstimate` 相同。所以 System Context 或 Tool Surface 的漂移不会影响差值。
- **分段耗时与结果**：`prepareMs/summaryMs/persistenceMs/installMs`、`sourceMessageCount`、`summaryChars`、`checkpointId`。

`prepareMs` 合并了原来的 `sourceCollectMs / stateCollectMs / systemContextMs`。三者都是本地亚毫秒操作，`summaryMs` 占绝大部分时间。拆成三个字段没有让任何人做出不同的决定。

Compaction Span 自身**没有正文槽位**：成功的摘要在 `conversation_compactions.summary` 里，`checkpointId` 指向它。

摘要采样的明细落在**子 Span 自己的列**上，父 Span **不保留任何聚合**。子 Span 有完整的 `request` 与 `response` 槽位。**摘要质量差时，要看的正是它当时读了哪些消息、写出了什么。** 失败的采样不进任何业务表，Trace 是唯一的落点。

用一条查询得到各类尝试的次数，不存 `attemptRollup`：

```sql
SELECT status, count(*) FROM trace_spans WHERE parent_span_id = $1 GROUP BY status
```

每个采样子 Span 的 `status` 就是它的分类结果。所以这个聚合 **by construction 恒等于**任何存下来的计数器。两者不是碰巧相等，而是同一个事实的两种写法。再存一份只会多出一处可能不一致的地方。

子 Span 的 `status` 是分类结果：

| 分类 | 含义 | 重试有意义吗 |
|---|---|---|
| `succeeded` | 产出可用摘要 | — |
| `degenerate` | 有响应但不可用（过短、缺章节、截断、请求了工具） | 是 |
| `deterministic` | 同输入重发无用（鉴权、请求非法、schema 错误） | 否 |
| `input_overflow` | 输入预算超限 | 否 |
| `transient` | 网络、过载、5xx | 是 |
| `timeout` | 超出单次尝试的墙钟预算 | 是 |

**分类目前只用于诊断，不改变重试循环。**

## 12. 状态与完整度

允许的状态：

```text
model_call   running succeeded failed cancelled outcome_unknown
tool_call    running succeeded failed denied cancelled outcome_unknown
compaction   running succeeded failed cancelled outcome_unknown
```

例外：Compaction 的摘要采样子 `model_call` 使用 §11.3 的分类状态：`succeeded / degenerate / deterministic / input_overflow / transient / timeout`。这些值只能出现在有父 Span 的 Model Call 上。普通 Model Call 仍只使用上表的生命周期状态。

- `running` 必须没有 `ended_at`，terminal 必须有；
- 普通失败用 `failed`，权限拒绝用 `denied`，用户取消用 `cancelled`，进程退出导致结果不可确认用 `outcome_unknown`；
- **Model Call 不能使用 `denied`。**

完整度在读取时派生，不写回数据库：

```rust
struct TraceCompleteness {
    expected_model_calls, captured_model_calls,
    expected_tool_calls, captured_tool_calls,
    orphan_tool_spans, running_spans, outcome_unknown_spans,
    state: Complete | Partial | None,
}
```

- **expected** 来自 `turns.model_submission_count / tool_call_count`。这是**独立参照物**：业务写入路径维护它，它与 Trace 写入路径互不依赖。两者一致才判定完整。**不要改成从 Span 计数派生。** 那样对账永远相等，失去意义。
- **captured** 是 `kind='model_call'` **且 `parent_span_id IS NULL`** 的 Span 数，以及 `kind='tool_call'` 的 Span 数。顶层过滤是必须的。摘要采样是 `model_call` 子 Span；把它们一起计入，会让 captured 恒大于 expected，把正常 Turn 误判成 `Partial`。
- **orphan** 是 `parent_span_id` 指向同一 Trace 中不存在的 Span。

### 为什么不统计"缺失的正文"

早先的设计里有一个 `spans_missing_payload`，统计没有 `request` 槽位的 Model Span。**这一项已删除，因为无法从读取结果可靠派生缺失原因。** 历史记录可能来自旧版本。正文写入也可能单独失败。部分槽位还会按 §5 的指针规则有意不写。

**必须接受这个后果**：界面上的"这里没有正文"只能陈述事实，不能声称原因。前端的处理方式见 [desktop.md](desktop.md)。

terminal Turn 的状态规则如下。captured 与 expected 分别相等，且 orphan/running/outcome_unknown 全为 0 时，状态为 `Complete`。expected 至少有一个调用，但 Model/Tool Span 全部没有采集到时，状态为 `None`。其他情况为 `Partial`。运行中的 Turn 可以展示实时计数，但**不能提前标记 `Complete`**。

## 13. 查询

```rust
list_turn_traces(filter, cursor, limit)      // Trace 列表，含标注
get_trace(turn_id)                            // 单个 Turn 的完整 Trace（不含正文）
get_trace_by_id(trace_id)                     // 无 Turn 的 Trace
get_span_payload(span_id, slot)               // 按需加载单个正文
list_compaction_spans(session_id, limit)      // Session 的压缩历史
upsert_annotation(trace_id, span_id, rating, note)
```

**正文必须是独立的按需查询。** 如果把正文放进 `get_trace`，打开一个 Turn 就会读取几 MB JSONB，而用户多数时候只想看时间线。列表页用 `requestMessageCount` / `byte_size` 显示规模。用户点开某个 Span 时才加载正文。

### 无 Turn 的 Span 必须有自己的读取路径

这是 Session scope 最容易遗漏的一步：**给 Span 加一个可空的 `turn_id` 很容易，但如果所有读取入口都以 Turn 为根，这些 Span 就是只写不读。**

`get_trace` 以 `turn_id` 为参数，所以 `turn_id IS NULL` 的 Span 不可能出现在它的结果里。以下三条路径补齐读取：

- **`list_turn_traces` 按 `trace_id` 组织**，它是两路来源的 `UNION ALL`。第一路是有 Turn 支撑的 Trace。第二路是 `turn_id IS NULL AND parent_span_id IS NULL` 的根 Span，每个根 Span 自成一条 Trace。第二路的 `turn_id` / `turn_sequence` 返回空，调用计数为 0。Span 与 Turn 的状态词汇不同，所以无 Turn 那一路在 SQL 里映射 `succeeded → completed`、`outcome_unknown → interrupted`，让列表只有一套状态词汇。
- **`get_trace_by_id`** 让无 Turn 的 Trace 也能打开详情。手动压缩的摘要采样有完整正文；没有这条入口，就看不到这些正文。
- **`list_compaction_spans`** 按 `session_id` 过滤，按 `started_at DESC` 排序。它也返回 threshold/overflow 的 Span，让一个 Session 的压缩历史成为一条完整列表。

**新增"可以没有 Turn"的 Span kind 时，必须同时给出它的读取入口。**

前端展示见 [desktop.md](desktop.md)。

## 14. 保留、清扫与删除

记录内容后，Trace 第一次成为**体积无界**的表。所以必须配套以下三件事。

### 截断

单个正文槽位有字节上限，默认是 1 MiB。用 `OpenWorkCoreConfig.trace_content` 中的 `TraceContentConfig` 配置上限。超限时截断正文，并设置 `truncated = TRUE` 与 `original_byte_size`。

**截断后必须说明原始大小**，数据库 CHECK 强制这一点。界面上无法量化的"已截断"警告没有用。

### 保留

正文按天保留。过期时只删 `trace_span_payloads` 行，**Span 本身与 token 保留**。理由是 token 用量要能在很长的时间跨度上比较，而正文的价值随时间迅速降低。

**带 `trace_annotations` 的 Trace 不参与自动清理。** 标为 `bad` 的调用是将来做回归的样本，最应该保留。

保留期由 `OpenWorkCoreConfig.trace_content` 的 `TraceContentConfig.retention_days` 配置，默认 **30 天**。30 只是第一版的起点，不表示正文价值有一个精确的 30 天分界。用 `trace_spans.started_at` 计算过期时间，不要用 `trace_payloads.created_at`。原因是正文按哈希去重，一条新 Span 可能引用数月前首次插入的 body。

仓库没有调度器，所以清理只在 bootstrap 时执行一次。执行时间在 Migration 与运行中状态收口之后，在 Recorder 启动和接受首个 Turn 之前。桌面应用的启动频率足以满足按天保留。不要为此增加常驻定时任务这个新运行部件。

### 孤儿清扫

`trace_payloads` 没有 `session_id`，删除 Session 不会级联删除正文。

**这是隐私必做项，不是空间优化。** 用户删除一个 Session 后，那个 Session 的源码内容必须真正从库里消失。所以清扫**必须在删除 Session 的同一次操作（同一事务）里执行**，不能只依赖周期任务。

清扫**只针对刚删掉的 Span 引用过的哈希**，不做全表扫描：

```sql
-- 1. 删除 Session 前先取出候选哈希
SELECT DISTINCT m.payload_hash
FROM trace_span_payloads m JOIN trace_spans s ON s.id = m.span_id
WHERE s.session_id = $1;

-- 2. 删除 Session（级联删掉 spans 与 mapping）

-- 3. 只清理候选里已经没人引用的
DELETE FROM trace_payloads p
WHERE p.hash = ANY($2)
  AND NOT EXISTS (SELECT 1 FROM trace_span_payloads m WHERE m.payload_hash = p.hash);
```

### 两个并发窗口

**并发时，全表 `NOT EXISTS` 扫描不够。** 这个问题是实现时发现的，不是理论问题：

| 窗口 | 现象 | 防线 |
|---|---|---|
| 正文 body 已插入、mapping 尚未挂载 | 清扫看到一个"没人引用"的新正文，并删掉它。随后 mapping 插入因 FK 失败 | 正文挂载与清扫**共享一把事务级 advisory lock** |
| mapping 已存在 | 清扫试图删除仍有引用的正文 | `payload_hash` 的 `ON DELETE RESTRICT` |

两条防线防护的不是同一件事。`RESTRICT` 保护**已经存在**的引用。advisory lock 覆盖**引用还没落地**的那一小段时间。**只有 `RESTRICT` 时，并发写入会随机失败；只有锁时，无法保护已提交的引用。**

限定候选哈希还有一个附带好处：清扫代价与删除的 Session 大小成正比，而不是与全表大小成正比。

保留策略复用同一路径。删除过期 mapping 时，用 `RETURNING payload_hash` 得到候选哈希。只把这批哈希交给同一个清扫 helper，并在同一事务、同一把 advisory lock 下完成清扫。保留策略不另起全表孤儿扫描。

## 15. attributes 的边界

`attributes` 不接受调用点随意构造的 map。Core 定义并测试版本化的 `ModelTraceAttributesV1` / `ToolTraceAttributesV1` / `CompactionTraceAttributesV1`。Storage 统一序列化它们，**拒绝未知字段，不静默透传**。

分工：

| | 放哪 |
|---|---|
| 标量、枚举、耗时、计数 | `attributes` |
| 正文 | `trace_span_payloads` |
| 跨 Trace 需要聚合或过滤的量 | 提升为列 |

**内容不进 `attributes`。** 如果混在一起，每次读 Span 都要同时读取正文，白名单校验也无法进行。

限制：`error_message` 截断到固定长度；只允许白名单 key；字符串/数组有元素与字节上限；URL 只保留 scheme/host 或稳定 label。

### 新增一个属性的门槛

§9 给出了新增 `kind` 的门槛，但一直没有给出新增属性的门槛。所以 kind 只有 3 个，属性一度增加到 78 个。这不是意外，是漏洞。

**三个问题全部通过，才可以新增一个字段：**

1. **有人会因为它做出不同的决定吗？** 「看着有用」不算。
2. **能不能从别的字段算出来？** 能算出来就不要存。存下来只会多出一处可能不一致的地方。
3. **是不是 99% 的行上都是空或同一个值？** 如果是，它属于别的层级。

一个属性的真实代价不是存储，因为 JSONB 里的空字段不占空间。真实代价是**每加一个属性要改 6 处**：Rust 类型、序列化白名单、前端属性白名单、三个语言包。i18n 有测试强制检查，漏掉一处，测试就会失败。

比这个代价更大的是**理解成本**：一个详情面板平铺 50 多行，就等于没有面板。所以删减必须配合分层，见下文。

### 主字段与折叠区

每个 kind 标出主字段。详情面板默认只显示主字段，其余字段放进「详细」折叠区：

| kind | 主字段 |
|---|---|
| `model_call` | 模型、耗时、token、`finishReason`、`temperature` |
| `tool_call` | 工具名、`permissionDecision`、`executionMs`、状态 |
| `compaction` | `trigger`、前后 token、回收量、尝试次数 |

**分层比删字段收益大**，而且不丢信息。删减解决"字段太多"，分层解决"不知道哪几个重要"。实际使用中的障碍是后者。

### 移除清单

**一、内容的影子。** 早期没有正文，只能用大小和计数近似表示内容。有了正文后，这些字段失去意义：

```text
requestSystemMessageCount / requestUserMessageCount
requestAssistantMessageCount / requestToolMessageCount
requestContentBytes / toolDefinitionBytes
responseTextBytes / responseReasoningBytes / responseToolArgumentsBytes
inputBytes / outputBytes / outputLines / inputTopLevelKeyCount
```

保留 `requestMessageCount` 和 `toolDefinitionCount`：它们是语义规模，列表页不加载正文时也要显示它们。字节数改由 `trace_payloads.byte_size` 提供。

**二、可派生的（第 2 问不过）。**

| 移除 | 因为 |
|---|---|
| `attempts[]`（嵌套 10 个子字段）| 见下文的单独一节 |
| `attemptRollup`（嵌套 7 个计数器）| 对子 Span 的 `status` 做一条 `GROUP BY` 就能得到它 |
| `summaryAttemptOutcome` | 和采样子 Span 的 `status` 是同一个枚举值 |
| `summaryEstimatedInputTokens` | 前三项估算之和 |

**看起来可派生、实际不可派生的**：`artifactCount` 和 `artifactTypes`。types 是排序去重后的列表，count 是原始个数。两者不等价，所以都保留。回答第 2 问时，看实际实现，不要看名字。

**三、没人据此做决定的（第 1 问不过）。**

```text
progressEventCount        进度事件个数改变不了任何判断
validationMs              schema 校验通常不到 1ms
resultPersistMs / resultPersistErrorCode   存储层自检，是运维问题
```

保留 `resultPersisted`：模型有没有看到这个结果是质量问题，而结果持久化的耗时和错误码不是。

**四、错层的（第 3 问不过）。**

```text
appVersion    三个属性类型各带一份，每行存一次进程级常量。移到 turns
```

`sourceCollectMs / stateCollectMs / systemContextMs` 合并成 `prepareMs`（三个都是本地亚毫秒操作）。

### 为什么砍掉 `attempts[]`

这是唯一需要单独说明的移除项，因为移除它会造成实际的能力损失。

`ModelTransportAttemptTrace` 的每个元素有 10 个子字段（`index/status/durationMs/errorCode/errorPhase/deliveryState/httpStatus/providerCode/providerRequestId/retryDelayMs`），一次调用最多有 N 个元素。它是**旧定位留下的最后一块纯运维内容**：逐次重试的传输轨迹。

新定位中没有它的位置。它回答的是「哪个 Provider 在抖」。这是规模化服务的问题，本地单机应用没有这个场景。

**保留下来的**：`attempt_count` 列（重试了几次）、`errorPhase`、`deliveryState`、`httpStatus`、`providerCode`、`error_code`。仍然可以回答「慢是不是重试造成的」：`attempt_count > 1` 加上总耗时就够了。

**确实失去的**：每次尝试各等了多久（`retryDelayMs`）、中间某次尝试的独立错误码。排查退避策略本身时会缺少数据。这是有意接受的代价，不是遗漏的问题。

**顺带解决一处重复**：`httpStatus` / `providerCode` 原本同时存在于顶层和每个 attempt 元素里。删除数组后，顶层那份成为唯一来源，语义明确为「最后一次尝试的结果」。

### 结果

| | 改动前 | Phase 1 之后 | 当前（Phase 3 完成） |
|---|---|---|---|
| `ModelTraceAttributesV1` | 31 | 31 | 22 |
| `ToolTraceAttributesV1` | 19 | 14 | 10 |
| `CompactionTraceAttributesV1` | 28 | 23 | 23 |
| **合计** | **78** | **68** | **55** |
| 嵌套类型 | `ModelTransportAttemptTrace`(10) + `CompactionAttemptRollup`(7) | 已删除 | — |

`ModelTraceAttributesV1` 在 Phase 1 一度持平（31 → 31）：删掉了 `attempts` / `appVersion` / `summaryAttemptOutcome` 三个，同时加入了 `temperature` / `topP` / `toolChoice` 三个。Phase 3 又删掉 9 个内容影子，最终为 22 个。

新加的 `temperature` / `topP` / `toolChoice` 明确通过第 1 问：**参数改了，行为就会变；没有它们就无法对照。**

落地分两步，现在已全部完成。第一步删除不依赖正文的字段。第二步在正文与 `trace_payloads.byte_size` 接管后，删除 13 个内容影子。这样中间不会出现既没有字节数也没有正文的空窗。

`summaryRetryDelayMs` **保留**。它是摘要重试的节奏，记在采样子 Span 上有意义，不属于已删除的传输层明细。上面任何一组都没有列出它。这是清单的疏漏，不是待决项。

## 16. 子 Agent

设计见 [multi-agent.md](multi-agent.md)。落地形态与本节原先的预案一致：**子 Agent 自己是一个 Session**。`sessions` 增加 `parent_session_id` 与 `spawn_span_id`，后者指向发起子 Agent 的 Tool Call Span。**Trace 结构不变，不新增 `kind`，不新增列。**

理由：子 Agent 本来就有自己的上下文窗口、对话和压缩。把它建模成 Session 是描述事实，不是绕路。这样它自然拥有合法的 `turn_id`，不影响父 Turn 的调用计数。跨 Session 汇总用 `sessions` 的递归 join。

具体规则：

- 子 Turn 的 `trace_id` **等于子 `turn_id`**，遵循 §2 的主规则，不继承父 Turn 的 `trace_id`；
- **不设跨 Trace 的 `parent_span_id`。** §12 把 orphan 定义为"指向同一 Trace 中不存在的 Span"。如果子 Agent 的根 Span 指向父 Trace，完整度计算就会永远把它判成 orphan；
- 业务列表达关联关系：`sessions.parent_session_id` 给出拓扑，`sessions.spawn_span_id` 给出"哪个 Tool Call Span 发起了它"。Trace UI 用这两列跳转。`spawn_span_id` **不建外键**：Trace 是 best-effort，队列满时那个 Span 可能根本没有落库。这符合 §2「结构标识不建外键」；
- 父侧不变：`spawn_agent` / `wait_agent` / `list_agents` / `followup_task` / `interrupt_agent` 各产生一个普通的 `tool_call` Span。

**不要采用的两种形态**：

- 让子 Agent 的 Span 共享父 `trace_id`。§12 的 expected 来自 `turns.model_submission_count`，captured 是顶层 `model_call` 数。子 Agent 的 Span 混进来会让 captured 恒大于 expected，**每个用了子 Agent 的正常 Turn 都会误判成 `Partial`**。这与摘要采样子 Span 当年的问题完全相同。
- 在父 Session 下给子 Agent 单独插入 `turns` 行。这会占用 Session 的轮次编号，并与"一个 Session 一个活跃 Turn"冲突。

## 17. 常见诊断规则

下表是 UI/查询层的推断。**必须把它们标注为推断**，不写回业务状态：

| 现象 | 推断 |
|---|---|
| 回答质量突然变化 | 先对比 `temperature` / `topP` / `resolved_model_name`，再看 `request` 正文 |
| 压缩后回答开始跑题 | 读压缩子 Span 的 `response`，摘要可能丢了关键约束 |
| `request` 正文里没有某条用户消息 | 它在 checkpoint 的摘要区间内，摘要替换了它 |
| Model Call 很慢且 `attempt_count > 1` | 主要延迟来自 Transport 重试 |
| `ttftMs` 高、`streamMs` 正常 | Provider 排队、连接或首包路径 |
| `request` 正文持续变大 | Conversation 或 Tool Schema 正在变大 |
| 估算与真实 `input_tokens` 长期偏差大 | bytes/4 不适合该模型，只能作为发送前近似 |
| Tool Call `permission_wait_ms` 占大部分 | 大部分时间在等待用户的权限决定 |
| `outputTruncated=true` | 后续模型看到的是截断结果 |
| Tool Call failed，下一次 Model Call succeeded | 模型看到错误后，用替代路径完成了任务 |
| Turn failed 且 Trace partial | 只说明诊断不完整，**不能归因于缺失节点** |
| Tool Call `outcome_unknown` | 副作用不可确认，**不得自动重放** |

## 18. 明确不做

- **不建 `traces` 表。** Trace 层没有持久业务状态，状态、耗时、计数都可以从 Span 派生。
- **不建 `trace_span_events` 表**，也**不在属性里存逐次 transport attempt**（§15）。重试只增加 `attempt_count`。
- **不存任何可由子 Span 聚合出来的计数器**（已删除的 `attemptRollup` 就是这种计数器）。
- **不复制 `messages` 已有的内容**（§6）。
- **不记录完整未截断的工具输出**（§11.2）。
- **不给 `parent_span_id` 建外键**（§10）。
- **不做自动评分与评测集**（§8）。
- **不记录成本**：不存单价、不算金额、不做汇总。见下文。

> **对旧设计的反转：** 早期文档写着"不建 Payload 表"，理由是 Trace 不该处理内容。这次定位调整后，那一条作废，因为内容正是质量追踪的主体。它背后的判断有一半正确，现在保留：**内容不能进 `trace_spans` 的行里**。所以才有独立的两张表和按需加载。

### 为什么撤掉成本

成本功能曾经完整实现，并通过了全部验收，然后整体撤除。撤除的理由不是"没做完"，而是**做完之后发现它在日常使用中基本失效**：

任何失败或**取消**的 Model Call 都会把整个 Turn 的成本标为未知。只要有一个未知 Turn，Session 汇总就整体显示"—"。取消是日常操作。一个 Session 里只要取消过一次，成本显示就永久失效。

更深的原因是 `turns.cost_amount` 用一个可空列同时表达两种状态：

- 「还没记录任何成本」
- 「有一项无法定价，总额不可知」

一个 NULL 无法区分这两者。所以累加逻辑只能用 `model_submission_count = 1 AND NOT EXISTS(压缩)` 这类代理条件，去猜"我是不是第一个"。这些条件在顺利路径上成立，在任何失败路径上全部失效。例如，overflow 重试的 Turn 即使每一步都有价格，最终成本仍是空。

要修复它，就要引入一个显式的成本状态列，并区分"请求未发出"和"已发出但结果未知"。`deliveryState` 已经有这个信息。这是一次完整的设计迭代，不是补丁。

**当前的判断是：这个功能的价值不值得这个复杂度。** Trace 已经记录 token。需要估算花费的人可以自己乘以单价。

保留下来的有两样：**§7 的 Provider token 口径表**（那是 token 语义，与计价无关），以及 `TokenUsage.cache_creation_input_tokens` 的解析。**单价列、金额列、汇总查询和界面全部移除。**

如果将来重做，先解决状态二义性，再写代码。

## 19. 尚未实施

- **正文的 Core 写入、Desktop 按需读取、截断、按天保留与孤儿清扫已经实施。** 标注仍只有 schema 和设计。**成本已从设计中移除**，理由见 §18；
- 仍没有用官方资料验证 Kimi 的 `cached_input_tokens` 集合关系，§7 表中标为未验证；
- **§15 的移除项已经实施**：逐次 Transport 明细、父级 attempt 聚合、低价值 Tool 耗时/计数、重复 `appVersion` 与 13 个内容影子均已删除；三个压缩准备耗时已合并为 `prepareMs`；
- **主字段/折叠区分层已经实施。** 属性白名单的每个 key 都有编译期穷尽归类；
- `temperature` / `topP` / `toolChoice` 已进入 Trace；`topP` 也已加入 `ModelRequest`，三个 Provider codec 都发出它；
- `flush_turn/flush_session` 的 dropped/write-failure 计数尚未在界面或运维出口显示。并发增加时，这一条的优先级会上升；
- Attempt 分类尚未用于重试决策；
- 自动压缩失败后没有抑制状态；
- Recorder 始终记录全部支持的正文槽位；`TraceContentPolicy` 及其运行时切换路径已经删除；`NoopTraceRecorder` 只用于测试装配；
- `trace_span_payloads.redacted_count` 恒为 `0`，是应当删除的列，见 [data-model.md](data-model.md)；
- 项目没有直接声明 `tracing` 或 OpenTelemetry 依赖。以后接入时，用同一个生命周期 Guard 同时更新领域 Trace 与 `tracing::Span`。OTLP 默认关闭，它失败时不影响 Turn。

## 20. 验收

### 内容

1. 一次 Model Call 的 `request` 槽位能完整还原当时提交的 provider-neutral 消息数组；
2. 压缩之后，`request` 正文里是**摘要 + 边界后的原始消息**，而不是全部 `messages`；
3. 成功的 Model Call **不**写 `response` 槽位，而是用 `response_message_id` 指向 Assistant Message；
4. 失败的 Model Call 写 `response` 槽位（若已收到部分响应），且没有 `response_message_id`；
5. Tool Call 默认没有任何正文槽位；只有权限拒绝的 Tool Call 才写 `response`；
6. 同一 Session 内多次调用的相同 `tool_definitions` 在 `trace_payloads` 中只有一行；
7. 正文超过上限时截断，`truncated = TRUE` 且 `original_byte_size` 非空。**数据库拒绝只置 `truncated`、不给原始大小的行**；
8. 正文写入失败时 Span 本身仍落库；
9. 正文中不出现 API Key、凭证、HTTP Header；
10. `get_trace` **不**返回正文；正文只经 `get_span_payload` 按需加载。

### 标注

13. 同一目标重复标注是更新，而不是新增一行；
14. 标注既能挂到整条 Trace（`span_id IS NULL`），也能挂到单个 Span；
15. 保留策略不清理带标注的 Trace；
16. 删除 Session 时级联删除它的标注。

### 保留与隐私

17. 正文过期后 Span 与 token 仍在，过期的 `trace_span_payloads` 行消失；由此产生的无人引用 body 按第 18 条清扫；
18. **删除 Session 或过期正文映射后立即执行孤儿清扫，`trace_payloads` 中不残留已经无人引用的独有内容**；
19. 清扫无法删除仍有引用的正文（`RESTRICT` 生效）。

### 结构

20. 同一次用户请求产生的全部 Span 共享一个 `trace_id`；
21. 手动压缩产生的 Span 有自己的 `trace_id`，且 `turn_id IS NULL`；
22. rewind 同上，且 `model_id` 为空、无子 Span；
23. `trace_id` 非空约束生效：数据库拒绝缺少它的写入，不让它静默落库；
24. Trace 表无 `sequence` 列，也无 `(turn_id, sequence)` 唯一约束；
25. **并发写入同一 Trace 的多个 Span 全部落库**，不因排序键冲突丢失任一条；
26. 排序仅由 `started_at` 决定，`id` 作为稳定 tiebreak，同毫秒内顺序可重现；
27. 父 Span 未落库时，子 Span 仍成功写入，并计为 orphan；
28. 数据库约束拒绝两种行：携带 tool 独占列的 `kind='model_call'` 行，以及非 model_call 上出现 `response_message_id` 的行。

### 压缩

29. 四类触发各产生且只产生一个 Compaction Span；
30. threshold/overflow 复用触发请求的 `trace_id` 并携带 `turn_id`；manual/rewind 新开 `trace_id` 且 `turn_id` 为空；
31. 摘要采样是子 Span，每次有自己的 `provider_request_id`、token 列和正文；
32. **失败的摘要采样的 `response` 正文可读**。它不进任何业务表，Trace 是唯一落点；
33. `SELECT sum(input_tokens) WHERE kind='model_call'` 包含压缩开销；
34. 摘要采样**不**增加 `turns.model_call_count / model_submission_count`；
35. 有子 Span 的 Turn，completeness 仍为 `Complete`；
36. overflow 的 `triggerModelSpanId` 指向那次真实失败的 Span，且该 Span 状态为 failed；
37. manual 压缩**不**记录窗口、阈值与触发估算；
38. 每次压缩都记录前后 token，且 `reclaimed = before - after`（不小于 0）；
39. 摘要尝试按六类分类，**分类结果就是子 Span 的 `status`**，父 Span 不存任何聚合；
40. 摘要连续失败时 Span 为 failed 且带错误码，**checkpoint 未安装、Conversation 未改变**；
41. `list_compaction_spans` 返回该 Session 的全部压缩，按 `started_at DESC` 排序，`limit` 生效；
42. **手动压缩在 Desktop 上可见且可打开详情**：存在从 `/compact` 到界面看到摘要正文的端到端用例。

### 基础

43. 无工具 Turn 产生一个 Model Span；
44. Model → Tool → Model 产生两个 Model Span 和一个正确挂载的 Tool Span；
45. Tool Name Alias 同时保留 requested/resolved；
46. Permission Wait 只增加 Tool Span 的等待耗时；
47. Provider Retry 只增加 `attempt_count`，**既不产生独立 Attempt Span，也不写逐次明细数组**；此时 `httpStatus` / `providerCode` 反映最后一次尝试的结果；
48. Model Span 不可写入 `denied`；
49. terminal Span 必有 `ended_at` 且 `ended_at >= started_at`；
50. 启动时遗留的 Running Span 变为 `outcome_unknown`；
51. Trace API 能识别 Complete/Partial/None；**正文有无不参与这个判断**：完整度只对账 Span 结构，不对账内容；
52. Model Span 的 Provider 耗时不含 `build_request`，三段耗时可分别验证；
53. Tool Span 能区分 permission/execution，并用 `resultPersisted` 区分结果是否进入 Conversation；持久化失败时仍产生 terminal Span；
54. 记录 `temperature` / `topP`，修改后新 Span 反映新值。

### 降级

**底线：Trace 可以丢，业务不能受影响。**

55. Trace Queue 满时 Turn 结果不变；
56. 数据库 Trace 写失败时 Message 仍提交；
57. 压缩的 Trace 写入失败时 checkpoint 仍安装、Conversation 仍替换；
58. `attributes` 只接受白名单字段，系统截断或拒绝超长字符串/数组；
59. 若启用 `tracing`/OTLP 出口，其关闭、丢弃或导出失败不改变 PostgreSQL Trace 和 Turn 结果。

### 时间口径

60. 时间列存东八区墙上时间，见 [data-model.md](data-model.md) 开头；
61. 出库字符串带 `+08:00` 而非 `Z`。**存在一个断言后缀的用例。** 标错时区不会报错，只会让界面整体偏 8 小时；
62. 一个瞬间经"落库 → 序列化 → 前端解析"往返后仍等于原瞬间。
