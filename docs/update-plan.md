# Update Plan

`update_plan` 是一个**任务清单工具**。模型在执行复杂 Turn 时用它维护任务清单。每次调用把当前步骤、步骤状态和一条可选说明作为完整快照提交给 Core。Core 持久化成功后，再通知 Desktop。

它回答“这次任务现在做到哪一步”。它不回答“先与用户讨论出一份方案”，那属于未来的 Plan mode。两者不共享状态机。不要用同一个功能顺带实现两者。

## 1. 范围

V1 必须端到端完成以下能力：

- 模型能看到并调用 `update_plan`；
- Core 校验完整参数，并按 Turn 保存最新计划；
- 一次调用替换整个当前计划，不做局部 patch；
- 下一次 Model Call 能从 Conversation 看到刚刚成功的调用；
- Turn 内发生压缩后，模型仍能得到最新计划；
- Desktop 实时显示计划，并能从 Snapshot 或持久层恢复计划；
- 计划更新不触发文件、进程或网络权限审批；
- Tool Call、计划状态、Tool Result 和 UI Update 的顺序可验证。

V1 不包含：

- Plan mode、模式切换和只读探索阶段；
- 用户手工编辑、拖拽或创建计划；
- 跨 Turn 复用计划；
- 子步骤、依赖图、预计耗时、负责人、截止时间；
- 通用 `WorldState`、通用动态上下文 Registry；
- 独立的项目管理、Issue 同步或后台任务调度；
- 计划历史表和版本回滚 UI。

## 2. 领域模型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub step: String,
    pub status: PlanStepStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlanArgs {
    #[serde(default)]
    pub explanation: Option<String>,
    pub plan: Vec<PlanStep>,
}

pub struct TurnPlan {
    pub turn_id: TurnId,
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub updated_at: PrimitiveDateTime,
}
```

名称约束：

- 协议参数沿用 Codex 的 `plan`，表示一次提交的完整步骤列表；
- Core 内部实体叫 `TurnPlan`，用名称表明生命周期；
- 单项统一叫 `PlanStep`，不再同时引入 `Todo`、`TaskItem` 等同义词；
- `PlanUpdate` 表示已经提交成功的领域事件，不表示待写入的命令。

### 2.1 不变量

1. 一个 Turn 最多有一个当前 `TurnPlan`；
2. 步骤顺序就是模型的提交顺序，Core 不排序；
3. `step` 去掉首尾空白后不得为空，但存储原始的非空文本；
4. 任意时刻最多一个步骤是 `in_progress`；
5. 全部完成时允许零个 `in_progress`；
6. `plan: []` 表示显式清空当前计划，不等同于参数缺失；
7. 未提供 `explanation` 时，新快照保存 `NULL`，不能沿用旧说明；
8. 每次成功调用都是完整替换，不能与旧步骤按文本合并；
9. 参数含未知字段、未知状态或违反不变量时，必须整体失败；
10. 失败调用不能改变已有计划，也不能发送 `PlanUpdated`；
11. 新 Turn 从无计划开始，不继承上一个 Turn 的计划。

Core 必须在运行时校验“最多一个 `in_progress`”，不能只依赖 Tool description。JSON Schema 无法简洁表达这条跨数组元素的约束。模型提示也不是数据完整性的边界。

## 3. 从 Codex 参考什么

以下路径均相对于 `/Volumes/Extreme SSD/Code/codex/codex-rs`。

| 关注点 | Codex 源码 | 可采用的结论 |
|---|---|---|
| 参数类型 | `protocol/src/plan_tool.rs:6-29` | 三态步骤、可选 explanation、完整 plan 数组、拒绝未知字段 |
| Tool Schema | `core/src/tools/handlers/plan_spec.rs:7-58` | 工具名为 `update_plan`，并向模型声明最多一个 `in_progress` |
| Core Handler | `core/src/tools/handlers/plan.rs:62-103` | Core 处理控制工具，成功结果是简短的 `Plan updated` |
| Plan mode 边界 | `core/src/tools/handlers/plan.rs:84-87` | checklist 与 Plan mode 是两个概念；Codex 在 Plan mode 拒绝该工具 |
| 内部事件 | `core/src/tools/handlers/plan.rs:90-93` | 解析成功后发送类型化的 `EventMsg::PlanUpdate` |
| Rollout 策略 | `rollout/src/policy.rs:150-180` | `PlanUpdate` 通知本身是瞬时事件，不作为 rollout 的持久事实 |
| App Server DTO | `app-server-protocol/src/protocol/v2/turn.rs:423-447` | 对外通知同时携带 thread、turn、explanation 和完整步骤列表 |
| App Server 转换 | `app-server/src/bespoke_event_handling.rs:1205-1259` | 在边界把 Core 事件转换成 Turn 级通知 |
| 模型行为规则 | `protocol/src/prompts/base_instructions/default.md:52-121,267-275` | 复杂任务才建计划；推进时更新；结束前把所有步骤置为 completed |
| TUI 投影 | `tui/src/chatwidget/turn_runtime.rs:498-512`、`tui/src/history_cell/plans.rs:169-247` | UI 消费完整计划快照，可直接计算 completed/total |

Codex 最值得采用的是三条边界，不是某个类型名：

1. `update_plan` 是 Core 控制工具，不是工作区副作用工具；
2. checklist 与 Plan mode 完全分离；
3. 事件向 UI 发送完整快照，前端不需要重放 patch。

### 3.1 不直接照抄的部分

Codex 把 `EventMsg::PlanUpdate` 归为瞬时事件。OpenWork 不能照此只做一次广播。原因是 Desktop 已经有同进程 Snapshot、页面重载和持久化会话展示。如果最新计划只存在于事件里，漏掉一次广播或刷新页面后，就无法重建计划。

因此 OpenWork V1 采用：

- Conversation 保存模型确实发起过的 Tool Call 和收到的 Tool Result；
- Plan Store 保存调用成功后的最新 `TurnPlan`；
- `SessionUpdate::PlanUpdated` 只是实时投影。

这不是把同一份内容无边界地复制三次。三者职责不同：Tool Call 是协议审计事实；`turn_plans` 是当前业务状态；Session Update 是可以丢失的进程内通知。

## 4. OpenWork 当前接缝

引用一律用符号名，不用行号。行号会随无关改动漂移。读者无法判断是文档过期了，还是代码变了。

当前代码有 Agent Loop、持久化消息、工具注册表和 Desktop Update，但没有 Turn Plan：

- `session/run_loop.rs` 的 `TurnRunner::run` 先持久化 Assistant Message，再逐个执行 `run_tool_call`；
- `session/run_loop.rs` 的 `run_tool_call` 负责校验、权限、执行、Tool Result 持久化和 Update 顺序；`append_cancelled_tool_result` 负责同批剩余调用的收尾；
- `openwork-tools/src/context.rs` 的 `ToolCallContext` 只有执行能力，没有 Turn ID、Plan Store 或 Session Update sink；
- `openwork-tools/src/registry.rs` 的 `FinalizedToolset` 同时提供 `definitions()` 与 `validate` / `authorize` / `call`；
- `core.rs` 在 Session 级构造并复用普通工具集；
- `session/storage.rs` 的 `SessionStorage` 没有计划读写接口；
- `migrations/202607260001_initial_schema.sql` 没有计划表；
- `session/updates.rs` 的 `SessionRuntimeSnapshot` 和 `SessionUpdate` 没有计划；
- `openwork-agent/src/prompt.rs` 的 `DEFAULT_SYSTEM_PROMPT` 没有计划使用规则；
- `desktop/src/features/chat/runtimeReducer.ts` 与 `desktop/src/features/chat/transcript.ts` 没有计划投影。

### 4.1 已经存在、必须复用的接缝

`session/compaction/state.rs` **已经**提供压缩期动态状态的扩展机制。不要另建一条平行路径。现有机制如下：

- `CompactionStateContributor` trait：`key()` / `schema_version()` / `failure_policy()` / `collect()` / `render()`；
- `CompactionStateCollector` 负责 key 去重、失败策略与顺序稳定；
- `CompactionRuntimeState.extensions: BTreeMap<String, CompactionStateEntry>` 按 key 持久化各来源的状态，每项带独立的 `schema_version`；
- `ReminderSection` + `render_system_reminder` 负责统一渲染与 `MAX_REMINDER_CHARS` 校验；
- 现成实现 `FileChangeStateContributor`（key `file_changes`）是可照抄的样板。

§8 的计划投影放在这个机制里，不新写一条 projection 管线。

不能把 `UpdatePlanTool` 作为普通 `Tool` 直接放进 `openwork-tools`，原因如下：

- 它修改 SessionActor 拥有的 Turn 状态，不修改工作区；
- 普通 `ToolCallContext` 不知道当前 Turn；
- `ToolRisk` 只有只读、工作区变更和进程执行，没有“内部控制状态变更”；
- 为它扩充所有普通工具的 Context，会制造只服务一个工具的 Data Clump；
- 把它伪装成 `ReadOnly`，会让权限语义失真。

## 5. 目标架构

```text
Agent policy
    + 普通 FinalizedToolset
    + Core update_plan definition/handler
              │
              ▼
        TurnToolset
  { definitions, resolve() }
              │
       Model Tool Call
              │
              ▼
        Session run loop
              │
      resolve exactly once
       ┌──────┴────────┐
       ▼               ▼
ResolvedTurnTool     ResolvedTurnTool
  ::UpdatePlan         ::Registered
(openwork-core)      (openwork-tools)
       │               │
       │          validate/authorize/call
       │          + PermissionEngine
       ▼
Plan Store + Tool Result transaction
       │
       ├──→ Chat State
       ├──→ Session runtime snapshot
       ├──→ SessionUpdate::PlanUpdated
       └──→ PlanStateContributor
              └──→ Compaction runtime reminder
```

### 5.1 模块边界

新增一个具体的 Core 模块，例如：

```text
crates/openwork-core/src/plan/
├── mod.rs          TurnPlan、PlanStep、校验
├── tool.rs         update_plan schema、参数解析、成功输出
└── projection.rs   Session DTO 投影 + PlanStateContributor
```

`projection.rs` 里的 `PlanStateContributor` 实现 `session/compaction/state.rs` 已有的 `CompactionStateContributor` trait（见 §8.1）。collector 负责 reminder 的渲染与长度校验。这里只负责把 `TurnPlan` 变成 `ReminderSection`。

这是一个 **Module**。它隐藏校验、存储形状和投影细节，对外只暴露窄 **Interface**。调用方不要自己解析状态字符串、检查 `in_progress` 个数或渲染计划。

`openwork-tools` 继续拥有文件、进程等普通能力工具；`openwork-core` 拥有 `update_plan`。不要反转依赖，也不要让工具 crate 依赖 SessionActor。

### 5.2 TurnToolset

Core 在现有的普通 `FinalizedToolset` 之上，物化一个供 Turn 执行使用的工具面：

```rust
pub struct TurnToolset {
    definitions: Vec<ModelToolDefinition>,
    tools: Arc<FinalizedToolset>,
}

/// 一次调用只解析一次，结果贯穿校验、权限、执行三个阶段。
pub enum ResolvedTurnTool<'a> {
    UpdatePlan,
    Registered(&'a ToolDefinition),
}
```

**注意现有 Runner 不是"解析一次然后 dispatch"的形状。** `FinalizedToolset` 暴露三个独立入口。`run_tool_call` 依次调用 `validate` → `authorize` → `call`，每次都用字符串名再查一遍表。`TurnToolset` 只包一个 `dispatch` map 是不够的。它必须让这三个阶段都消费同一个 `ResolvedTurnTool`：

```text
resolve(name) -> ResolvedTurnTool
   ├─ UpdatePlan   → 跳过 authorize，直接进提交路径
   └─ Registered   → validate / authorize / call 照旧
```

约束：

- Default mode 下，Core 固定把 `update_plan` 加入 `definitions`，不把它写进 Agent 的普通工具白名单；
- 必须用同一个 `TurnToolset` 构造 `definitions` 与 `resolve`，二者的一致性是唯一的测试边界；
- `update_plan` 与已注册工具重名时，构造 `TurnToolset` 就确定性失败，不要等到调用时才发现；
- Runner 只在 `run_tool_call` 开头解析一次，后续按 `ResolvedTurnTool` 分支，不再比较字符串；
- 不在参数校验、权限判断、执行结果和 UI 层重复比较字符串 `"update_plan"`；
- V1 只有一个 Core 控制工具，不先抽象 `ControlTool` trait 或插件 Registry；
- 将来出现第二个真实的控制工具时，再判断是否需要通用注册接口。

这个 **Seam** 的深度来自两点：调用方只需要“解析并执行一个 Turn 工具”；模块内部同时隐藏普通工具的权限路径与控制工具的提交路径。

### 5.3 权限

`update_plan` 不进入 `PermissionEngine`，因为它不访问主机能力，也不修改工作区。用 `ResolvedTurnTool::UpdatePlan` 这个类型化分支表达“绕过权限”，不要给它伪造 `ToolRisk::ReadOnly`。

这不表示所有 Core 工具都自动免审批。每新增一个控制工具，都必须重新判断它的效果。V1 只对 `update_plan` 给出结论。

### 5.4 Trace

每个 Tool Call 都通过 `start_tool_trace` 开一个 Span，并在 Span 上记录权限口径。`update_plan` 绕过 `PermissionEngine`，**不等于**它不留 Trace。如果不留 Trace，`turns.tool_call_count` 与 `trace_spans` 的计数会对不上。schema 注释明确说这两者互为参照物。

因此控制工具分支必须：

- 照常 `start_tool_trace` 并 `set_resolved_tool_name("update_plan")`；
- 记录 `record_permission_decision("allow", "control_tool")`，用一个**新的、可区分的** source，不复用 `builtin`；
  - 理由与 `run_tool_call` 里“`Mode` / `Builtin` 不可合并”的注释一致：事故复盘要能回答"这条为什么没问我"。"它是 Core 控制工具"和"它被内置规则放行"是两个不同的答案；
  - `append_cancelled_tool_result` 已有 `record_permission_decision("cancelled", "system")` 的先例。这说明这一列本来就记录非 PermissionEngine 的结论。
- 不调用 `record_permission_rule`（没有规则参与）；
- 照常用 `record_permission_mode` 记录当前模式，以便与同一 Turn 内的其他工具对齐。

## 6. 持久化

计划属于 Turn，不属于 Session。目标表只保存最新快照：

```sql
CREATE TABLE turn_plans (
    turn_id       TEXT PRIMARY KEY
                  REFERENCES turns(id) ON DELETE CASCADE,
    explanation   TEXT,
    steps         JSONB NOT NULL,
    updated_at    TIMESTAMP WITHOUT TIME ZONE NOT NULL
                  DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),

    CONSTRAINT turn_plans_steps_is_array
        CHECK (jsonb_typeof(steps) = 'array')
);
```

Rust 侧的时间使用项目统一的 `PrimitiveDateTime` 和 `china_now()`。IPC 序列化结果带 `+08:00`。应用层校验每个 JSON 元素和跨元素不变量。数据库只兜底检查顶层类型和外键生命周期。

### 6.1 计划必须有界

```rust
pub const MAX_PLAN_STEPS: usize = 128;
pub const MAX_STEP_CHARS: usize = 1_024;
```

这两个上限不是多余的防御，是压缩链路的硬要求。计划要投影进 reminder，而 reminder 有 `MAX_REMINDER_CHARS`（32 KB）的硬上限。**不设界的计划可以让压缩整体失败，进而拖垮整个 Turn**。这种失败比"拒绝一份超长计划"糟糕得多。而且失败点离原因很远，排查时很难想到是计划太长。

超限时**整体拒绝**，不截断。截断会让模型以为提交成功，而实际存下的是另一份计划。这两个上限的量级与 `FileChangeStateContributor` 的 `MAX_EDITED_PATHS` 一致。

不新增以下内容：

- `plan_steps` 子表：V1 只整体替换和整体读取，拆表会增加写入与排序的复杂度，没有查询收益；
- `plan_revisions`：Conversation 已记录每次模型调用，V1 没有回滚需求；
- `session_id` 冗余列：`turn_id` 已唯一定位所属 Session；
- `revision` 乐观锁：一个 Runner 串行执行同一 Turn 的工具调用。

`SessionStorage` 增加窄接口：

```rust
async fn load_turn_plan(&self, turn_id: &TurnId) -> Result<Option<TurnPlan>, String>;

async fn load_session_turn_plans(&self, session_id: &SessionId)
    -> Result<Vec<TurnPlan>, String>;

async fn commit_plan_update(
    &self,
    turn_id: &TurnId,
    plan: &TurnPlan,
    success_tool_result: &Message,
) -> Result<(), String>;
```

这些接口在三点上与现有 `SessionStorage` 对齐：

- 提交接口以 `turn_id` 为键，与既有的 `append_tool_result(&self, turn_id, message)` 一致。本节已说明"`turn_id` 已唯一定位所属 Session"，所以接口不该再多要一个 `session_id`；
- 错误类型沿用 trait 里现有的 `Result<_, String>`，不为这一个功能引入第二套错误约定；
- `load_session_turn_plans` 是 §9 加载历史 Turn 计划的来源。它按 Session 一次取全，避免前端按 Turn 做 N+1 查询。

`commit_plan_update` 必须在同一个数据库事务中完成以下步骤：

1. `INSERT ... ON CONFLICT (turn_id) DO UPDATE` 当前计划；
2. 追加本次成功 Tool Result Message；
3. 提交。

这个专用接口防止出现“计划已经改变，但模型历史里没有成功 Tool Result”，也防止出现相反的状态。不要让 Plan handler 先调用两个互相独立的 Repository 方法，再自己补偿。

## 7. 一次调用的顺序

```text
1. Assistant Tool Call Message 已持久化并进入 Chat State
2. start_tool_trace 开 Span（与普通工具同一路径）
3. TurnToolset 解析为 ResolvedTurnTool::UpdatePlan
4. 记录 trace 权限口径 allow / control_tool
5. 解析 JSON + 校验领域不变量
6. 在一个事务中 upsert TurnPlan + 追加成功 Tool Result
7. 把成功 Tool Result 追加到 Chat State
8. 更新运行中 current_plan
9. 广播 SessionUpdate::PlanUpdated（完整快照）
10. 广播通用 ToolCallFinished 并结束 Span
11. 下一次 Model Call 读取最新 Conversation
```

成功输出固定为简短的 `Plan updated`。计划本体已经在 Assistant Tool Call 的参数中，不在 Tool Result 里再复制一遍。

### 7.1 失败与并发顺序

- JSON/领域校验失败：不写 `turn_plans`，给模型生成普通的失败 Tool Result；
- 持久化事务失败：事务回滚，生成失败 Tool Result；如果失败结果本身也无法持久化，Turn 失败；
- 数据库已提交，但 Chat State 追加失败：Turn 失败，不发送 `PlanUpdated`；持久层仍是重建来源；
- broadcast 没有接收者：不回滚已提交的状态，Snapshot/持久层可以恢复；
- 用户取消与提交竞争：提交开始前观察到取消时，不执行；事务已提交后，不撤销计划；
- **同批工具调用中前序调用失败**：`run_tool_call` 失败后，同一 Assistant 响应里剩余的调用走 `append_cancelled_tool_result`。如果 `update_plan` 是其中之一，它**不执行、不写 `turn_plans`、不广播 `PlanUpdated`**，只产生 cancelled Tool Result。计划永远不能在已经终止的 Turn 上推进；
- **重复提交与 doom loop**：`is_doom_loop` 对**连续相同**的 `name` + canonical input 计数，达到阈值就终止 Turn。模型连续提交完全相同的计划也计入。V1 **不给 `update_plan` 开豁免**：连续多次提交完全相同的计划，本身就是无进展的信号，正是该保护要拦截的情况。任何一次其他工具调用都会重置计数，所以正常推进不会被误拦。

Assistant Tool Call 必须继续在任何计划副作用之前落库。这与普通 Tool Call 的审计顺序一致。

## 8. Conversation、压缩与 WorldState

正常情况下，不需要额外注入计划：

```text
Assistant(update_plan arguments)
Tool("Plan updated")
```

这两条已经是下一次 Model Call 可见的 Conversation。如果每次请求再注入一份相同的计划，会产生重复上下文，以及两个可能漂移的渲染版本。

### 8.1 压缩后的恢复：实现为一个 CompactionStateContributor

压缩**无条件**把模型看到的整个 Conversation 替换成三条。`compacted_items` 产出"最后一条真实用户消息重放 + 摘要 + reminder"。中间所有的 Assistant 消息、Tool Call 和 Tool Result 都离开投影，包括那次 `update_plan` 调用和它的 `Plan updated` 结果。

所以压缩之后，**reminder 是当前计划唯一的载体**。这不是"少了一层冗余"。reminder 出错时，模型会完全不知道当前计划，并回头重做已经完成的步骤。压缩时必须重新物化当前计划。

**这条路径已经存在，不要新建。** 按 §4.1，`session/compaction/state.rs` 的 `CompactionStateContributor` 正是为"权威状态 → reminder 投影"这类需求设计的。`FileChangeStateContributor` 是现成样板。把计划投影实现为第二个 contributor：

```rust
struct PlanStateContributor;   // key = "turn_plan", schema_version = 1
                               // failure_policy = RequiredWhenEnabled
```

它复用 collector 已有的能力：key 去重、失败策略、`extensions` 持久化（带独立的 `schema_version`）、`ReminderSection` 渲染和 `MAX_REMINDER_CHARS` 校验。渲染出的 section 形如：

```text
## Current plan
- [completed] 读取现有 schema
- [in_progress] 增加 turn_plans 迁移
- [pending] 接上 Desktop
```

状态标记直接用 wire 形式的状态名，**不要**发明 `[x]` / `[>]` / `[ ]` 这类符号。理由有两个：

1. reminder 用 XML 包裹，`escape_reminder_text` 会把 `>` 转成 `&gt;`，模型读到的是转义后的噪声；
2. schema、数据库、事件、UI 已经统一使用这套名字。再加一套符号，就多了第二份需要对齐的词汇表。

#### 需要改动的接口

`CompactionStateCollectInput` 目前只有 `messages: &'a [Message]`。contributor 拿不到计划，而且**不允许自行查库**。因此必须扩展这个结构体：

```rust
pub struct CompactionStateCollectInput<'a> {
    pub messages: &'a [Message],
    /// 当前 Turn 的权威计划。None 表示"本次压缩没有 Turn 上下文"。
    pub plan: Option<&'a TurnPlan>,
}
```

`collect_with_base` 相应增加参数。两个调用点都要传这个参数：

- `compaction/mod.rs` 的 `run_compaction`：通过 `ConversationCompactionRequest` 的新增字段带入，值来自 Runner 当前的 `TurnPlan`；
- `compaction/recovery.rs` 的 rewind 路径：**没有 Turn 上下文，传 `None`**。

#### `None` 与空计划必须区分

这是本节最容易写错的地方。写错时不会报错，只会让模型看到过期的计划。collector 的既有语义是：contributor 的 `collect` 返回 `None` 时，collector **保留并继续渲染** `extensions` 里的旧值（见 `carries_forward_unknown_extensions_and_rederives_file_state` 测试）。因此：

| 输入 | `collect` 返回 | 效果 |
|---|---|---|
| `Some(plan)` 非空 | `Some(json)` | 覆盖 extension，`render` 输出 section |
| `Some(plan)` 空（`plan: []`） | `Some(json!({"steps": []}))` | **覆盖成空**，`render` 返回 `Ok(None)`，不输出 section |
| `None`（rewind，无 Turn） | `None` | 保留上次结转的值并渲染 |

清空计划时，**绝不能**从 `collect` 返回 `None`。否则上一版计划会留在 extensions 里并继续注入，这正好违反"`plan: []` 清除旧 reminder 内容"。空计划要显式写成空值来覆盖旧值，让 `render` 决定不输出 section。

#### 其余规则

- Plan Store（或 Runner 当前状态）是计划的唯一来源，不能从摘要文本反向解析计划；
- contributor 只消费传入的已校验 `TurnPlan`，不查数据库，不解析 Tool Call JSON；
- reminder 包含步骤顺序和状态，不加入额外推断；
- 后续新的 `update_plan` Tool Call 位于 checkpoint 边界之后，自然覆盖旧 reminder 的语义；
- 再次压缩时，用最新计划重新物化（与 `file_changes` 每次重新推导同理）。

计划投影属于 Conversation，不属于 System Context。计划随当前 Turn 快速变化。把它放进 System Context 会污染稳定前缀，并让压缩无法正确控制它的生命周期。

### 8.2 复用现有 contributor 机制，不引入 WorldState

Codex 的 `core/src/context/world_state/` 维护一本增量交付账本，记录"模型已经见过什么"。`update_plan` 的协议和 handler 本身不依赖它。

OpenWork 已经有 `CompactionStateContributor` 这一层。它解决"压缩时把权威状态重新投影给模型"，**不**跟踪增量交付。计划的需求正好属于前者，所以：

- 复用 `CompactionStateContributor`，新增 `PlanStateContributor`，不新建平行管线；
- 不新增 Codex 式 `WorldState`，不引入增量交付账本；
- 不把 Plan 放进 `ModelRequestBuilder`；
- 不让 Chat State 查询或拥有 Plan Store。

以后出现确实需要"权威状态 → 模型投影 → **已交付状态**"三段式的来源时（即现有 contributor 无法表达增量），再评估 Codex 式 `WorldState`。到那时，它应是交付账本，不是 Plan 的业务真相。

## 9. Session Runtime 协议

Runtime Snapshot 增加：

沿用 `session/updates.rs` 里的既有类型名（枚举叫 `SessionUpdate`，没有 `SessionUpdateKind` 这个类型）：

```rust
pub enum SessionRuntimeSnapshot {
    Idle,
    Running {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        phase: SessionPhase,
        draft_text: String,
        draft_reasoning: String,
        tool_calls: Vec<LiveToolCall>,
        pending_permission: Option<Box<PermissionRequest>>,
        plan: Option<TurnPlanSnapshot>,          // 新增
    },
    Terminal {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        outcome: TurnOutcome,
        plan: Option<TurnPlanSnapshot>,          // 新增
    },
}

pub enum SessionUpdate {
    // existing variants...
    PlanUpdated {
        explanation: Option<String>,
        plan: Vec<PlanStep>,
        /// 带 `+08:00`，与历史计划的字段同形。
        updated_at: String,
    },
}
```

`updated_at` 不是可选的装饰，`TurnPlanSnapshot` 需要它。没有这个字段，Actor 就无法把事件无损地合并进 Snapshot，只能另找时间来源。那样，事件就不再是"完整快照"。这个字段还让前端对实时计划和历史计划使用同一套渲染。

`Idle` 不带计划：没有活动 Turn，就没有当前计划。历史计划从持久层取。

`SessionUpdateEnvelope` 已携带 `session_id`、`turn_id` 和单调的 `sequence`，payload 不重复这些字段。

`PlanUpdated` 是语义事件：

- 每次携带完整快照；
- 不与 text/reasoning delta 合批；
- 只在持久化成功后发送；
- 丢失不影响业务状态；
- reducer 按 envelope sequence 消费；
- 同进程重连时，从 Snapshot 恢复当前活动计划。

Running 和 Terminal 都携带对应 Turn 的计划。这样，Turn 刚结束时同进程重连，也不会丢失计划卡。历史 Turn DTO 从 `turn_plans` 读取最终计划。不能依赖 ring buffer 或重新解析 Tool Call JSON 来展示历史状态。

现有 `RuntimeLoadedSession` 在 `messages` 之外，增加按 Turn 返回的计划列表：

```ts
interface RuntimeLoadedSession {
  session: RuntimeSessionRecord
  messages: RuntimeStoredMessage[]
  plans: RuntimeTurnPlan[]
}

interface RuntimeTurnPlan {
  turnId: string
  explanation: string | null
  steps: RuntimePlanStep[]
  updatedAt: string
}
```

加载接口返回列表，不返回 `Record<string, ...>`。这样 Rust DTO 保持自然结构。前端在 transcript projection 边界一次性按 `turnId` 建索引。

## 10. Desktop

Desktop 在对应的 Assistant Turn 内显示一张计划卡：

- 保留服务端顺序；
- `pending`、`in_progress`、`completed` 有稳定的视觉状态和无障碍文本；
- 展示 `completed / total`；
- explanation 有值时显示，没有值时不留空占位；
- 空数组移除计划卡；
- 不允许前端自行把第一个 pending 推进成 in_progress；
- 不根据 Assistant 文本猜测进度；
- 不在刷新时从事件日志重放计划。

Reducer 只执行完整替换：

```ts
case "plan_updated":
  return replaceTurnPlan(state, envelope.turnId, update.plan)
```

计划 DTO 在 Tauri bridge 统一的 compat/schema 边界生成或校验。Rust 和 TypeScript 不各自维护一套拼写。状态沿用工具参数的 wire 形式 `pending | in_progress | completed`，不再增加第二套状态映射。

## 11. 模型提示

只提供工具定义，得不到稳定的行为。默认 Agent prompt 增加以下规则。

**注入条件**：这段规则是否出现，与 `update_plan` 是否出现在 `definitions` 中严格一致。广告了工具，就必须有规则；没广告工具，就不能有规则。当前 `DEFAULT_SYSTEM_PROMPT` 是一个五行常量。直接拼接会让 §14 第 1 步（尚未接入模型时不广告工具）出现"prompt 讲了一个不存在的工具"的中间态。因此，构造 `TurnToolset` 的同一处代码按是否加入 `update_plan` 决定规则文本，不把规则无条件写死在常量里。

规则内容：

- 简单、单步任务不使用计划；
- 复杂、多阶段或需要持续验证的任务，先建立计划；
- 开始执行某一步前，将其置为 `in_progress`；
- 完成后及时更新，不能等到最后一次性补记；
- 正常执行期间最多一个 `in_progress`；
- 任务真正结束前，将全部步骤置为 `completed`；
- 不为凑步骤而写“阅读代码”“回答用户”等无信息步骤；
- 调用成功后继续工作，不在聊天中重复粘贴完整计划。

Prompt 负责引导行为，Core 校验负责数据安全。不能因为写了 Prompt 就删除运行时不变量。

## 12. 与未来 Plan mode 的边界

Plan mode 是 Collaboration Mode：它改变允许的行为、工具面和最终输出契约。`update_plan` 只是 Default mode 下的执行清单。

未来实现 Plan mode 时：

- 用 collaboration mode 的权威状态选择 Tool Surface；
- Plan mode 不广告、也不分派 `update_plan`，不要只在 handler 里补一个兼容性拒绝；
- Plan mode 的最终方案不写入 `turn_plans`；
- 从 Plan mode 进入执行模式时，如果需要创建任务清单，在新的执行 Turn 中显式调用 `update_plan`；
- 两者不得共享一个名为 `plan` 的模糊全局状态。

## 13. 测试

### 13.1 领域与协议

- 三种状态可以稳定地序列化/反序列化；
- 缺少 `plan`、未知字段、未知状态时失败；
- 空白 step 失败，且不改变已有计划；
- 两个 `in_progress` 时失败；
- 全 completed 和空数组成功；
- 成功更新保持步骤原顺序，并完整替换旧计划；
- 超过 `MAX_PLAN_STEPS` 或 `MAX_STEP_CHARS` 时整体失败，不截断（§6.1）；
- 步骤文本保留原始首尾空白。Core 只判断它不是空白，不替模型改写表达。

### 13.2 Tool Surface

- definitions 中出现一次 `update_plan`，`resolve` 能解析同一名称；
- 与普通注册工具重名时，**构造 `TurnToolset` 即失败**，不是调用时才失败；
- 控制工具不进入 PermissionEngine；
- 普通工具仍走原权限路径；
- 未知工具仍生成失败 Tool Result；
- `update_plan` 仍产生 Trace Span，权限口径为 `allow` / `control_tool`，且不带 rule id。

### 13.3 Storage 与 Runner

- `commit_plan_update` 原子写入计划和成功 Tool Result；
- 任一步写入失败时，事务回滚；
- Assistant Tool Call 的 message sequence 早于 Tool Result；
- `PlanUpdated` 只在提交和 Chat State 追加之后出现；
- 连续调用只保留最新的 `turn_plans` 快照，Conversation 保留调用事实；
- 新 Turn 不继承旧计划；
- Session 删除后，计划级联删除；
- 同批调用中前序工具失败时，`update_plan` 只得到 cancelled Tool Result，`turn_plans` 没有写入，且没有 `PlanUpdated`；
- 连续提交同一份计划达到阈值时，仍按 doom loop 终止 Turn；
- §15.1 信号的三态各自成立：没建计划记 `NULL`，全部收尾记 `0`，有未收尾步骤记实际条数；
- 被取消的 Turn 照样记下未收尾条数，但按 `status` 过滤能把它排除在"忘了收尾"之外。

### 13.4 Context 与 compaction

- 未压缩时，下一次 Model Call 通过 Tool Call/Result 看到更新；
- 压缩移除原调用后，reminder 包含最新计划；
- **清空计划（`plan: []`）后再压缩，`extensions["turn_plan"]` 被覆盖为空且 reminder 不含旧步骤**。这条专门守护 §8.1 那张表的第二行，那是最容易退化成"结转旧值"的分支；
- 第二次更新后再次压缩，只渲染新快照；
- rewind 路径传 `plan: None` 时不 panic，且沿用结转值；
- `PlanStateContributor` 与 `FileChangeStateContributor` 共存时，key 不冲突，section 顺序稳定；
- `ModelRequestBuilder`、Chat State 和 Provider Adapter 都不查询 Plan Store；
- `PlanStateContributor::collect` 不接触 `SessionStorage`。

### 13.5 Desktop

- 完整快照替换旧卡片，不做 merge；
- 乱序或重复的 sequence 不让 UI 回退；
- 漏掉 live event 后，Snapshot 恢复当前计划；
- 页面重载后，历史 Turn 从持久层恢复最终计划；
- 空 plan 移除卡片；
- 三种状态具有文本或 ARIA 语义，不只靠颜色区分。

## 14. 实施顺序

按可工作的纵向切片推进，不先搭空框架：

1. **领域 + Storage**：增加类型、校验、迁移、原子提交接口和 PostgreSQL 测试；
2. **Core 闭环**：增加 `TurnToolset` 与 `ResolvedTurnTool`、工具定义/handler、Runner 顺序、Trace 口径和 Prompt 规则（与工具广告同步开启）；
3. **Live Desktop**：增加 `PlanUpdated`、Snapshot 字段、bridge DTO、reducer 与计划卡；
4. **恢复闭环**：历史 Turn 走 `load_session_turn_plans`；新增 `PlanStateContributor`，扩展 `CompactionStateCollectInput` 与 `collect_with_base`，并把当前 `TurnPlan` 从 Runner 穿到 `ConversationCompactionRequest`；rewind 路径传 `None`；
5. **整体验收**：跑领域、Storage、Session 集成、compaction、bridge 和前端测试，再做一次真实的多步骤 Turn。

第 4 步要改动两个已有调用点（`run_compaction` 与 rewind）和一个已有的公开结构体。它是全程唯一触碰现存压缩代码的一步。把它安排在 Desktop 之后，可以让前面三步的回归先稳定下来。

每一步完成后，产品都应保持可运行。第 1 步尚未接入模型，既不广告工具，也不注入 Prompt 规则。第 2 步接入模型后，就必须产生合法的 Tool Result。不能先广告一个尚未可执行的 `update_plan`。

## 15. 验收标准

功能完成必须同时满足：

- 一个复杂 Turn 中，模型能创建并多次推进计划；
- Core 拒绝非法状态，旧计划保持不变；
- 每个成功调用在数据库、Conversation、Snapshot 和 Desktop 上指向同一个完整快照；
- 刷新 Desktop 不丢最终计划；
- 同进程漏事件后，Snapshot 能恢复计划；
- 压缩后，模型仍知道当前步骤；
- 普通 Tool 的权限行为没有变化；`update_plan` 不弹审批，但仍留下 Trace Span；
- 简单任务不会因 Prompt 强制而产生无意义的计划；
- Plan mode 尚未实现，该功能也没有暗中预建它；
- 没有通用 WorldState、通用控制工具插件层或计划版本系统；
- 压缩投影走 `CompactionStateContributor`，没有第二条平行的 reminder 管线。

### 15.1 未收尾计划的可观测性

实践中最常见的失败不是数据不一致，而是**模型建了计划却没在 Turn 结束前把步骤置为 `completed`**。

Core 不应强制这一点。Turn 可能因错误、取消或达到上限而终止，这时留下未完成步骤是**正确**的记录。所以它是 §11 的 Prompt 规则，不是 §2.1 的不变量。

但"不强制"不等于"不可见"。如果规则无法证伪，写了也等于没写。那样就无法回答"模型到底遵不遵守"，只能逐条翻聊天记录。

信号落在 `turns` 上，**不落 Trace**。trace 的 payload 有保留期，过期后会被清理；而这个指标要跨月对比"改了提示词之后比例降了吗"。`turns` 上已有 `model_call_count` / `tool_call_count` 这类同性质的聚合列。

```sql
ALTER TABLE turns ADD COLUMN plan_unfinished_step_count INTEGER;
```

`finish_turn` 相应增加一个参数。Runner 从当前 `TurnPlan` 算出这个参数的值：

```rust
async fn finish_turn(
    &self,
    turn_id: &TurnId,
    outcome: &TurnOutcome,
    unfinished_plan_steps: Option<usize>,
) -> Result<(), String>;
```

**三态必须分开，不要给默认值把它们合并：**

| 值 | 含义 |
|---|---|
| `NULL` | 这个 Turn 没建计划。简单任务本来就不该建，属于正常 |
| `0` | 建了计划并且全部收尾，规则生效 |
| `> 0` | 建了计划但没收尾。只有 `status = 'completed'` 时才可疑 |

如果把"没建计划"和"建了且做对了"都记成 0，统计的分母里就会混进大量没用过这个功能的 Turn，算出的比例没有意义。

```sql
-- 正常完成的 Turn 里，有多少留下了没收尾的计划？
SELECT count(*) FILTER (WHERE plan_unfinished_step_count > 0) AS forgot_to_finish,
       count(*)                                               AS completed_with_plan
FROM turns
WHERE status = 'completed' AND plan_unfinished_step_count IS NOT NULL;
```

`status = 'completed'` 这个过滤是关键。出错和取消的 Turn 留下未完成步骤是正确记录，不该算作违规。计数照常记录（那是事实，不是指控），查询再按状态区分。

**已知边界**：`interrupted` 状态的 Turn 走 `mark_running_interrupted()`，直接改 `turns.status`，不经过 `finish_turn`，因此不记录这个信号。这时进程已经退出，记不到是合理的。而且"死时的欠账"与正常收尾的语义不同，混进同一列反而会污染统计。

## 16. 实施状态

本文描述的 V1 范围**已经实现**，代码位置如下：

| 领域 | 位置 |
|---|---|
| 类型与校验 | `openwork-core/src/plan/mod.rs` |
| 工具定义、参数解析、Prompt 规则 | `openwork-core/src/plan/tool.rs` |
| Session DTO 与压缩 contributor | `openwork-core/src/plan/projection.rs` |
| 表结构 | `migrations/202608070001_create_turn_plans.sql` |
| §15.1 观测信号 | `migrations/202608070002_add_turn_plan_completion_signal.sql` |
| 读写与原子提交 | `storage/postgres/plan.rs` |
| 工具面 | `session/toolset.rs`（`TurnToolset` / `ResolvedTurnTool`） |
| Handler 与顺序 | `session/run_loop.rs` 的 `run_update_plan` |
| 计划卡 | `desktop/src/features/chat/components/PlanCard.tsx` |

压缩一侧按 §8.1 复用了既有机制，改动如下：新增 `PlanStateContributor`；`CompactionStateCollectInput` 增加 `plan` 字段；`collect_with_base` 增加对应参数；Runner 给 `run_compaction` 带入当前 `TurnPlan`；rewind 与手动压缩传 `None`。**没有另建平行的投影管线。**

### 16.1 真跑实证

2026-08-07 的一次真实多步骤 Turn（`turn-7ed55dea…`）：

| 指标 | 值 |
|---|---|
| 状态 | `completed` |
| 时长 | 10 分 2 秒 |
| 模型调用 | 19 |
| 工具调用 | 24 |
| `update_plan` 调用 | **5** |
| `plan_unfinished_step_count` | **0** |

5 次调用证明，模型不只是建计划，而是**边做边推进**。未收尾为 0，说明 §11 里"结束前把每一步标成 completed"这条 Prompt 规则确实生效。这正是 §15.1 那个信号存在的意义，这次它第一次证明了自己有用。全程 `update_plan` 没有触发任何权限审批。

尚未取得实证的有两条：**简单任务不误建计划**（§15 第 8 条），以及**真实压缩后模型仍知道当前步骤**（§15 第 6 条，该轮未触发压缩阈值；有自动化测试覆盖）。

---

后续改动以本文的目标边界为准。不引入以下路径：只广播、不持久化；把计划伪装成普通只读工具；绕开 `CompactionStateContributor` 另建投影管线。
