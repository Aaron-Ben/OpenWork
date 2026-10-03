# 上下文窗口

本页描述一次 Model Call 的输入由什么组成、从哪里来、怎样组装与估算。实现在 `openwork-core` 的 `context/`：`builder.rs` 解析 System Context，`world_state/` 产生 World State 消息，`engine.rs` 组装请求。Conversation 由 `openwork-chat-state` 持有，工具面由 `openwork-tools` 物化。

Agent Loop 什么时候组装请求，见 [session-runtime.md §4](session-runtime.md)。压缩怎样替换 Conversation，见 [compaction.md](compaction.md)。

## 1. 请求的组成

一次 Model Call 的输入来自三个区域。三个区域各自物化，只在 `ContextEngine::prepare` 汇合：

| 区域 | 权威来源 | 什么时候变 | 物化结果 |
|---|---|---|---|
| System Context | Agent 定义 + 工具面的提示规则 | 换 Agent | `ResolvedSystemContext` |
| Conversation | Chat State（含 World State 消息） | 每次追加、每次压缩 | `ConversationContextView` |
| Tool Surface | Agent 工具名 + 注册表 + Core 控制工具 | Session 载入时 | `TurnToolset` |

“物化”指：从权威来源读取当前状态，按确定的规则生成一份只读结果。物化结果不是新的业务事实，也不反过来拥有来源。

```text
ResolvedSystemContext ─┐
ConversationContextView ┼→ ContextEngine::prepare → PreparedModelCall { ModelRequest, ContextBudgetEstimate, … }
TurnToolset.definitions ┘
```

会随工作区变化的上下文（项目布局、`AGENTS.md`、Skill 目录、沙箱策略）不在 System Context 里。它们作为 World State 消息进入 Conversation（§3）。

理由见 [Agent Note：三个区域分别物化](../../.agents/notes/implemented/architecture/2026-07-27-three-materialized-input-regions.md)。

## 2. System Context

`SystemContextBuilder::build`（`context/builder.rs`）只产生一个 part：

| key | 内容 |
|---|---|
| `core/agent-system` | Agent system prompt，加上工具面需要的规则（`TurnToolset::system_prompt`：`update_plan` 规则与子 Agent 规则，只在对应工具被广告时出现） |

- Agent Loop 在每个 Turn 开始时解析一次，同一 Turn 内的所有 Model Call 复用它。
- `TurnToolset` 在 Session 载入时构造一次，所以这段内容在 Actor 的生命周期内逐字节不变。
- System Context 不读 Conversation，不选工具，不构造请求。
- 组装时校验（§6）：key 非空、不重复，content 非空，每个 part 估算低于 10 000 token。

手动压缩没有 Turn。它的摘要请求用 Agent system prompt 现场构造 System Context，不带工具面规则（[compaction.md §3](compaction.md)）。

## 3. World State

World State 是会话中会变化的上下文。每次 Model Call 之前，runner 采样一次。变化的 section 以一条 `Role::User`、`MessageKind::WorldState` 的消息追加到 Conversation 末尾。没有变化的 section 不发任何字节。

| section ID | 正文 | 来源 |
|---|---|---|
| `runtime/user-project-context` | `<user_project_context format_version="1">`：工作目录、仓库根、顶层布局 | `context/user_project.rs` |
| `project/AGENTS.md` | `<project_instructions>` 包裹的 `AGENTS.md` 原文 | `context/project_instructions.rs` |
| `skills/catalog` | `<available_skills>` Skill 目录 | `context/skill_catalog.rs`，内容见 [skills.md](skills.md) |
| `runtime/sandbox-policy` | `<sandbox_policy>` 四行 | 会话模式与沙箱可用性，见 [permissions.md §11](permissions.md) |

来源的规则：

- 项目上下文只列仓库根（找不到 `.git` 时为工作目录）的顶层条目，按名称排序，最多 64 条，多出的写成一行省略计数。正文超过 16 384 字符时，采样失败。
- `AGENTS.md` 只读工作目录根下的这一个文件。文件不存在或内容为空白时，section 缺失。符号链接、非普通文件、超过 64 KiB、非 UTF-8 时，采样失败。
- Skill 目录不包含已停用的 Skill。扫描告警写进日志，不进模型上下文。
- 采样失败时，Turn 以对应的错误码失败（`user_project_context_error`、`project_instruction_error`、`skill_catalog_task_error`），不调用模型。

采样流程（`TurnRunner::sample_world_state`）：

```text
capture（读四个来源）
  → render_diff（与内存基线比较，只读）
  → 逐条写库（message_id = world-state:<turn>:<model call>:<采样序号>:<section>，重复写入幂等）
  → 追加进 Chat State
  → 基线推进
```

**任何一条写库失败时，基线不推进，下一次采样重新产生同样的消息。** 一次采样有几个 section 变化，就写几条消息，不合并。四个 section 的顺序固定：项目上下文、`AGENTS.md`、Skill 目录、沙箱策略。

每个 section 的正文是全量重渲染。是否附加声明，由内存基线与“这个 section 的消息还在不在当前 Conversation 里”共同决定：

| 内存基线 | 消息还在 | 有正文时发送 | section 缺失时发送 |
|---|---|---|---|
| 有，且正文相同 | 是 | 不发 | — |
| 有，正文不同 | 是 | 取代声明 + 正文 | 失效声明 |
| 有 | 否（被压缩换走） | 正文，不带声明 | 不发 |
| 无（进程重启） | 是 | 取代声明 + 正文 | 失效声明 |
| 无 | 否 | 正文，不带声明 | 不发 |

- “消息还在”由 `RetainedSections::scan` 判断。它只看 `MessageKind::WorldState` 的消息，再按正文开头的标记或固定声明识别 section。用户输入了同样的标记，也不算数。
- 取代声明举例：`以下 AGENTS.md 指令取代先前提供的全部 AGENTS.md 指令。` 失效声明举例：`先前提供的 AGENTS.md 指令不再适用。`
- 基线只在内存里。进程重启后基线为空，所以仍在 Conversation 里的 section 会带取代声明重发一次。
- Turn 内的自动压缩之后，runner 立即重新采样（采样序号 2），所以压缩后的那次提交仍带 World State。

理由见 [Agent Note：World State 以追加消息进入 Conversation](../../.agents/notes/implemented/architecture/2026-08-15-world-state-as-appended-messages.md)。各 section 的理由见 [Agent Note：Skill 目录](../../.agents/notes/implemented/architecture/2026-08-14-skill-catalog-world-state-section.md) 与 [Agent Note：sandbox-policy section](../../.agents/notes/implemented/architecture/2026-09-24-sandbox-policy-world-state-section.md)。

## 4. Conversation

### 4.1 Chat State

`ChatStateActor`（`openwork-chat-state`）是 Conversation 的唯一写者。它持有一列 `ConversationItem`、一份流式草稿，以及修剪水位线（[compaction.md §2](compaction.md)）。

每个 item 有来源与类型：

| 来源 | 含义 |
|---|---|
| `Real { message_id, sequence }` | `messages` 表的一行。运行中刚追加的 item 还没有 ID 与序号 |
| `Synthetic { compaction_id, reason, … }` | 压缩投影产生的 item：`LastUserRequestReplay`、`CompactionSummary`、`SystemReminder` |

| `MessageKind` | 含义 |
|---|---|
| `normal` | 用户可见的消息，以及所有 Assistant 与 Tool 消息 |
| `skill_instruction` | 显式选择的 Skill 正文快照，写在用户消息之前 |
| `agent_message` | 子 Agent 发回的消息 |
| `world_state` | 一个 World State section 的一次全量渲染 |

后三种都是 `Role::User`，但都不是用户请求。需要“用户请求”的代码必须检查 `MessageKind`，不能只看 role。

Chat State 的约束：

- 不接受 System Message。载入时遇到 System Message，返回 `PersistedSystemMessage`。
- Tool Result 必须对应一个尚未有结果的 Tool Call，否则返回 `UnmatchedToolResult`。同一个 Tool Call ID 不能重复。
- `context_view()` 返回当前 item 与水位线，不含草稿。每次 Model Call 重新读取，不缓存。
- Chat State 不加载 `AGENTS.md`、Skill 或计划的来源，也不接收模型、system prompt 或工具定义。

未压缩时，Conversation 是 Session 的全部已提交消息。压缩之后，它是“最后一条真实用户请求、摘要、运行提醒，加上边界之后的消息”（[compaction.md §5](compaction.md)）。

### 4.2 请求副本

`ContextEngine::prepare` 不改 Chat State，也不改数据库。它在请求副本上依次做三步：

| 步骤 | 实现 | 作用 |
|---|---|---|
| 修剪 | `context/prune.rs` | 水位线以下超过 8 192 字符的旧 Tool Result 只留头尾（[compaction.md §2](compaction.md)） |
| 单项投影 | `context/projection.rs` | 单条 Tool Result 估算超过 8 000 token 时截断：去掉 data block，文本保留前 75% 与后 25% 的预算，中间换成截断标记 |
| 合法化 | `context/normalize.rs` | 让 Tool Call 与 Tool Result 成对，过滤模型不接受的 data block |

合法化的规则：

- Tool Result 必须紧跟声明它的 Assistant Message，并按 Tool Call 的顺序排列。错位、重复或找不到 Tool Call 的结果从副本中移除。
- 缺少结果的 Tool Call 补一条 `interrupted` 结果，正文固定为 `Tool result unavailable: the previous turn ended before a durable result was recorded.`。同一段历史每次补出的字节相同。
- 模型不接受 data block 时：真实用户输入里有 data block，请求失败；Skill 正文、Agent Message 与 Tool Result 里的 data block 被移除。

## 5. Tool Surface

```text
Agent 工具名 + 工具注册表 + ToolSessionContext
  → FinalizedToolset { definitions, dispatch }        （openwork-tools）
  → TurnToolset：再加 update_plan 与子 Agent 控制工具   （openwork-core，session/toolset.rs）
```

- **Definitions 与 Dispatch 来自同一个 `FinalizedToolset`。** 广告的每个名称都能解析，没有广告的名称返回 `ToolNotFound`。
- 根 Session 广告 `update_plan` 与子 Agent 控制工具。子 Agent Session 不广告它们，也不能调用它们。
- 控制工具与注册表工具重名时，`TurnToolset::new` 失败，Session 不能载入。
- 默认 Agent 另外注册 `conversation_history`（[compaction.md §8.4](compaction.md)）。
- 沙箱不可用时，工具 schema 不带越界参数（[permissions.md §9.2](permissions.md)）。会话模式不改变工具面。
- `TurnToolset` 在 Session 载入时构造，之后每个 Turn 复用。

## 6. 组装

`ContextEngine::prepare`（`context/engine.rs`）只做确定性工作：

1. 对 Conversation 做 §4.2 的三步。
2. 校验 System Context：key 非空、不重复，content 非空，每个 part 不超过单项上限。
3. 校验 Conversation 中没有 System Message，否则返回 `SystemMessageInConversation`。
4. 测量预算（§7）。
5. 生成 `ModelRequest`：System part 在前，Conversation 在后；工具定义放在 `tools` 字段；`max_output_tokens` 取模型能力；`temperature`、`top_p`、`thinking` 不设置。

它不读项目文件，不改 Chat State，不选择或执行 Tool Call，不调用 Provider。`ModelRequest` 与 Provider 无关。Provider Adapter 在组装之后把它编码成各家的协议，不加载任何上下文来源。

`PreparedModelCall` 同时给出请求、预算估算、投影摘要，以及每条 Conversation 消息的来源（已写库的消息 ID，或“合成”）。

## 7. 预算与上限

### 7.1 估算

`ContextBudgetEstimate::measure` 分别测量三个区域：

```rust
struct ContextBudgetEstimate {
    system_context_tokens,
    conversation_tokens,
    tool_surface_tokens,
    estimated_input_tokens,   // 三者之和
    reserved_output_tokens,   // 模型的 max_output_tokens
}
```

- 口径是 JSON 序列化字节数除以 4，向上取整。
- Tool Result 的 artifact 只给界面用，不发给模型，不计入估算。
- 估算测量的是 §4.2 处理之后、实际提交的那份请求。
- 估算不改变请求：不截断，不拒绝，不重排。

用途只有两个：Desktop 的上下文用量展示；自动压缩的阈值判断。压缩前后的比较只测 Conversation 区域（`estimate_conversation_tokens`），口径相同。

### 7.2 模型上限

`ModelContextLimits::from_capabilities`（`context/limits.rs`）从模型能力推导：

| 值 | 计算或取值 |
|---|---|
| 可用输入 `effective_input_tokens` | 窗口 − `max_output_tokens` − `max_reasoning_tokens`（没有则为 0） |
| 自动压缩线 `auto_compact_token_limit` | ⌈可用输入 × 85%⌉ |
| 触线条件 | 输入估算 + 输出预留 ≥ min(自动压缩线, 可用输入) |
| 摘要输出上限 | 16 384 token |

例：窗口 200 000、输出 32 000 时，可用输入 168 000，自动压缩线 142 800。

### 7.3 单项上限

作者控制的单项超过上限时，系统拒绝它，不裁剪（`context/item_limits.rs`）：

| 单项 | 上限 | 超过时 |
|---|---|---|
| System Context part | 10 000 token | 请求组装失败 |
| Skill 正文 | 8 000 token | 拒绝开始 Turn |
| Agent Message | 4 000 token | 投递失败 |

Tool Result 不在这张表里。它由 §4.2 的单项投影截断。

## 8. 上下文检查

`OpenWorkCore::inspect_context_window` 返回一份只读预览：System Context 各 part、Conversation、Tool Surface、预算与自动压缩百分比。

- 它从当前来源物化三个区域，再经同一个 `ContextEngine::prepare` 组装。Conversation 部分就是请求副本里 System 之后的消息。
- 它读的是已写库的 Conversation 与当前水位线，不做 World State 采样。下一次 Model Call 才会追加的 section 不在预览里。
- 它不调用 Provider，也不保存请求副本。运行中的 Turn 继续使用它在开始时解析的 System Context。

## 9. 各能力的位置

| 能力 | System Context | Conversation | Tool Surface | 权威存储 |
|---|---|---|---|---|
| Skill | — | 目录是 World State；显式选择的正文是 `skill_instruction` 消息；模型自己读的正文是 `read` 的结果 | 不新增工具，复用 `read` | 文件系统；已接受 Turn 的正文快照随消息写库 |
| 计划 | `update_plan` 规则 | Tool Call 与结果；压缩后进入运行提醒 | `update_plan` | `turn_plans` |
| 子 Agent | 子 Agent 规则 | `agent_message` 消息 | 子 Agent 控制工具 | Session 表与 Mailbox |
| 沙箱策略 | — | World State | 越界参数随沙箱可用性出现 | 会话模式 |

显式选择的 Skill 正文由 Core 在接受 Turn 之前解析并写库（[skills.md §3](skills.md)）。Chat State 只接收已经物化的 User-role Text。`ContextEngine` 与 Provider Adapter 不识别 Skill。

## 10. 不变量

1. System Context 只有 `core/agent-system` 一段，不写入 Conversation。
2. Conversation 不含 System Message。
3. System Context 在 Conversation 之前。
4. Tool Definitions 与 Dispatch 来自同一个 `FinalizedToolset`。
5. Provider Adapter 不加载任何上下文来源。
6. 预算估算只测量，不改变请求。
7. 压缩只替换 Conversation。摘要输入不含 World State 消息，摘要不是 System Context 或 Tool Surface 的副本。
8. 系统生成的 User-role 消息都带 `MessageKind` 或合成来源，可以与真实用户输入区分。
9. 来源与顺序不变时，同一段历史的请求副本逐字节相同。

## 11. 验收

原文的条目没有编号。下面按原文分组与顺序编号。原文写的 `ModelRequestBuilder` 在代码中是 `ContextEngine::prepare`；`AGENTS.md` 已从 System Context 移到 World State。条目按代码事实改写。测试路径相对 `crates/`；带 Postgres 的测试需要 `TEST_DATABASE_URL`。

**System Context**

1. System Context 只有 Agent system prompt 一段；`AGENTS.md` 作为 World State 消息出现在 Conversation 里。
   - 测试：`openwork-core/src/context/builder.rs::the_system_prefix_is_only_the_agent_system_prompt`；`openwork-core/tests/session_runtime.rs::world_state_reaches_the_model_as_user_messages_not_system_parts`
2. 根部 `AGENTS.md` 存在时加载它；缺失或空白时，section 缺失，不发空消息。
   - 测试：`openwork-core/src/context/project_instructions.rs::missing_file_produces_no_context`；`openwork-core/src/context/project_instructions.rs::whitespace_only_file_produces_no_context`；`openwork-core/src/context/project_instructions.rs::loads_a_stable_project_instruction_block`；`openwork-core/src/context/world_state/mod.rs::a_fresh_session_stays_silent_about_sections_that_never_existed`
   - 缺口：没有用空白 `AGENTS.md` 跑完整 Turn 的测试。
3. 拒绝符号链接与超过 64 KiB 的文件。
   - 测试：`openwork-core/src/context/project_instructions.rs::rejects_symlinked_instruction_files`；`openwork-core/src/context/project_instructions.rs::rejects_oversized_and_non_utf8_files`；`openwork-core/tests/session_runtime.rs::invalid_project_instructions_fail_before_model_and_leave_no_draft`
4. 同一 key 重复时确定性失败。
   - 测试：`openwork-core/src/context/engine.rs::rejects_duplicate_system_context_keys`；`openwork-core/src/context/engine.rs::rejects_malformed_system_context_and_system_messages_in_conversation`

**Conversation**

5. 未压缩时是完整的已提交 Conversation；压缩后是摘要投影加后续消息。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_uses_the_full_conversation_and_replaces_only_the_active_projection`；`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`
6. 不含草稿，不含 System Message。
   - 测试：`openwork-chat-state/src/actor.rs::serializes_conversation_writes_and_returns_a_context_view`；`openwork-core/src/context/engine.rs::rejects_malformed_system_context_and_system_messages_in_conversation`
   - 缺口：Chat State 载入时拒绝 System Message 没有测试。
7. Assistant Message 与 Tool Result 写入后，进入下一次 Model Call。
   - 测试：`openwork-core/tests/session_runtime.rs::tool_result_is_in_the_next_model_request`；`openwork-core/tests/session_runtime.rs::multiple_tool_results_keep_provider_order_in_the_next_request`
8. 显式选择的 Skill 作为带 name、path、body 的快照进入 Conversation，并写在用户消息之前；磁盘变化不改写已接受的 Turn。
   - 测试：`openwork-core/src/skills/tests.rs::selected_skills_resolve_exact_paths_and_deduplicate_body_snapshots`；`openwork-core/src/session/input.rs::turn_input_labels_skill_context_separately_from_the_user_request`；`openwork-core/tests/postgres_session_storage.rs::postgres_persists_contextual_input_before_the_visible_user_message`
   - 缺口：没有在 Turn 接受后修改磁盘 Skill 的测试。
9. `ContextEngine` 直接组装已物化的 contextual Text，不识别 Skill；Provider Adapter 不读 Skill 来源。
   - 状态：无测试。由结构保证：`openwork-models` 不引用 Skill 或 `MessageKind`。
10. Chat State 不接收模型、system prompt 或工具定义。
    - 状态：无测试。由结构保证：`ChatStateHandle` 的接口没有这类参数。

**Tool Surface**

11. Definitions 与 Dispatch 来自同一个 `FinalizedToolset`。
    - 测试：`openwork-tools/src/registry.rs::finalized_toolset_is_the_model_and_dispatch_subset`；`openwork-core/src/session/toolset.rs::every_advertised_name_resolves`
12. 工具面不广告的工具，也不能执行。
    - 测试：`openwork-core/src/session/toolset.rs::a_disabled_control_tool_is_neither_advertised_nor_dispatched`；`openwork-core/tests/session_runtime.rs::unknown_tool_becomes_a_result_and_the_model_continues`
13. 输入不变时，工具顺序确定。
    - 测试：`openwork-core/tests/postgres_core_host_flow.rs::production_host_persists_and_starts_an_idle_explorer_session`
    - 缺口：没有用同一输入构造两次并比较顺序的测试。

**组装**

14. 三个区域按固定边界进入 `ModelRequest`。
    - 测试：`openwork-core/src/context/engine.rs::assembles_the_system_prefix_ahead_of_the_conversation`；`openwork-core/src/context/engine.rs::preserves_the_system_materialization_order`
15. Conversation 中出现 System Message 时失败。
    - 测试：`openwork-core/src/context/engine.rs::rejects_malformed_system_context_and_system_messages_in_conversation`
16. `ModelRequest` 与 Provider 无关。
    - 测试：`openwork-core/src/context/engine.rs::assembles_the_system_prefix_ahead_of_the_conversation`
    - 缺口：只断言采样参数未设置，与 Provider 无关由类型保证。
17. 预算估算对应实际准备的同一份请求。
    - 测试：`openwork-core/src/context/budget.rs::measures_the_three_input_regions_without_mutating_them`；`openwork-core/src/context/budget.rs::tool_result_artifacts_do_not_count_toward_the_conversation`
    - 缺口：没有测试把估算与 `prepared.request` 的重新测量直接比较。
18. 上下文检查与真实请求使用同一个投影。
    - 测试：`openwork-core/src/context/engine.rs::the_conversation_view_is_the_one_actually_submitted`；`openwork-core/tests/postgres_core_host_flow.rs::bootstrapped_core_persists_a_provider_and_creates_a_session_from_its_model`
    - 缺口：没有端到端比较检查结果与真实请求；检查不做 World State 采样（§8）。

**跨压缩**

19. 压缩后，System Context、Tool Surface 与 World State 从各自的来源重新物化。
    - 测试：`openwork-core/tests/session_runtime.rs::context_overflow_compacts_and_resubmits_once_in_the_same_turn`；`openwork-core/tests/session_runtime.rs::a_request_rebuilt_after_compaction_still_carries_world_state`；`openwork-core/src/context/world_state/mod.rs::a_compacted_away_fragment_is_re_emitted_as_a_first_appearance`
20. 压缩前后，未变化的 System 前缀与工具定义逐字节相同。
    - 测试：`openwork-core/src/context/builder.rs::workspace_changes_do_not_alter_the_prefix`；`openwork-core/src/context/builder.rs::disabling_a_skill_does_not_alter_the_prefix`
    - 缺口：没有跨压缩比较前缀或工具定义的测试。
21. 合成 item 与 contextual 消息不会被当成最后一条真实用户请求；rewind 后也不重复注入。
    - 测试：`openwork-core/src/session/compaction/compacted_view.rs::a_skill_instruction_is_not_the_user_request_either`；`openwork-core/src/session/compaction/compacted_view.rs::an_agent_message_never_displaces_the_real_user_request`；`openwork-chat-state/src/item.rs::a_world_state_message_is_contextual_not_a_user_request`；`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`
    - 缺口：没有用 World State item 调用 `last_real_user` 的测试；rewind 不重复注入只由条数间接断言。

**World State**（原文没有这一组，按代码事实补充）

22. 首次采样按固定顺序发出全部存在的 section；没有变化时不发。
    - 测试：`openwork-core/src/context/world_state/mod.rs::the_first_sampling_emits_every_section_in_a_fixed_order`；`openwork-core/src/context/world_state/mod.rs::an_unchanged_world_emits_nothing`；`openwork-core/tests/session_runtime.rs::an_unchanged_world_adds_nothing_to_the_next_model_call`
23. 同一 Turn 内修改 `AGENTS.md`，下一次 Model Call 收到带取代声明的新正文。
    - 测试：`openwork-core/tests/session_runtime.rs::an_agents_md_edit_is_seen_by_the_next_model_call_in_the_same_turn`
24. 进程重启后，仍在 Conversation 里的 section 带取代声明重发；用户输入同样的标记不算数。
    - 测试：`openwork-core/src/context/world_state/mod.rs::a_restarted_session_re_emits_with_a_replacement_notice`；`openwork-core/src/context/world_state/mod.rs::a_normal_user_message_with_the_same_text_does_not_count`
    - 缺口：只有单元测试，没有 Actor 或 Postgres 层的重启测试。
25. 缺少结果的 Tool Call 在请求副本里得到固定正文的 `interrupted` 结果，字节稳定。
    - 测试：`openwork-core/src/context/normalize.rs::missing_tool_result_is_synthesized_in_call_order_with_the_original_call_id`；`openwork-core/src/context/normalize.rs::synthesizing_the_same_history_twice_produces_identical_bytes`
