# Trace

Trace 回答**「模型看到了什么、说了什么、烧了多少 token、人怎么评价」**。它是质量追踪，不是运维埋点。

写入仍是 best-effort：丢一条 Span 只让排查变难，不改变任何业务结果。

## 1. 定位

早期版本把 Trace 当成运维 trace：记录字节数、重试轨迹、持久化耗时，用来回答「runtime 是不是在正常工作」。这个定位错了。

错在两处。**一是收益前提不成立**——运维型 trace 的价值来自规模（成千上万个用户在跑，你需要知道哪个 Provider 在抖）。OpenWork 是本地单机应用，那个规模效应根本不存在。**二是答不了用户真正会问的问题**——「这次回答为什么不对」，字节数和重试次数一个都答不了。

**Trace 能回答：**

| 问题 | 靠什么 |
|---|---|
| 模型这次收到的完整请求是什么 | `request` / `system_context` / `tool_definitions` 三个正文槽位 |
| 它回了什么 | `response_message_id` 指向的 Message，失败时是 `response` 槽位 |
| 花了多少 token | token 四列，Span 与 Turn 两级 |
| 用什么参数跑的 | `temperature` / `topP` / `maxOutputTokens` / `thinkingMode` |
| 这次好不好 | `trace_annotations` |
| 慢在哪、错在哪 | 分段耗时、`errorPhase`、`deliveryState` |
| 压缩为什么触发、省了多少 | Compaction Span 的触发证据与前后 token |

**Trace 不能做：**

- 恢复未完成的 Turn；
- 推导 Tool Call 的副作用是否发生；
- 决定是否重试；
- 让 Turn 失败。

PostgreSQL 领域 Trace 是本地产品契约。Rust `tracing` 只承担运行时 instrumentation，OpenTelemetry/OTLP 只作为未来可选出口，**三者不能互相替代**。`attempt_count`、`denied`、`outcome_unknown`、Complete/Partial 都是 OpenWork 的领域语义，必须由类型化 Guard 显式产生，不能靠 Subscriber 反推。

## 2. 三层标识

Trace 有且只有三个**结构标识**，其余都是业务标签：

| 标识 | 作用 | 外键 | 可空 |
|---|---|---|---|
| `trace_id` | **结构根**。一次用户请求（或一次无 Turn 的独立操作）的全部 Span 共享它 | 无 | 否 |
| `id` | Span 自身 | 主键 | 否 |
| `parent_span_id` | **发起关系**：父 Span 发起了子 Span | **有意不建** | 是 |

`session_id` 和 `turn_id` 是**业务标签**。`turn_id` 的语义是"这个操作是否发生在某个 Agent Loop 内部"——手动压缩和 rewind 不在，因此为空。

**判别法：** 能不能给一件没有对应业务行的事，随手发一个这样的 id？`trace_id` 能（手动压缩没有 Turn 照样开一个），`turn_id` 不能（要先往 `turns` 插一行，而那行要求 `client_request_id`、`sequence > 0`、`resolved_provider_kind`）。

由此推出外键规则：**结构标识不建外键，业务标签建外键。** 结构标识必须能指向"可能不存在"的东西（父 Span 可能因为队列满而根本没落库）；业务标签指向真实业务行，不但该建外键，还该跟随业务生命周期级联删除。

### `trace_id` 由谁生成

| 场景 | 取值 |
|---|---|
| Turn 内的任何 Span | **等于该 Turn 的 `turn_id`** |
| 手动压缩、rewind | 新生成 `trace-<uuid>` |
| 任何子 Span | **继承父 Span 的**，不重新生成 |

Turn 内直接复用 `turn_id` 是因为两者一一对应、能省一次映射；**这不代表语义相同**——`turn_id` 仍可为空并带外键，`trace_id` 必填且无外键。

生成时机是**操作开始时**，由发起该操作的执行体分配：`TurnRunner` 在 Turn 开始时确定，`SessionActor` 在手动压缩/rewind 开始时确定。一次操作内不得中途更换。

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

关系由**一条规则**决定：

> `parent_span_id` 表示"**谁发起了谁**"，不是"谁包含谁"。

| 关系 | 为什么 |
|---|---|
| Tool Call 的父是那次 Model Call | 工具调用是模型在那次响应里明确请求的 |
| 摘要采样的父是 Compaction | 压缩流程发起了这次采样 |
| Compaction 没有父 | 它是 Core 的策略决定，模型和工具都没请求它。挂到某个 Model Call 下会撒谎 |
| overflow 的失败 Model Call **不是**父 | 它只**导致**压缩，没有**发起**压缩。记在 `attributes.triggerModelSpanId` |

**这条规则和 OTel 惯例不同，是有意的。** OTel 里父 Span 通常在时间上包含子 Span，但这里 Tool Span 在 Model Span 关闭之后才开始（`call_model` 先 finish 再返回）。四种情形里只有"摘要采样在 Compaction 内部"是真正的时间嵌套。

取因果语义的理由：时间关系用 `started_at` 已经能表达，而"这次工具调用是哪次模型响应要求的"没有别的地方可放——并发工具调用的时间区间互相重叠，靠区间反推不出来。

### 没有根 Span

一条 Trace 的顶层是若干平级 Span，不存在一个代表整次请求的根。想看"用户问了什么、最终回答了什么"，读 `turns` 那一行和它的首尾 Message。

理由见 §6：那两段内容已经是业务真相，Trace 不复制。

## 4. 不设 `sequence`，按 `started_at` 排序

Span 之间的先后一律由 `started_at` 决定，`id` 兜底 tiebreak。**不设 `sequence` 列，也不设 `UNIQUE (turn_id, sequence)`。**

序号方案在单条串行 Agent Loop 下可行，但它把"同一时刻只有一个执行体在写"编码进了唯一约束。一旦有第二个执行体（并发工具、subagent、后台任务）向同一 Trace 写入，两个独立分配的序号必然冲突；而 Trace writer 的冲突子句只覆盖主键，唯一约束冲突会让整个批次事务回滚——**一次丢失最多 64 条 Span，只增加一个计数器，不重试也不告警**。

删掉序号即消除这类故障，代价只是失去"写入顺序"这一冗余信息（`started_at` 已经表达了它）。

## 5. 一次 Turn 记几行

2 次模型调用、3 次工具调用、1 次 threshold 压缩、摘要一次成功：

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
| 每次逻辑模型调用 | Turn 本身（`turns` 表已经是它） |
| 每次工具调用 | Transport 重试（只增加 `attempt_count` 列） |
| 每次压缩 | 权限等待（记在 `permission_wait_ms` 列） |
| 压缩内每次摘要采样 | |

摘要重试每次都重新构造并提交独立请求（各有 `provider_request_id` 和 usage），所以是行；transport 重试是同一 payload 重发，所以是属性。

注意计数后果：上例中 `COUNT(*) WHERE kind='model_call'` 是 **3** 而不是 2，因此完整度必须加 `AND parent_span_id IS NULL`（见 §12）。

### 摘要采样为什么必须是子 Span

把它们压平成 JSON 数组会让 token 统计出现黑洞：

```sql
SELECT sum(input_tokens) FROM trace_spans WHERE kind = 'model_call'
```

这条查询算不出会话真实开销——压缩烧掉的 token 在另一个 kind 里，而且失败尝试的 usage 完全没有落库位置。改成子 Span 后，每次采样有自己的行和 token 列。

## 6. 内容

这是质量追踪的核心，也是和早期设计分歧最大的一节。

### 只记 `messages` 回答不了的

> **凡是已经落进 `messages` 或 `conversation_compactions` 的内容，Trace 不复制，只留指针。**

理由不是省空间，是**避免同一份内容出现两个可能不一致的版本**。`messages` 是业务真相、只增不改不删、永远比 Trace 完整；再拷一份进 best-effort 的表里，一旦两边不一致，没人知道该信谁。

| 内容 | 在哪 | Trace 怎么做 |
|---|---|---|
| 用户输入 | `messages`（role=user） | 不记 |
| 成功调用的响应 | `messages`（role=assistant） | 记 `response_message_id` 指针 |
| 工具参数 | assistant message 的 tool_use 块 | 不记 |
| 工具结果 | `messages`（role=tool），由 `(turn_id, provider_call_id)` 唯一确定 | 不记 |
| 成功的摘要 | `conversation_compactions.summary` | 记 `checkpointId` 指针 |
| **组装后的请求** | **不在任何地方** | **记 `request` 槽位** |
| **System Context** | **不在任何地方** | **记 `system_context` 槽位** |
| **工具定义** | **不在任何地方** | **记 `tool_definitions` 槽位** |
| **失败调用的响应** | **不在任何地方**（没有产生 Message） | **记 `response` 槽位** |

**组装结果是这一节存在的全部理由。** 压缩之后模型看到的 Conversation 和原始 `messages` 不再相同——它是 checkpoint 的摘要加上边界之后的原始消息，还叠了 System Context 和 runtime reminder。这个投影结果是"模型实际看到了什么"的唯一答案，而且**没有任何别的表保存它**。

### 为什么不用描述符重建投影

投影理论上可从 `checkpoint_id + message 序号区间` 重建，那样几乎不占空间。**否决**：System Context 和 reminder 会随版本变化，重建出的是今天的组装结果而不是当时的。95% 忠实的重建在排查"模型为什么答错"时比没有更糟——你会对着一份没人见过的输入找原因。

### 去重

`trace_payloads` 以内容哈希为主键，`trace_span_payloads` 挂载。

一个 Session 内 System Context 和工具定义几乎不变，却随每次 Model Call 重复发送。20 KB 的工具定义在 50 个 Turn、400 次调用后按行存是 8 MB，去重后是 20 KB。

`request` 槽位去重不了（每次都在长），它是体积的主要来源。这是有意接受的代价，由截断和保留策略兜底（§14）。

`trace_payloads` **没有 `session_id`**：同样的工具定义本就跨 Session 相同，加上它等于放弃去重。代价是**删除 Session 不级联删除正文**，必须由孤儿清扫收尾——见 §14，这是隐私相关的必做项而不是优化。

### 外键为什么这次可以建

`trace_span_payloads.span_id` 建了外键，看起来和 §10「Trace 有损所以不建外键」矛盾，其实不是：

- `parent_span_id` 指向**另一个**可能被独立丢弃的 Span——建外键会把单点丢失放大成级联丢失；
- `trace_span_payloads.span_id` 指向的是**同一批写入的自己**——不存在指空的可能。

判别法：这个引用指向的行，有没有可能在被引用时还不存在或已经丢了？

### 内容记录是可关的

`TraceContentPolicy` 的初始值挂在 `OpenWorkCoreConfig` 上，三档：

| 档位 | 行为 |
|---|---|
| `full` | 记录全部四个槽位（默认） |
| `compaction_only` | 只记录 Compaction 及其摘要采样子 Span 的槽位 |
| `off` | 不写 `trace_payloads` / `trace_span_payloads`，其余 Trace 不变 |

默认 `full` 的理由是**内容不出本机**——Postgres 在用户自己的机器上，这正是云端 observability 厂商必须默认关闭而我们可以默认开启的原因。但它记录的是用户的私有代码，因此必须有关闭入口，且界面上要说明"开启后 Trace 含源码内容"。

**切换只发生在 Recorder 注入处，不得在 Agent Loop 里加 Trace 分支。**

运行中由 Recorder 持有一个原子策略值；每次接收写入信号时读取当下值，再决定是否保留正文。Desktop 在本地持久化用户选择，并在启动时、允许发起第一个 Turn 之前把选择推给 Core；之后切换立即作用于后续写入，不需要重启，也不追溯改写历史正文。

无论哪一档，以下永远不记录：API Key、解密后的凭证、HTTP Header、未脱敏的 Provider 错误 Body。

## 7. Token 口径

Trace 记录 token，**不记录钱**。理由见 §18。

四个 token 列（`input_tokens` / `output_tokens` / `cached_input_tokens` / `reasoning_tokens`）的语义**跨 Provider 不统一**，聚合前必须知道这一点。

### `cached_input_tokens` 是不是 `input_tokens` 的子集

| Provider kind | `input` 含 `cached` 吗 | 依据 |
|---|---:|---|
| `anthropic` | **否** | usage 把 `input_tokens`、`cache_read_input_tokens`、`cache_creation_input_tokens` 分成三个独立桶 |
| `openai` | 是 | `cached_tokens` 是 `prompt_tokens` 的明细 |
| `deepseek` | 是 | `prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens` |
| `qwen` | 是 | OpenAI-compatible 响应中 `cached_tokens` 是 `prompt_tokens` 的子集 |
| `glm` | 是 | 官方示例以 `cached_tokens / prompt_tokens` 计算命中比例 |
| `kimi` | **未验证** | 走 OpenAI-compatible 解析，但未查到官方对集合关系的明确说明。**不要当成已验证事实** |

**后果：`sum(input_tokens)` 跨 Provider 不可比。** Anthropic 的那份不含缓存读取，其余的含。任何"缓存命中率"或"总输入量"的计算都必须先按 `resolved_provider_kind` 分组，否则数字是错的且不会报错。

Anthropic 另有 `cache_creation_input_tokens`（写缓存）。它既不在 `input_tokens` 里，也不是 `cached_input_tokens`——当前 adapter 不发送 `cache_control`，所以这个桶实际总是 0，但解析层保留了它。

`reasoning_tokens` 是 `output_tokens` 的子集；`total_tokens` 定义为 `input + output`，不额外加缓存或推理 token。

> 这张表是做成本功能时查出来的。功能撤了，结论留下——它描述的是 token 语义，和计价无关。

## 8. 人工标注

`trace_annotations` 是**业务真相**——丢了就是用户输入丢了——尽管它指向 best-effort 的 Span。

```sql
rating   good | bad | unsure
span_id  为空 = 评价整条 Trace，非空 = 评价其中一次调用
```

一个目标只有一条标注，改评价是 upsert 而不是追加一条相反的。

由"它是业务真相"推出一条保留策略约束：**带标注的 Trace 不参与自动清理**（§14）。被标为 `bad` 的那次调用恰恰是最需要长期保留的——它是将来做回归的样本。

**不做的**：自动打分、评测集、A/B 对照。它们需要一整套离线运行与对照基线，不是给 Span 加几列能得到的。这里只提供人的判断这一个信号。

## 9. 什么时候才该新增一个 `kind`

门槛是**三条同时成立**：

1. 有自己的起止时刻和成败（不是一个状态字段）；
2. 不属于任何 Model Call 或 Tool Call——是 Core 自主发起的，模型并不知道它发生了；
3. 可能脱离 Turn 发生。

`compaction` 三条全中。反过来，凡是**模型通过工具触发**的能力（Plan、Todo、Memory 的读写），它对模型的接口就是工具，天然是 `tool_call`，不需要新 kind——见 [context-window.md](context-window.md) 对这些能力接入位置的定义。

新增一个 kind 的实际成本：

| | 要动什么 |
|---|---|
| 1 | migration 改 `kind` 的 CHECK 与独占列的判别式约束 |
| 2 | 新的 `XxxTraceAttributesV1` 类型、版本与白名单 |
| 3 | 新 Guard + `Drop` 兜底 |
| 4 | 完整度语义决策：算不算 expected |
| 5 | **正文槽位决策**：它有没有 `messages` 回答不了的内容 |
| 6 | 前端 kind 分支（树、时间线、详情）、图标、颜色、属性白名单、**三语 i18n**（有测试强制） |
| 7 | **若它可能没有 Turn，必须同时给出读取入口**（见 §13） |

第 7 条最容易漏：Span 写进去了，但读取入口以 Turn 为根，结果只写不读。

**默认答案是"用现有 kind 表达"。** 若某能力将来演变成"Core 自动进入某模式、自己跑一个多轮子流程"，更可能的正确形态是**子 Session**（见 §16），而不是新 kind。

## 10. 有损写入决定的两条硬规则

Trace 是 best-effort：队列满时 `try_send` 直接丢，批量写失败丢整批。由此推出两条不可协商的规则：

1. **`parent_span_id` 不建外键。** 父 Span 丢失时，加了外键会让所有子 Span 插入一并失败，把单点丢失放大成级联丢失。孤儿 Span 仍有独立诊断价值，应保存并计入采集缺口。
2. **Trace 写入失败不改变任何业务结果。** 队列满、数据库不可用、Flush 超时都不能让 Turn 失败或让压缩回滚。**正文写入失败时 Span 本身仍须落库**——退化成没有内容的 Span，而不是连 Span 一起丢。

## 11. Span 语义

### 11.1 Model Call

**开始**：请求已构建完成、即将调用 `ModelPort::invoke`。
**结束**：流被完整消费，或明确返回错误/取消。

请求构建发生在 Span 开始**之前**，单独测量为 `requestBuildMs`。这样 `started_at → ended_at` 保持 Provider 调用生命周期，而本地组装耗时仍可诊断。

标准列：`resolved_model_name`、`model_id`、`status`、`attempt_count`、`provider_request_id`、token 系列、`response_message_id`、`started_at/ended_at`、`error_code/error_message`。

正文槽位：`request`、`system_context`、`tool_definitions`；仅当没有产生 Assistant Message 时才有 `response`。

`reasoning_tokens` 是 `output_tokens` 的子集；`total_tokens` 定义为 `input + output`，不额外加缓存或推理 token。

`attempt_count` 是该调用内 Transport 总尝试数，**必须等于实际开始过的数量**，不得用配置的最大值代替。不为每个尝试建子 Span。

关键属性：

| 属性 | 语义 |
|---|---|
| `modelCallIndex` | 当前 Turn 内第几次 Model Call |
| `temperature` / `topP` | 采样参数。**改了参数行为变了，没有这两个就无从对照** |
| `maxOutputTokens` / `thinkingMode` / `toolChoice` | 其余请求参数 |
| `requestBuildMs` | 构建完整请求的耗时 |
| `ttftMs` | 首次 Transport Attempt 到第一个**有效模型事件**的耗时 |
| `streamMs` | 第一个有效模型事件到流结束 |
| `finishReason` | `stop/tool_use/length/content_filter/refusal` |
| `errorPhase` | `request_encode/connect/response_headers/response_body/stream_decode/response_decode/cancelled` |
| `deliveryState` | `not_sent/possibly_sent/accepted_no_semantic_output/semantic_output_emitted` |
| `httpStatus` / `providerCode` | **最后一次**尝试的传输结果 |
| `requestMessageCount` / `toolDefinitionCount` | 规模概览，让列表页不必加载正文 |

"第一个有效模型事件"指 Text、Reasoning 或 Tool Call 的首个语义事件，**不包括**连接、响应头和心跳。若调用在产生语义事件前失败，`ttftMs` 和 `streamMs` **保持为空，不用 `0` 冒充**已观察耗时；此时用 `errorPhase`、`deliveryState` 和 `httpStatus` 解释失败位置。

**不记录逐次 Transport Attempt 的明细。** 重试只增加 `attempt_count` 列，见 §15。

### 11.2 Tool Call

**开始**：完整 Provider Tool Call 已组装，即将解析参数、验证 Schema、决策权限。
**结束**：Tool Result Message 的持久化尝试完成，或在形成可持久化结果前明确失败/拒绝/取消。**不等待下一次 Model Call。**

标准列：`provider_call_id`、`requested_tool_name`、`resolved_tool_name`、`status`、`permission_wait_ms`。

**正文槽位通常为空**：参数在 assistant message 的 tool_use 块里，结果在 role=tool 的 Message 里，两者都由 `(turn_id, provider_call_id)` 定位。只有失败或被拒绝、没有产生结果 Message 时，才写 `response` 槽位。

**不记录未截断的完整工具输出。** 模型看到的是截断后的结果，那才是质量分析要的输入；完整输出是运维关注点，且一条 `bash` 就可能产生上百 MB。

`requested_tool_name` 是模型给的名称，`resolved_tool_name` 是 alias 路由后的真实实现，**二者不能互相覆盖**。

**权限不是独立 Span。** Tool Span 包围完整生命周期，`permission_wait_ms` 只记录等待用户决定的累计耗时。

关键属性：`permissionPolicy`、`permissionDecision`、`permissionDecisionSource`、`executionMs`、`artifactCount`、`artifactTypes`、`errorRetryable`、`resultPersisted`、`outputTruncated`。

`status` 表示**工具执行**的结果，持久化失败不得伪装成工具失败：工具成功但 Message 写入失败时，Span 保持 `succeeded` 并记 `resultPersisted=false`。

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

- **触发证据**：`trigger`、`contextWindowTokens`、`thresholdPercent`、`triggerEstimatedInputTokens`、`triggerPercent`；`overflow` 另记 `triggerModelSpanId` / `triggerErrorCode`。**`manual` 不针对窗口度量，因此不记录策略与触发估算，不用默认值伪造。**
- **压缩效果**：`conversationTokensBefore/After`、`reclaimedConversationTokens`。三者只度量 Conversation 区域并使用与 `ContextBudgetEstimate` 相同的口径，因此差值不被 System Context 或 Tool Surface 漂移污染。
- **分段耗时与结果**：`prepareMs/summaryMs/persistenceMs/installMs`、`sourceMessageCount`、`summaryChars`、`checkpointId`。

`prepareMs` 合并了原来的 `sourceCollectMs / stateCollectMs / systemContextMs`。三者都是本地亚毫秒操作，`summaryMs` 吃掉绝大部分时间，拆成三个字段没有让任何人做出不同的决定。

Compaction Span 自身**没有正文槽位**：成功的摘要在 `conversation_compactions.summary` 里，由 `checkpointId` 指过去。

摘要采样的明细落在**子 Span 自己的列**上，父 Span **不保留任何聚合**。子 Span 有完整的 `request` 与 `response` 槽位——**摘要质量差时要看的就是它当时读了哪些消息、写出了什么**，而失败的采样不进任何业务表，Trace 是唯一的落点。

各类尝试的次数由一条查询得到，不存 `attemptRollup`：

```sql
SELECT status, count(*) FROM trace_spans WHERE parent_span_id = $1 GROUP BY status
```

每个采样子 Span 的 `status` 就是它的分类结果，所以这个聚合 **by construction 恒等于**任何存下来的计数器——不是碰巧相等，是同一个事实的两种写法。存一份只多出一处可能不一致的地方。

子 Span 的 `status` 是分类结果：

| 分类 | 含义 | 重试有意义吗 |
|---|---|---|
| `succeeded` | 产出可用摘要 | — |
| `degenerate` | 有响应但不可用（过短、缺章节、被截断、请求了工具） | 是 |
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

Compaction 的摘要采样子 `model_call` 例外地使用 §11.3 的分类状态：`succeeded / degenerate / deterministic / input_overflow / transient / timeout`。这些值只允许出现在有父 Span 的 Model Call 上；普通 Model Call 仍只使用上表的生命周期状态。

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

- **expected** 来自 `turns.model_submission_count / tool_call_count`。这是**独立参照物**——由业务写入路径维护，与 Trace 写入路径互不依赖，两者一致才判定完整。**不要改成从 Span 计数派生**，那会让对账永远相等、失去意义。
- **captured** 是 `kind='model_call'` **且 `parent_span_id IS NULL`** 的 Span 数，以及 `kind='tool_call'` 的 Span 数。顶层过滤是必须的——摘要采样是 `model_call` 子 Span，一并计入会让 captured 恒大于 expected，把正常 Turn 误判成 `Partial`。
- **orphan** 是 `parent_span_id` 指向同一 Trace 中不存在的 Span。

### 为什么不统计"缺失的正文"

早先的设计里有一个 `spans_missing_payload`，统计"策略为 `full` 却没有 `request` 槽位"的 Model Span。**这一项已删除，因为它不可能被正确计算。**

`TraceContentPolicy` 是进程级配置，**不随 Span 持久化**。读一条三个月前的 Span 发现它没有正文，无法区分两种原因：

- 当时策略不是 `full`（**设计如此**）；
- 当时正文写入失败（**采集缺口**）。

拿**当前**进程的配置去解释**历史** Span 是猜测，会把"用户那阵子关了内容记录"报成数据丢失。

考虑过给每条 Span 记录当时的有效策略，否决了：它在 99% 的行上都是同一个值 `full`，过不了 §15 的第 3 问。为一个边缘诊断指标给每一行加一个近乎常量的字段，代价高于收益。

**后果要接受**：界面上"这里没有正文"只能陈述事实，不能声称原因。前端的处理方式见 [desktop.md](desktop.md)。

terminal Turn 的状态规则：captured 与 expected 分别相等且 orphan/running/outcome_unknown 全为 0 时 `Complete`；expected 至少有一个调用但 Model/Tool Span 全未采集时 `None`；其余 `Partial`。运行中的 Turn 可以展示实时计数，但**不能提前标记 `Complete`**。

## 13. 查询

```rust
list_turn_traces(filter, cursor, limit)      // Trace 列表，含标注
get_trace(turn_id)                            // 单个 Turn 的完整 Trace（不含正文）
get_trace_by_id(trace_id)                     // 无 Turn 的 Trace
get_span_payload(span_id, slot)               // 按需加载单个正文
list_compaction_spans(session_id, limit)      // Session 的压缩历史
upsert_annotation(trace_id, span_id, rating, note)
```

**正文必须是独立的按需查询。** 把它塞进 `get_trace` 会让打开一个 Turn 就拉走几 MB JSONB，而用户多数时候只想看时间线。列表页靠 `requestMessageCount` / `byte_size` 显示规模，点开某个 Span 才加载正文。

### 无 Turn 的 Span 必须有自己的读取路径

这是 Session scope 最容易漏的一步：**给 Span 加一个可空的 `turn_id` 很容易，但如果所有读取入口都以 Turn 为根，这些 Span 就是只写不读。**

`get_trace` 以 `turn_id` 为参数，`turn_id IS NULL` 的 Span 不可能出现在它的结果里。三条补齐路径：

- **`list_turn_traces` 按 `trace_id` 组织**，是两路来源的 `UNION ALL`：有 Turn 支撑的 Trace，以及 `turn_id IS NULL AND parent_span_id IS NULL` 的根 Span 各自成一条 Trace。后者的 `turn_id` / `turn_sequence` 返回空、调用计数为 0。Span 与 Turn 的状态词汇不同，无 Turn 那一路在 SQL 里把 `succeeded → completed`、`outcome_unknown → interrupted`，让列表只有一套状态语言。
- **`get_trace_by_id`** 让无 Turn 的 Trace 也能打开详情。手动压缩的摘要采样有完整正文，没有这条入口就看不到。
- **`list_compaction_spans`** 按 `session_id` 过滤、`started_at DESC` 排序，同时返回 threshold/overflow 的 Span，使一个 Session 的压缩历史读起来是一条完整列表。

**新增任何"可以没有 Turn"的 Span kind 时，必须同时给出它的读取入口。**

前端展示见 [desktop.md](desktop.md)。

## 14. 保留、清扫与删除

内容让 Trace 第一次成为**体积无界**的表，必须配套三件事。

### 截断

单个正文槽位有字节上限。默认是 1 MiB，由
`OpenWorkCoreConfig.trace_content` 中的 `TraceContentConfig` 配置；超限时截断，并置
`truncated = TRUE` 与 `original_byte_size`。

**截断了就必须说明原始多大**，由数据库 CHECK 强制——界面上一个无法量化的"已截断"警告没有用。

### 保留

按天保留正文，过期只删 `trace_span_payloads` 行，**Span 本身与 token 保留**。理由是 token 用量要能跨越很长时间比较，而正文的价值随时间迅速衰减。

**带 `trace_annotations` 的 Trace 不参与自动清理。** 被标为 `bad` 的调用是将来做回归的样本，正是最该留下的那些。

保留期放在 `OpenWorkCoreConfig.trace_content` 的 `TraceContentConfig.retention_days`，默认 **30 天**。30 只是第一版的起点，不代表正文价值存在一个精确的 30 天分界。过期时间以 `trace_spans.started_at` 计算，不能用 `trace_payloads.created_at`：正文按哈希去重，一条新 Span 可能引用数月前首次插入的 body。

仓库没有调度器，因此清理只在 bootstrap 时执行一次：Migration 与运行中状态收口之后、Recorder 启动和首个 Turn 被接受之前。桌面应用的启动频率足以满足按天保留，不为此增加常驻定时任务这个新运行部件。

### 孤儿清扫

`trace_payloads` 没有 `session_id`，删除 Session 不会级联删除正文。

**这是隐私必做项，不是空间优化。** 用户删了一个 Session，那个 Session 的源码内容必须真的从库里消失。因此清扫**必须在删除 Session 的同一次操作（同一事务）里执行**，不能只挂在周期任务上。

清扫**只针对刚被删掉的那些 Span 引用过的哈希**，不做全表扫描：

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

**全表 `NOT EXISTS` 扫描在并发下是不够的**，这是实现时暴露出来的，不是理论问题：

| 窗口 | 现象 | 防线 |
|---|---|---|
| 正文 body 已插入、mapping 尚未挂载 | 清扫看到一个"没人引用"的新正文并删掉它，随后 mapping 插入撞 FK 失败 | 正文挂载与清扫**共享一把事务级 advisory lock** |
| mapping 已存在 | 清扫试图删除仍被引用的正文 | `payload_hash` 的 `ON DELETE RESTRICT` |

两条防线管的不是同一件事：`RESTRICT` 保护**已经存在**的引用，advisory lock 补上**引用还没落地**的那一小段。**只有 `RESTRICT` 会让并发写入随机失败，只有锁则挡不住已提交的引用。**

限定候选哈希还有一个附带好处：清扫代价与被删 Session 的大小成正比，而不是与全表成正比。

保留策略复用同一路径：删除过期 mapping 时用 `RETURNING payload_hash` 得到候选，只把这批哈希交给同一个清扫 helper，并在同一事务、同一把 advisory lock 下完成。它不会另起一条全表孤儿扫描。

## 15. attributes 的边界

`attributes` 不接受调用点随意构造的 map。Core 定义并测试版本化的 `ModelTraceAttributesV1` / `ToolTraceAttributesV1` / `CompactionTraceAttributesV1`，由 Storage 统一序列化，**未知字段拒绝而不是静默透传**。

分工：

| | 放哪 |
|---|---|
| 标量、枚举、耗时、计数 | `attributes` |
| 正文 | `trace_span_payloads` |
| 跨 Trace 需要聚合或过滤的量 | 提升为列 |

**内容不进 `attributes`。** 混在一起会让每次读 Span 都拖着正文走，也让白名单校验无从下手。

限制：`error_message` 截断到固定长度；只允许白名单 key；字符串/数组有元素与字节上限；URL 只保留 scheme/host 或稳定 label。

### 新增一个属性的门槛

§9 给了新增 `kind` 的门槛，却一直没有给属性的——所以 kind 只有 3 个，属性一度长到 78 个。这不是意外，是漏洞。

**三问全过才配有一个字段：**

1. **有人会因为它做出不同的决定吗？** 「看着有用」不算。
2. **能不能从别的字段算出来？** 能算就别存——存下来只多出一处可能不一致的地方。
3. **是不是 99% 的行上都是空或同一个值？** 是的话它属于别的层级。

一个属性的真实代价不是存储（JSONB 里空字段不占空间），是**每加一个要改 6 处**：Rust 类型、序列化白名单、前端属性白名单、三个语言包。i18n 有测试强制，漏一个直接红。

比代价更重的是**理解成本**：一个详情面板平铺 50 多行，等于没有面板。所以删减必须配合分层，见下。

### 主字段与折叠区

每个 kind 标出主字段，详情面板默认只显示这些，其余进「详细」折叠区：

| kind | 主字段 |
|---|---|
| `model_call` | 模型、耗时、token、`finishReason`、`temperature` |
| `tool_call` | 工具名、`permissionDecision`、`executionMs`、状态 |
| `compaction` | `trigger`、前后 token、回收量、尝试次数 |

**分层比删字段收益大**，而且不丢信息。删减解决"字段太多"，分层解决"不知道哪几个重要"——后者才是实际用起来的障碍。

### 移除清单

**一、内容的影子。** 当年没有正文，只好用大小和计数近似。正文到位后它们失去意义：

```text
requestSystemMessageCount / requestUserMessageCount
requestAssistantMessageCount / requestToolMessageCount
requestContentBytes / toolDefinitionBytes
responseTextBytes / responseReasoningBytes / responseToolArgumentsBytes
inputBytes / outputBytes / outputLines / inputTopLevelKeyCount
```

保留 `requestMessageCount` 和 `toolDefinitionCount`：它们是语义规模，列表页不加载正文也要显示。字节数改由 `trace_payloads.byte_size` 提供。

**二、可派生的（第 2 问不过）。**

| 移除 | 因为 |
|---|---|
| `attempts[]`（嵌套 10 个子字段）| 见下，单列一节 |
| `attemptRollup`（嵌套 7 个计数器）| 子 Span 的 `status` 一条 `GROUP BY` 就是它 |
| `summaryAttemptOutcome` | 和采样子 Span 的 `status` 是同一个枚举值 |
| `summaryEstimatedInputTokens` | 前三项估算之和 |

**看着可派生但其实不是的**：`artifactCount` 和 `artifactTypes` —— types 是排序去重后的列表，count 是原始个数，两者不等价，都保留。第 2 问要看实际实现，不能看名字。

**三、没人据此做决定的（第 1 问不过）。**

```text
progressEventCount        进度事件个数改变不了任何判断
validationMs              schema 校验通常不到 1ms
resultPersistMs / resultPersistErrorCode   存储层自检，是运维问题
```

保留 `resultPersisted`——模型有没有看到这个结果是质量问题，它的耗时和错误码不是。

**四、错层的（第 3 问不过）。**

```text
appVersion    三个属性类型各带一份，每行存一次进程级常量。移到 turns
```

`sourceCollectMs / stateCollectMs / systemContextMs` 合并成 `prepareMs`（三个都是本地亚毫秒操作）。

### 为什么砍掉 `attempts[]`

这是唯一一块需要单独说明的，因为它砍掉之后有实实在在的能力损失。

`ModelTransportAttemptTrace` 每个元素 10 个子字段（`index/status/durationMs/errorCode/errorPhase/deliveryState/httpStatus/providerCode/providerRequestId/retryDelayMs`），一次调用最多 N 个。它是**旧定位留下的最后一块纯运维内容**——逐次重试的传输轨迹。

按新定位它没有位置：它回答的是「哪个 Provider 在抖」，而那是规模化服务的问题，本地单机应用没有这个场景。

**保留下来的**：`attempt_count` 列（重试了几次）、`errorPhase`、`deliveryState`、`httpStatus`、`providerCode`、`error_code`。「慢是不是重试造成的」仍然答得了——`attempt_count > 1` 加上总耗时就够。

**确实失去的**：每次尝试各等了多久（`retryDelayMs`）、中间某次尝试的独立错误码。要排查退避策略本身时会缺数据。这是接受的代价，不是忽略的问题。

**顺带解决一处重复**：`httpStatus` / `providerCode` 原本同时存在于顶层和每个 attempt 元素里。砍掉数组后，顶层那份成为唯一来源，语义明确为「最后一次尝试的结果」。

### 结果

| | 改动前 | Phase 1 之后 | 当前（Phase 3 完成） |
|---|---|---|---|
| `ModelTraceAttributesV1` | 31 | 31 | 22 |
| `ToolTraceAttributesV1` | 19 | 14 | 10 |
| `CompactionTraceAttributesV1` | 28 | 23 | 23 |
| **合计** | **78** | **68** | **55** |
| 嵌套类型 | `ModelTransportAttemptTrace`(10) + `CompactionAttemptRollup`(7) | 已删除 | — |

`ModelTraceAttributesV1` 在 Phase 1 一度持平（31 → 31）：删掉 `attempts` / `appVersion` / `summaryAttemptOutcome` 三个，同时加了 `temperature` / `topP` / `toolChoice` 三个。Phase 3 再删掉 9 个内容影子，最终为 22。

新加的 `temperature` / `topP` / `toolChoice` 第 1 问过得干脆利落：**改了参数行为变了，没有它们无从对照。**

落地分两步，现已全部完成：先删除不依赖正文的字段；正文与 `trace_payloads.byte_size` 接管后，再删除 13 个内容影子，避免中间出现既没有字节数也没有正文的空窗。

`summaryRetryDelayMs` **保留**——它是摘要重试的节奏，落在采样子 Span 上有意义，不属于被删的传输层明细。它没有出现在上面任何一组里，是清单的疏漏，不是待决项。

## 16. 子 Agent

设计见 [multi-agent.md](multi-agent.md)，落地形态与本节原先的预案一致：**子 Agent 自己是一个 Session**，`sessions` 增加 `parent_session_id` 与 `spawn_span_id`（指向发起它的 Tool Call Span），**Trace 结构不变，不新增 `kind`，不新增列**。

理由：子 Agent 本来就有自己的上下文窗口、对话和压缩，建模成 Session 是事实描述而非绕路。这样它天然拥有合法的 `turn_id`，不污染父 Turn 的调用计数，跨会话汇总走 `sessions` 的递归 join。

具体规则：

- 子 Turn 的 `trace_id` **等于子 `turn_id`**，按 §2 的主规则走，不继承父的；
- **不设跨 Trace 的 `parent_span_id`。** §12 的 orphan 定义是"指向同一 Trace 中不存在的 Span"，子 Agent 根 Span 若指向父 Trace 就永远被判成 orphan；
- 关联关系由业务列表达：`sessions.parent_session_id` 给拓扑，`sessions.spawn_span_id` 给"哪个 Tool Call Span 发起了它"。Trace UI 要跳转就用这两列。`spawn_span_id` **不建外键**——Trace 是 best-effort，那个 Span 可能因队列满而根本没落库，符合 §2「结构标识不建外键」；
- 父侧照常：`spawn_agent` / `wait_agent` / `list_agents` / `followup_task` / `interrupt_agent` 各产生一个普通的 `tool_call` Span。

**不要采用的两种形态**：让子 Agent 的 Span 共享父 `trace_id`（§12 的 expected 来自 `turns.model_submission_count`，captured 是顶层 `model_call` 数，混进来会让 captured 恒大于 expected，**每个用了子 Agent 的正常 Turn 都被误判成 `Partial`**——与摘要采样子 Span 当年的坑一模一样），或给子 Agent 单独插父 Session 下的 `turns` 行（占用会话轮次编号，并与"一个 Session 一个活跃 Turn"冲突）。

## 17. 常见诊断规则

这些是 UI/查询层推断，**必须标注为推断**，不写回业务状态：

| 现象 | 推断 |
|---|---|
| 回答质量突然变化 | 先对比 `temperature` / `topP` / `resolved_model_name`，再看 `request` 正文 |
| 压缩后回答开始跑题 | 读压缩子 Span 的 `response`，摘要可能丢了关键约束 |
| `request` 正文里没有某条用户消息 | 它在 checkpoint 的摘要区间内，被摘要替换了 |
| Model Call 很慢且 `attempt_count > 1` | Transport 重试贡献主要延迟 |
| `ttftMs` 高、`streamMs` 正常 | Provider 排队、连接或首包路径 |
| `request` 正文持续变大 | Conversation 或 Tool Schema 正在膨胀 |
| 估算与真实 `input_tokens` 长期偏差大 | bytes/4 不适合该模型，只能作为发送前近似 |
| Tool Call `permission_wait_ms` 占大部分 | 用户权限等待 |
| `outputTruncated=true` | 后续模型看到的是截断结果 |
| Tool Call failed，下一次 Model Call succeeded | 模型观察错误后完成了替代路径 |
| Turn failed 且 Trace partial | 只说明诊断不完整，**不能归因于缺失节点** |
| Tool Call `outcome_unknown` | 副作用不可确认，**不得自动重放** |

## 18. 明确不做

- **不建 `traces` 表。** Trace 层没有持久业务状态，状态、耗时、计数全可从 Span 派生。
- **不建 `trace_span_events` 表**，也**不在属性里存逐次 transport attempt**（§15）。重试只增加 `attempt_count`。
- **不存任何可由子 Span 聚合出来的计数器**（`attemptRollup` 是被删的那一个）。
- **不复制 `messages` 已有的内容**（§6）。
- **不记录完整未截断的工具输出**（§11.2）。
- **不给 `parent_span_id` 建外键**（§10）。
- **不做自动评分与评测集**（§8）。
- **不记录成本**——不存单价、不算金额、不做汇总。见下。

> **对旧设计的反转：** 早期文档写着"不建 Payload 表"，理由是 Trace 不该碰内容。那条随本次定位调整作废——内容正是质量追踪的主体。保留的是它背后那半条正确判断：**内容不能进 `trace_spans` 的行里**，所以才有独立的两张表和按需加载。

### 为什么撤掉成本

成本曾经实现过一遍，跑通了全部验收，然后整体撤除。撤除的理由不是"没做完"，而是**做完之后发现它在日常使用中基本失效**：

任何失败或**被取消**的 Model Call 都会把整个 Turn 的成本标为未知，而 Session 汇总只要有一个未知 Turn 就整体显示"—"。取消是日常操作，一个会话里取消过一次，成本显示就永久失效了。

更深的原因是 `turns.cost_amount` 用一个可空列同时表达两种状态：

- 「还没记录任何成本」
- 「有一项无法定价，总额不可知」

一个 NULL 区分不了这两者，于是累加逻辑只能靠 `model_submission_count = 1 AND NOT EXISTS(压缩)` 这类代理条件去猜"我是不是第一个"。这些条件在顺利路径上成立，在任何失败路径上全部失效——overflow 重试的 Turn 即使每一步都有价格，最终成本仍是空。

要修就得引入一个显式的成本状态列，并区分"请求未发出"和"已发出但结果未知"（`deliveryState` 已经有这个信息）。那是一次完整的设计迭代，不是补丁。

**当前的判断是：这个功能的价值配不上这个复杂度。** token 已经记录，需要估算花费的人可以自己乘单价。

留下来的是两样东西：**§7 的 Provider token 口径表**（那是 token 语义，和计价无关），以及 `TokenUsage.cache_creation_input_tokens` 的解析。**单价列、金额列、汇总查询和界面全部移除。**

若将来重做，先解决状态二义性，再写代码。

## 19. 尚未实施

- **正文的 Core 写入、Desktop 按需读取、截断、运行时内容策略、按天保留与孤儿清扫已经实施。** 标注仍只有 schema 和设计。**成本已从设计中移除**，理由见 §18；
- Kimi 的 `cached_input_tokens` 集合关系仍未从官方资料验证，§7 表中标为未验证；
- **§15 的移除项已经实施**：逐次 Transport 明细、父级 attempt 聚合、低价值 Tool 耗时/计数、重复 `appVersion` 与 13 个内容影子均已删除，三个压缩准备耗时已合并为 `prepareMs`；
- **主字段/折叠区分层已经实施。** 属性白名单的每个 key 都有编译期穷尽归类；
- `temperature` / `topP` / `toolChoice` 已进入 Trace；`topP` 同时补入 `ModelRequest` 并由三个 Provider codec 发出；
- `flush_turn/flush_session` 的 dropped/write-failure 计数尚未暴露到界面或运维出口。这一条优先级会随并发上升；
- Attempt 分类尚未用于重试决策；
- 自动压缩失败后没有抑制状态；
- `TraceContentPolicy` 已由 Desktop 在启动门控阶段推给 Core，并可在运行时原子切换；`NoopTraceRecorder` 只用于测试装配；
- `trace_span_payloads.redacted_count` 恒为 `0`，是应当删除的列，见 [data-model.md](data-model.md)；
- 未直接声明 `tracing` 或 OpenTelemetry 依赖。以后接入时由同一生命周期 Guard 同时更新领域 Trace 与 `tracing::Span`，OTLP 默认关闭且失败不影响 Turn。

## 20. 验收

### 内容

1. 一次 Model Call 的 `request` 槽位能完整还原当时提交的 provider-neutral 消息数组；
2. 压缩之后，`request` 正文里是**摘要 + 边界后的原始消息**，而不是全部 `messages`；
3. 成功的 Model Call **不**写 `response` 槽位，而是 `response_message_id` 指向 Assistant Message；
4. 失败的 Model Call 写 `response` 槽位（若已收到部分响应），且没有 `response_message_id`；
5. Tool Call 默认没有任何正文槽位；被拒绝的 Tool Call 才写 `response`；
6. 同一 Session 内多次调用的相同 `tool_definitions` 在 `trace_payloads` 中只有一行；
7. 超过上限的正文被截断，`truncated = TRUE` 且 `original_byte_size` 非空——**数据库拒绝只置 `truncated` 不给原始大小的行**；
8. `TraceContentPolicy = off` 时不产生任何 `trace_payloads` 行，Span 与 token 不受影响；
9. `compaction_only` 时只有 Compaction 及其子 Span 有正文；
10. 正文写入失败时 Span 本身仍落库；
11. 任何档位下，正文中都不出现 API Key、凭证、HTTP Header；
12. `get_trace` **不**返回正文；正文只经 `get_span_payload` 按需加载。

### 标注

13. 同一目标重复标注是更新而不是新增一行；
14. 标注既能挂到整条 Trace（`span_id IS NULL`）也能挂到单个 Span；
15. 带标注的 Trace 不被保留策略清理；
16. 删除 Session 级联删除它的标注。

### 保留与隐私

17. 正文过期后 Span 与 token 仍在，过期的 `trace_span_payloads` 行消失；由此产生的无人引用 body 按第 18 条清扫；
18. **删除 Session 或过期正文映射后立即执行孤儿清扫，`trace_payloads` 中不残留已经无人引用的独有内容**；
19. 仍被引用的正文无法被清扫删除（`RESTRICT` 生效）。

### 结构

20. 同一次用户请求产生的全部 Span 共享一个 `trace_id`；
21. 手动压缩产生的 Span 有自己的 `trace_id`，且 `turn_id IS NULL`；
22. rewind 同上，且 `model_id` 为空、无子 Span；
23. `trace_id` 非空约束生效：缺少它的写入被数据库拒绝，而不是静默落库；
24. Trace 表无 `sequence` 列，也无 `(turn_id, sequence)` 唯一约束；
25. **并发写入同一 Trace 的多个 Span 全部落库**，不因排序键冲突丢失任一条；
26. 排序仅由 `started_at` 决定，`id` 作为稳定 tiebreak，同毫秒内顺序可重现；
27. 父 Span 未落库时子 Span 仍成功写入并被统计为 orphan；
28. `kind='model_call'` 不允许携带 tool 独占列，`response_message_id` 不允许出现在非 model_call 上，由数据库约束拒绝。

### 压缩

29. 四类触发各产生且只产生一个 Compaction Span；
30. threshold/overflow 复用触发请求的 `trace_id` 并携带 `turn_id`；manual/rewind 新开 `trace_id` 且 `turn_id` 为空；
31. 摘要采样是子 Span，每次有自己的 `provider_request_id`、token 列和正文；
32. **失败的摘要采样其 `response` 正文可读**——它不进任何业务表，Trace 是唯一落点；
33. `SELECT sum(input_tokens) WHERE kind='model_call'` 包含压缩开销；
34. 摘要采样**不**增加 `turns.model_call_count / model_submission_count`；
35. 有子 Span 的 Turn 其 completeness 仍为 `Complete`；
36. overflow 的 `triggerModelSpanId` 指向那次真实失败的 Span，且该 Span 状态为 failed；
37. manual 压缩**不**记录窗口、阈值与触发估算；
38. 每次压缩都记录前后 token，且 `reclaimed = before - after`（不小于 0）；
39. 摘要尝试按六类分类，**分类结果就是子 Span 的 `status`**，父 Span 不存任何聚合；
40. 摘要连续失败时 Span 为 failed 且带错误码，**checkpoint 未安装、Conversation 未改变**；
41. `list_compaction_spans` 返回该 Session 的全部压缩，按 `started_at DESC`，`limit` 生效；
42. **手动压缩在 Desktop 上可见且可打开详情**——存在从 `/compact` 到界面看到摘要正文的端到端用例。

### 基础

43. 无工具 Turn 产生一个 Model Span；
44. Model → Tool → Model 产生两个 Model Span 和一个正确挂载的 Tool Span；
45. Tool Name Alias 同时保留 requested/resolved；
46. Permission Wait 只增加 Tool Span 的等待耗时；
47. Provider Retry 只增加 `attempt_count`，**既不产生独立 Attempt Span，也不写逐次明细数组**；此时 `httpStatus` / `providerCode` 反映最后一次尝试的结果；
48. Model Span 不可写入 `denied`；
49. terminal Span 必有 `ended_at` 且 `ended_at >= started_at`；
50. 启动时遗留 Running Span 变为 `outcome_unknown`；
51. Trace API 能识别 Complete/Partial/None；**正文有无不参与这个判断**——完整度只对账 Span 结构，不对账内容；
52. Model Span 的 Provider 耗时不含 `build_request`，三段耗时可分别验证；
53. Tool Span 能区分 permission/execution，并以 `resultPersisted` 区分结果是否进入 Conversation；持久化失败时仍产生 terminal Span；
54. `temperature` / `topP` 被记录，修改后新 Span 反映新值。

### 降级

**底线：Trace 可以丢，业务不能受影响。**

55. Trace Queue 满时 Turn 结果不变；
56. 数据库 Trace 写失败时 Message 仍提交；
57. 压缩的 Trace 写入失败时 checkpoint 仍安装、Conversation 仍替换；
58. `attributes` 只接受白名单字段，超长字符串/数组被截断或拒绝；
59. 若启用 `tracing`/OTLP 出口，其关闭、丢弃或导出失败不改变 PostgreSQL Trace 和 Turn 结果。

### 时间口径

60. 时间列存东八区墙上时间，见 [.claude/rules/database.md](../.claude/rules/database.md)；
61. 出库字符串带 `+08:00` 而非 `Z`——**存在一个断言后缀的用例**。标错时区不会报错，只会让界面整体偏 16 小时；
62. 一个瞬间经"落库 → 序列化 → 前端解析"往返后仍等于原瞬间。
