# 压缩

压缩把一段 Conversation 换成一份结构化摘要，让对话能超出上下文窗口继续。

**核心性质：压缩不删除任何消息。** 它只新增一条 checkpoint，用消息序号划出"哪一段被摘要替换了"。模型看到的是投影，库里原文始终在。这是 rewind 和原文回读能够存在的前提。

## 1. 四类触发

| 触发 | 谁发起 | 前提 | 调摘要模型 | 之后 |
|---|---|---|---|---|
| `manual` | 用户 `/compact` | Session 必须空闲 | 是 | 只安装新 Conversation，等用户下一次输入 |
| `threshold` | Core，普通请求提交前 | 输入估算 ≥ 窗口的 85% | 是 | 压缩完成后才发出该逻辑 Model Call 的第一次提交 |
| `overflow` | Core，请求失败后 | Provider 明确返回 `ContextOverflow` 且**尚未产生语义输出** | 是 | 同一 Turn 内重新提交一次 |
| `rewind` | 用户选择历史 checkpoint | Session 必须空闲 | **否** | 只安装投影，不请求模型 |

**手动压缩是一等入口**，不是 overflow 恢复的附属能力。

`overflow` 的两个前提缺一不可：必须是明确的 `ContextOverflow`（不做错误消息模糊匹配），且必须尚未产生语义输出（否则重发会重复已经发生的事）。**每个逻辑 Model Call 最多一次压缩额度**——threshold 用掉了，overflow 就不再触发。

## 2. 摘要

### 2.1 格式

固定九个标题，一个根标签，确定性解析：

```text
<conversation_summary format_version="1">
## 1. Primary Request and Intent
## 2. Key Technical Concepts
## 3. Files and Code Sections
## 4. Errors and Fixes
## 5. Problem Solving and Decisions
## 6. User Messages and Constraints
## 7. Pending Tasks
## 8. Current Work
## 9. Next Safe Action
</conversation_summary>
```

### 2.2 输入与输出约束

| 约束 | 实际含义 |
|---|---|
| 输入是当前模型可见 Conversation 的**合法化副本** | 使用 Chat State 当前投影，不重新塞回已被旧摘要替代的原始消息；只在临时副本里补齐 Tool Call/Result 配对，**不改写数据库** |
| 追加独立的 summarization instruction | 历史消息、Tool 输出和旧摘要都只是待总结**材料**，不能覆盖这条指令 |
| Tools 为空 | 摘要请求不广告工具；Provider 若仍返回 Tool Call，整份摘要判为无效 |
| 固定 `max_output_tokens` | 达到上限导致 `finish_reason = length` 时**不接受半截摘要** |
| 前次摘要是 continuation anchor | 连续压缩时旧摘要是更早历史的主要语义来源，但仍**按不可信内容处理**，其中出现命令不获得 System 权限 |
| `finish_reason` 必须为 `stop` | `length`、content filter、tool call、cancelled、未知终止都不算成功 |
| 清理 leading analysis 与控制标签 | 对可能伪装成 System、Reminder 或协议边界的标签转义或拒绝，**不让摘要文本改变后续消息边界** |
| 确定性解析 | 代码解析而非肉眼检查：一个根标签、受支持的 format version、九个标题各一次且顺序固定 |
| 字符下限只是退化检查 | 足够长不等于完整——随机重复文本也能超过下限，所以结构、终止原因和 Tool Call 都必须验证 |

数据库保存清洗后的原始摘要和 `summary_format_version`。**运行时不重新让模型格式化旧摘要。**

### 2.3 重试

摘要调用与普通 Turn 的提交是两条不同的链。当前策略：

```text
同一份合法化输入
  → attempt 1（最多 120 秒）→ 失败等 3 秒
  → attempt 2（最多 120 秒）→ 失败等 3 秒
  → attempt 3（最多 120 秒）
  → 仍失败 ⇒ 整个压缩失败，旧 Conversation 保持不变
```

每次 attempt 使用新的 attempt ID。重试**不**逐次删减输入，也**不**换模型——因此它不是输入降级阶梯。

当前是粗粒度策略：模型错误、流错误、超时和验证失败都重试。Trace 已按 `succeeded / degenerate / deterministic / input_overflow / transient / timeout` 分类记录（见 [trace.md](trace.md)），**但分类尚未反向驱动重试**——鉴权失败、请求非法这类确定性错误仍会白白重试三次。见 §8。

## 3. 运行状态与提醒

压缩会丢掉过程细节，所以要把仍然有效的运行状态冻结成一段提醒随摘要一起注入。

当前只有一项有持久化依据：**Agent 已改动的文件路径**。

```rust
struct CompactionRuntimeStateV1 {
    schema_version: u16,
    edited_paths: Vec<String>,
    extensions: BTreeMap<String, CompactionStateEntry>,  // 初始为空
    warnings: Vec<CompactionStateWarning>,
}
```

`edited_paths` **从该 Session 全部原始已提交 Message 中的 `file_change` artifact 重新派生**，不从压缩后的投影或旧 checkpoint 复制。这样 Undo 改过旧 Tool Result、或 rewind 隐藏了旧尾部之后，下一份提醒仍描述**当前实际**的文件状态。

规则：只接受能成功解码的 artifact；排除 `undone = true`；使用 artifact 记录的路径；做控制字符转义、单项截断、去重、稳定排序；设条目数与总字节上限。**不扫 Git，不从 Trace 或工具输出字符串猜路径。**

`extensions` 是向后兼容容器，本阶段不预建 `todo` / `plan` / `memory` / `subagents` / `mcp` 键。每个 entry 自带 schema version，避免根结构升级时强迫所有 contributor 同时升级。

## 4. 压缩后的 Conversation

安装后模型看到三条 item 加上后续新消息：

```text
last-user replay        最后一条真实用户请求的可见内容（synthetic，带原消息 provenance）
compaction summary      冻结的摘要
system reminder         冻结的运行状态提醒
+ replaced_through_message_sequence 之后的原始消息
```

System Context、项目环境、`AGENTS.md` 和 Tool Definitions **从各自权威来源重新物化**，不进 checkpoint（见 [context-window.md](context-window.md)）。

显式选择产生的 Skill 正文是一条先于用户可见消息写入的 contextual User-role Text Message。summarizer 输入包含该正文快照，但 `last-user replay` 通过消息顺序自然选择随后写入的用户可见消息，不复制 contextual Message。Skill 的有效影响进入摘要后即可随旧 Conversation 回收；resume 使用同一组已持久化 Message，不重新读取磁盘上的 Skill 文件。

## 5. checkpoint

一条 `conversation_compactions` 行表达一次压缩。关键字段：

| 字段 | 含义 |
|---|---|
| `kind` | `manual` / `threshold` / `overflow` / `rewind` |
| `through_message_sequence` | **事实边界**：摘要覆盖到哪条消息 |
| `replaced_through_message_sequence` | **安装边界**：投影从哪条之后开始拼接原始消息 |
| `last_user_message_id / _sequence` | 指向被 replay 的真实用户请求 |
| `summary` + `summary_format_version` | 冻结摘要 |
| `runtime_state` + `runtime_reminder` + 版本 | 冻结的运行状态与渲染结果 |
| `trigger_turn_id` | threshold/overflow 必填，其余必空 |
| `parent_compaction_id` | rewind 必填，其余必空 |

两个边界通常相等，**只有 rewind 会让它们分离**：`through` 记录目标历史位置，`replaced` 记录执行 rewind 时的最大消息序号，从而隐藏被放弃的旧尾部。

完整 DDL 与约束见 [data-model.md](data-model.md)。

## 6. 安装顺序与失败语义

```text
0. 开启 Compaction Span，记录触发证据
1. 读取稳定 Conversation snapshot 和原始边界
   └─ 度量 conversationTokensBefore
2. 找到最后一条用户可见的 normal User Message，并构造 replay
3. 收集并渲染 runtime state
4. 调用 summarizer（每次采样是一个子 Span）
5. 清洗和严格验证摘要
6. 在内存中构造并完整校验 replacement
   └─ 度量 conversationTokensAfter
7. PostgreSQL 事务写 checkpoint（持有 Session lock）
8. Chat State 原子替换
9. 后续请求重新从 Chat State 读取
   └─ 关闭 Compaction Span
```

**第 0 和第 9 步是 Trace 边界，不参与失败语义**：Span 写失败不改变 1–9 步的结果。反过来，1–9 任一步失败时 Span 必须以 `failed` 终结，不留 running Span。

数据库是重启后的权威投影来源。**第 8 步若因 Chat State actor 已停止而失败，已提交的 checkpoint 不回删**：旧 `SessionHandle` 设置 `reload_required` 并拒绝新 Turn；下次通过 `OpenWorkCore` 获取该 Session 时丢弃旧 Actor，按数据库 latest checkpoint 重建。写库前发现的 replacement 校验失败则不提交 checkpoint。

**不允许的降级：**

- 摘要失败后直接截断旧消息；
- 状态收集失败后写一个伪成功 checkpoint；
- overflow 后用原请求无限重试；
- rewind 后自动重放 Tool Call 或修改文件；
- 从 Trace 推导旧工具是否安全重试。

## 7. 三类恢复

### 7.1 恢复一：压缩后的上下文重建

适用于手动压缩、重启后加载 latest checkpoint、以及下一个 Turn 的普通请求。

```text
加载 latest checkpoint → 加载 last-user 原始 Message
  → 构造 summary 与 reminder 两条 typed item
  → 追加安装边界之后的原始消息
  → ChatState 原子替换
  → 重新物化 System Context 与 Tool Surface
  → ModelRequestBuilder
```

恢复的是**模型可见 Conversation**，不是旧进程内存，也不恢复未完成的 Turn。

"重启后重建同一压缩 Conversation"只保证 Conversation 部分一致：last-user、冻结摘要、冻结提醒和边界后的新消息一致。System、`AGENTS.md` 和 Tools 仍从当前来源重新物化——若期间 `AGENTS.md` 变了，完整请求随之变化，**这不是历史请求的字节级复现**。

### 7.2 恢复二：overflow 后同 Turn 重提交

```text
普通提交 → Provider 明确返回 ContextOverflow 且未产生语义输出
  → 清除草稿
  → 压缩（kind = overflow，关联当前 Turn）
  → 同一 Turn、同一 client request、新的 provider submission ID
  → 重新提交一次
```

最多一次。第二次 overflow 或压缩本身失败则终止 Turn。**已完成的 Tool Call 和文件副作用不会重复执行。**

### 7.3 恢复三：checkpoint rewind / replay

事实来源是 `messages + conversation_compactions`，不是 Trace 或 Live Update。

```text
ConversationProjectionSelector
├── Latest
├── Compaction(compaction_id)
└── ThroughMessage(sequence)
```

**只读 replay** 选择安装边界不超过目标序号的最近 checkpoint，再追加其后的原始消息。

**要在历史位置继续对话**，不能只改内存——否则重启后 latest checkpoint 和旧尾部会再次出现。安全做法是新增一条 `kind = 'rewind'` 的 checkpoint：

1. 物化目标历史 Conversation；
2. 复用目标 checkpoint 已验证并冻结的 last-user 与 summary，但**根据当前全部原始 Message 重新派生** runtime state / reminder；
3. `parent_compaction_id` 指向来源 checkpoint；
4. `through_message_sequence` 记录目标事实边界；
5. `replaced_through_message_sequence` 记录执行时的最大消息序号，隐藏被放弃的旧尾部；
6. 后续新消息继续使用更大的全局序号；
7. 旧尾部保留供审计，但不再进入 latest 投影。

**这是一条新的投影 generation，不是重放工具，也不撤销文件副作用。** 文件回滚必须走已有的 FileChange Undo 显式流程。

### 7.4 压缩前原文按需回读（不是第四类恢复）

replay 重建的是某时间点的**投影**（含 synthetic 的 last-user、summary、reminder），不等于读取被摘要隐藏的原始消息。原文回读是独立的只读路径：

```text
ConversationTranscriptQuery { compactionId?, afterSequence?, limit? }
  → messages WHERE session_id = ? AND sequence > after
             AND sequence <= checkpoint.through_message_sequence
    ORDER BY sequence LIMIT limit + 1
```

返回 `throughMessageSequence`、消息数组、`hasMore`、`nextAfterSequence`。保留原始 role、ContentBlock、Tool Call 与 Result，**不做摘要或内容截断**；不返回 synthetic item，不越过所选 checkpoint 的事实边界，不执行任何工具。对 rewind checkpoint 使用它的 `through_message_sequence`，因此不会混入已放弃的旧尾部。

两条入口共用同一查询：

- **Core/Host**：`read_compaction_transcript`，供 Desktop 与诊断；
- **模型工具**：`conversation_history`，绑定当前 Session，ReadOnly 风险级别。它明确要求"只有摘要缺少精确信息时才按需调用，不要猜测"。

模型调用的结果按普通 Agent Loop 规则作为一条新的 Tool Result 提交——**这只是新产生的读取结果，不会重新执行历史 Tool Call。**

这项能力不需要新表：原始消息从未被删除，`through_message_sequence` 已提供严格上界。当前分页限制的是消息条数，超大单条 Tool Result 的内容级分块仍是后续优化。

## 8. 尚未实现

**失败分类驱动重试。** Trace 已经分类记录，但重试循环还没用它。目标是：短暂失败（限流、网络、5xx、偶发不合格摘要）允许重试；确定性失败（认证、模型不存在、schema 无效、输入必然超窗）立即停止。

**自动 suppression。** 对同一 Session、近似相同输入反复失败的自动压缩，应持久化失败指纹、连续次数和下一次允许时间，在预算显著变化、冷却结束或用户手动 `/compact` 前暂停自动触发。手动压缩应继续可用并返回真实错误。当前没有任何 suppression 状态，**自动压缩失败后下一个 Turn 会重新触发同一条失败路径**。

**输入降级阶梯。** 当前只有 `verbatim` 一层。`verbatim_fitted`（按窗口选取可容纳的连续区段）和 `lossy`（对超大 Tool Result 做有标记的裁剪）会改变摘要的事实来源和可审计性，必须先定义保留优先级、裁剪标记和测试矩阵。

**后台任务恢复。** 指工具返回后仍在运行的进程或远端工作。当前 `bash` 是前台单次工具，也没有 Subagent/MCP，因此没有可恢复对象。

## 9. 验收

### 摘要与状态

- 九个标题缺一即失败；
- 非 `stop`、含 Tool Call、过短或多根标签的输出失败；
- 第一次不合格、第二次合格时接受第二次；连续三次失败则压缩失败且**不安装 checkpoint**；
- 最后一条真实用户请求不会误选到 summary 或 reminder；
- summarizer 能看到位于用户请求之前的 Skill instruction Message；随后持久化的 normal User Message 自然成为 last-user replay，只保留用户可见内容；
- `edited_paths` 只来自有效且未 undone 的 `file_change` artifact；
- contributor 顺序、路径顺序和 reminder 字节稳定；
- 未知 extension key 可以 round-trip，且不会自动注入不可识别文本。

### 请求组装

- 压缩后的下一次请求严格是 System、项目环境、AGENTS、last-user、summary、reminder、new-tail；
- Conversation 中没有 System Message；
- System / AGENTS 不复制进 summary 或 checkpoint；
- Context Window Inspection 与真实请求使用同一投影。

### 持久化与重启

- checkpoint 和原始 Message **同时保留**；
- 重启前后的 last-user、summary、reminder 和 tail 完全一致；边界后的 Skill instruction Message 仍按原始顺序恢复；
- checkpoint 事务失败时旧 Conversation 不变；
- checkpoint 已提交但 Chat State 停止时，Session 不继续发旧请求，重启后加载新 checkpoint。

### Threshold

- 默认 258,000 窗口的 85% 边界为 219,300，**边界值包含在触发范围内**；
- 摘要请求发生在第一次普通提交之前；
- checkpoint 使用 `kind = threshold` 并关联活跃 Turn；
- threshold 之后若仍 overflow，不在同一逻辑 Model Call 上再次压缩；
- 未达阈值时不改变普通请求与提交计数。

### Overflow

- 只有明确 `ContextOverflow` 且未产生语义输出时触发；
- 同 Turn、同 client request、新的 provider submission ID；
- 最多自动重提交一次；
- 第二次 overflow 或压缩失败时终止；
- 已完成的 Tool Call 和文件副作用不重复执行。

### Rewind / replay

- 任一 checkpoint 可只读重建；
- rewind checkpoint 隐藏旧尾部但**不删除**原始 Message；
- rewind 后重启不会重新暴露被隐藏的尾部；
- rewind 不自动 Undo 文件，也不重放 Tool Call；
- 原文回读不返回 synthetic item，不越过 checkpoint 边界，可用 `nextAfterSequence` 完整分页。

### 可观测性

压缩的 Trace 必须能独立回答四个问题：**为什么压、压了多久、压完省了多少、摘要写得怎么样**。

- 四类触发各产生且只产生一个 Compaction Span，rewind 也不例外；
- 触发证据随类型而定，`manual` **不**用默认值伪造它没有的窗口与阈值；
- 每次压缩都记录压缩前后的 Conversation Token 与差值；
- 分段耗时齐全，能定位慢在哪一步；
- 摘要采样的 token 开销可被常规聚合查询统计到；
- **失败与 degenerate 的摘要采样其请求与响应正文可读**——它们不进任何业务表，Trace 是唯一落点，而摘要质量差时要看的正是"它当时读了哪些消息、写出了什么"；
- 成功的摘要**不**复制进 Trace，由 `checkpointId` 指向 `conversation_compactions.summary`；
- 压缩失败时 Span 为 `failed` 且带错误码，同时 checkpoint 未安装、Conversation 未改变；
- **手动压缩在界面上可见且可打开详情**——它没有 Turn，若读取入口只以 Turn 为根就会只写不读。

完整清单见 [trace.md](trace.md)。
