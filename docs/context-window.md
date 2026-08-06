# 上下文窗口分层

模型每次调用看到的输入由**三条互相独立的物化链**产生，它们只在一个集中的组装边界汇合。

"物化"指：从某类权威来源读取当前状态，按确定规则生成一份只读结果。**物化结果不是新的业务真相，也不反向拥有来源。**

## 1. 为什么要分三条

三条链的权威来源、变化频率和生命周期完全不同：

| 链 | 权威来源 | 什么时候变 |
|---|---|---|
| System Context | Agent 定义 + 工作目录文件 | 换 Agent、改 `AGENTS.md` |
| Conversation | Chat State | 每次消息追加、每次压缩 |
| Tool Surface | Agent 工具策略 + 权限 + 注册表 | 换 Agent、权限变化 |

把它们混在一起，任何一方变化都会污染另外两方——最典型的是压缩：它只该替换 Conversation，若三者纠缠，摘要就会变成 System Source 的副本。

## 2. 三条链

### 2.1 System Context

```text
Agent Definition ──→ Agent System Prompt
工作目录 ──→ ProjectInstructionLoader ──→ AGENTS.md
                                    ↓
                          ResolvedSystemContext
```

它可以读取来源、判断生命周期、确定性排序和渲染；**不读 Conversation、不选工具、不构造 `ModelRequest`。**

生命周期：每个 Turn 开始解析一次，同一 Turn 内多次 Model Call 复用同一份；下一个 Turn 重新读取。

约束：

- Agent System Prompt 始终在 Project Instructions 之前；
- 同一个 key 重复时**确定性失败**，不静默去重；
- 缺失或空白不产生空 System Message；
- 拒绝符号链接和超过 64 KiB 的文件。

### 2.2 Conversation

```text
已提交的 User / Assistant / Tool Message
+ 最新压缩 checkpoint 与边界之后的新消息
+ 类型化的 synthetic item
      ↓
  Chat State（单写者）
      ↓
  ConversationView
```

未压缩时是完整的已提交 Conversation；压缩后是"最后一条真实用户请求 + 摘要 + 运行提醒 + 边界之后的新消息"（见 [compaction.md](compaction.md)）。**每次 Model Call 重新读取当前一致视图**，不缓存。

约束：

- `ConversationView` **不允许包含 System Message**；
- 不包含流式草稿；
- 系统生成的 User-role item 必须带结构化 provenance，不能与真实用户输入混淆。

Chat State **不**加载 `AGENTS.md`、Memory、Plan 或 Skill 的权威数据，**不**接收 Model 或 Tool Definitions。只有当某个功能产生"应当作为 Conversation 发送且需要重放"的条目时，该条目及其 provenance 才进入 Chat State。

### 2.3 Tool Surface

```text
Agent 工具策略 + 权限/运行时能力 + Tool Registry
      ↓
  FinalizedToolset { definitions, dispatch }
```

**Definitions 和 Dispatch 必须来自同一份物化结果。** 这条不变量防止"广告了但调不动"或"能调但没广告"。

它可以做权限过滤、能力选择和确定性排序；**不读 Conversation、不加载 System Context。**

## 3. 组装边界

请求组装**不是第四条链**，它是三条链之后的集中汇合点：

```text
ResolvedSystemContext + ConversationView + ToolSurface.definitions + 解析后的模型设置
      ↓
  ModelRequestBuilder::build
      ↓
  ModelRequest（provider-neutral）
```

它只做确定性工作：

- 校验 System Context key 非空且不重复；
- 保证 Conversation 中不出现 System Message；
- 固定 System Context 在 Conversation 之前；
- 计算只读的 Context Budget Estimate；
- 生成 `ModelRequest`。

**它不能**读取项目文件、查询 Memory、修改 Chat State、选择或推进 Tool Call、调用 Provider。

Provider Adapter 位于组装之后，只负责把 `ModelRequest` 编码成各家协议。

## 4. 预算估算

`ContextBudgetEstimate::measure` 分别测量三个区域，得到 provider 无关的预发送估算：

```rust
struct ContextBudgetEstimate {
    system_context_tokens,
    conversation_tokens,
    tool_surface_tokens,
    estimated_input_tokens,   // 三者之和
    reserved_output_tokens,
}
```

口径是**序列化字节数 / 4**。它是粗略近似，用途只有两个：

1. Desktop 的上下文用量展示；
2. 采样前压缩的阈值判断（见 [compaction.md](compaction.md)）。

**它不改变请求本身**——不截断、不拒绝、不重排。与 Provider 返回的真实 `input_tokens` 长期偏差很大时，说明该模型的分词与这个估算不匹配，只能当作发送前的量级参考。

压缩前后的对比只测量 Conversation 区域（`estimate_conversation_tokens`），使用同一口径，因此差值不会被 System Context 或 Tool Surface 的漂移污染。

## 5. 不变量

**当前成立：**

1. System Context 不写入 Conversation；
2. `ConversationView` 不含 System Message；
3. Agent System Prompt 始终在 Project Instructions 之前；
4. System Context 始终在 Conversation 之前；
5. Tool Definitions 与实际 Dispatch 来自同一 `FinalizedToolset`；
6. Provider Adapter 不加载任何 Context Source；
7. Context Budget 只测量，不改变请求。

**未来能力必须继续满足：**

1. 三条链分别物化，只在组装边界汇合；
2. Chat State 可以拥有 synthetic item 和已解析的 Skill 快照，但**不读取或拥有**其上游 Memory / Plan / Skill / Project Instruction 来源；
3. 系统生成的 User-role contextual message 必须带 provenance；显式 Skill 指令在正文标记中保留 canonical `name + path`，持久层以 `message_kind = 'skill_instruction'` 区分它和用户可见消息；
4. **压缩只替换 Conversation 投影**，摘要不得成为 System Source 或 Tool Surface 的权威副本；
5. 压缩或 resume 之后必须**重新物化三条链**再构造请求；
6. 来源与顺序未变化时，稳定前缀应保持字节一致；
7. `ModelRequestBuilder` 保持纯组装，不因加入新能力而去读它们的存储。

## 6. 未来能力接在哪条链

新增能力时先判断它属于哪条链，而不是直接塞进 Chat State 或 `ModelRequestBuilder`：

| 能力 | System Context | Conversation | Tool Surface | 权威存储 |
|---|---|---|---|---|
| Memory | 稳定的记忆规则 | 检索结果，带 synthetic provenance | memory search tool | Memory store |
| Plan | plan-mode 规则 | 当前计划/进度，带 synthetic provenance | plan read/update tools | Plan store |
| Skill | skill 目录（name + description + 绝对路径） | 显式选择生成的 contextual User-role Text，或模型 `read` 的 Tool Result | **无新增工具**（复用 `read`） | 文件系统目录；已接受 Turn 的历史快照随 contextual Message 持久化 |
| MCP | — | — | 远端工具定义 | MCP client |

**注意它们对模型的接口大多是工具** —— 工具路径天然产生 `tool_call` Span；用户显式选择 Skill 则直接进入 Model Request 正文。两条路径都不需要新的 Trace kind（见 [trace.md](trace.md)）。

Skill 一行分两层，**两层的归宿相反**：**目录**（每个 skill 只有 name、description 和绝对路径，一行 bullet）常驻 System Context，**正文**进入 Conversation。正文有两条入口：用户从 `$` 候选框选中时，Core 在接受 Turn 前把 `UserInput::Skill { name, path }` 解析为带标记的 contextual User-role Text 快照；模型自主决定使用时，仍作为 `read` 的 Tool Result 进入。理由见 [skills.md §4](skills.md)：正文放 System Context 就永远回收不掉（压缩只替换 Conversation），目录放 Conversation 则会在压缩时被边界甩掉、要补就得给压缩投影开特例；放 System Context 则每个 Turn 重新物化，压缩前后都在——**零额外机制**。

显式选择没有制造第四条物化链：Tauri Command 把 `UserInput::Text` 与 `UserInput::Skill { name, path }` 一次性交给 Core；Core 解析并持久化正文快照后，Chat State 只接收已经物化的 User-role Text Message。`ModelRequestBuilder` 不读取文件、不查询 Skill 状态，也不做 Skill 特殊投影；provider adapter 永远只看到已有 ContentBlock。

Skill 也是这张表里唯一**不新增工具**的一项：目录里带着绝对路径，模型用已有的 `read` 打开它，与读 `references/` 是同一个机制。**新增能力时先问它能不能落在已有工具上，再考虑加工具。**

不为未来可能性预建字段、枚举、Registry 或数据库表。只有出现**第二个**真正需要独立更新和恢复的动态来源时，才抽出通用的 Source 生命周期接口。

## 7. 验收

**System Context**
- Agent System Prompt 在 Project Instructions 之前；
- 根部 `AGENTS.md` 存在时被加载，缺失或空白时不产生空 System Message；
- 符号链接与超过 64 KiB 的文件被拒绝；
- 同一 key 重复时确定性失败。

**Conversation**
- 未压缩时返回完整已提交 Conversation，压缩后返回摘要加后续消息；
- 不包含 Draft，不包含 System Message；
- Assistant Message 与 Tool Result 写入后进入下一次 Model Call；
- 显式选择的 Skill 作为带 `name + path + body` 的历史快照进入 Conversation；磁盘变化不改写已接受 Turn；
- `ModelRequestBuilder` 直接组装 Core 已物化的 contextual Text Message，不识别 Skill；provider adapter 不读取 Skill 来源；
- Chat State 不接收 Model、System Prompt 或 Tool Definitions。

**Tool Surface**
- Definitions 与 Dispatch 来自同一 `FinalizedToolset`；
- 权限过滤后不可见的工具既不广告也不可执行；
- 输入未变化时工具顺序确定。

**组装**
- 三者按固定边界进入 `ModelRequest`；
- Conversation 中出现 System Message 时失败；
- `ModelRequest` 保持 provider-neutral；
- Budget Estimate 与实际准备的**同一份**请求对应；
- Context Window Inspection 与真实请求使用同一 projection。

**跨压缩**
- 压缩后 System Context 与 Tool Surface 从各自来源重新物化；
- 压缩前后未变化的 System 与 Tool 前缀保持字节一致；
- synthetic item 不会被识别成最后一个真实用户请求，且 rewind 后不重复注入。
