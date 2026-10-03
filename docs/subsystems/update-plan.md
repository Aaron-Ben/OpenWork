# Update Plan

本页描述 `update_plan` 任务清单工具：模型怎样提交计划，Core 怎样校验、保存与投影计划，Desktop 怎样显示计划。类型、校验与压缩投影在 `openwork-core` 的 `src/plan/`；工具面在 `src/session/toolset.rs`；执行顺序在 `src/session/run_loop/`；存储在 `src/storage/postgres/plan.rs`；计划卡在 `desktop/src/features/chat/`。

`update_plan` 回答“这次任务做到哪一步”。它与 Plan mode 无关。Plan mode 的边界见 [Agent Note：Plan mode 与 update_plan 分开](../../.agents/notes/proposed/feature/2026-08-07-plan-mode-separate-from-update-plan.md)。

## 1. 范围

- 只有根 Session 的模型能调用 `update_plan`。子 Agent 的工具面里没有它（[multi-agent.md](multi-agent.md)）。
- 每次调用提交完整计划，替换当前计划。没有局部修改。
- 计划属于 Turn。新 Turn 从没有计划开始。
- 计划更新不出审批卡片。

没有以下能力：用户手动编辑、拖动或创建计划；跨 Turn 复用计划；子步骤、依赖、预计耗时、负责人与截止时间；计划历史与回滚界面。

## 2. 数据模型

```rust
// crates/openwork-core/src/plan/mod.rs
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus { Pending, InProgress, Completed }

#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub step: String,
    pub status: PlanStepStatus,
}

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
    pub updated_at: PrimitiveDateTime,   // china_now()
}
```

- 状态的 wire 形式是 `pending`、`in_progress`、`completed`。工具 schema、数据库、事件、reminder 与 Desktop 都用这一套名称。
- 协议参数叫 `plan`，与 Codex 相同。Core 内部实体叫 `TurnPlan`，单项叫 `PlanStep`。

### 2.1 不变量

`validate_args` 在 Core 运行时检查这些不变量：

1. 一个 Turn 最多有一个当前 `TurnPlan`。
2. 步骤顺序就是提交顺序。Core 不排序。
3. `step` 去掉首尾空白后不能为空。Core 保存原文，不去掉空白。
4. 最多一个步骤是 `in_progress`。
5. 全部 `completed` 时，可以没有 `in_progress`。
6. `plan: []` 是显式清空。缺少 `plan` 时解析失败。
7. 没有 `explanation` 时保存 `NULL`，不沿用旧说明。
8. 每次成功调用完整替换旧计划，不按文本合并。
9. 未知字段、未知状态或违反不变量时，整个调用失败。
10. 失败的调用不改变已有计划，也不发 `PlanUpdated`。
11. 新 Turn 不继承上一个 Turn 的计划。压缩 reminder 是一个例外，见 §7.2。

第 4 条只在运行时检查。工具描述也写了这一条，但描述不是数据边界。

### 2.2 上限

| 常量 | 值 | 超出时 |
|---|---|---|
| `MAX_PLAN_STEPS` | 128 步 | 整个调用失败 |
| `MAX_STEP_CHARS` | 每步 1 024 个 Unicode 字符 | 整个调用失败 |

- 超出上限时不截断。
- `explanation` 没有上限。
- 这两个上限不保证压缩 reminder 不超长，见 §7.2。

上限与 reminder 的关系见 [Agent Note：压缩后经 reminder 重新给出计划](../../.agents/notes/implemented/architecture/2026-08-07-plan-in-compaction-reminder.md)。

## 3. 工具面

### 3.1 所有权

`update_plan` 由 `openwork-core` 拥有，不注册进 `openwork-tools`。`openwork-tools` 不依赖 SessionActor。

### 3.2 TurnToolset

```rust
// crates/openwork-core/src/session/toolset.rs
pub(super) enum ResolvedTurnTool {
    UpdatePlan,
    Agent(AgentTool),
    Registered(ToolId),
}

pub enum ControlToolSurface {
    Root { max_active_sub_agent_turns: usize },
    SubAgent,
}
```

- `TurnToolset::new` 在普通 `FinalizedToolset` 之上加控制工具。`Root` 加 `update_plan` 与五个子 Agent 工具；`SubAgent` 不加。
- 控制工具与已注册工具重名时，构造失败（`TurnToolsetError::NameCollision`），Session 不启动。
- `definitions()` 与 `resolve()` 出自同一个 `TurnToolset`。没有广告的名称不能分派。
- Runner 在 `run_tool_call` 开头调用一次 `resolve`，之后按 `ResolvedTurnTool` 分支，不再比较名称。未知名称得到 `ToolNotFound` 失败结果。
- 控制工具的参数不经注册表的 schema 校验，由 `plan` 模块解析。

理由见 [Agent Note：update_plan 是 Core 控制工具](../../.agents/notes/implemented/architecture/2026-08-07-update-plan-core-control-tool.md)。

### 3.3 工具定义

模型看到的描述（`update_plan_definition`）：

```text
Maintain a checklist for the task you are working on right now.

Use it for multi-step work that benefits from tracking; skip it for simple, single-step requests. Each call replaces the whole plan, so send the full list every time. Mark a step in_progress before starting it and completed as soon as it is done — at most one step may be in_progress at a time. Before finishing the task, mark every step completed.
```

参数 schema：

```json
{
  "type": "object",
  "properties": {
    "explanation": {
      "type": "string",
      "description": "Optional one-line note about why the plan changed."
    },
    "plan": {
      "type": "array",
      "description": "The complete checklist. Each call replaces the entire plan, so always send every step, not just the ones that changed.",
      "maxItems": 128,
      "items": {
        "type": "object",
        "properties": {
          "step": { "type": "string", "description": "What this step accomplishes." },
          "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
        },
        "required": ["step", "status"],
        "additionalProperties": false
      }
    }
  },
  "required": ["plan"],
  "additionalProperties": false
}
```

`step` 在 schema 里没有长度上限。

### 3.4 Prompt 规则

`TurnToolset::system_prompt` 在 Agent 系统提示之后追加 `UPDATE_PLAN_PROMPT_RULES`。追加的条件与广告 `update_plan` 的条件相同。运行 Turn 与上下文检查都经这个函数。原文：

```text
## Task checklists

You have an `update_plan` tool for tracking multi-step work.

- Skip it for simple, single-step tasks. An unnecessary checklist is noise.
- For complex, multi-stage work, or work that needs repeated verification, create a plan first.
- Mark a step `in_progress` before you start it, and `completed` as soon as it is done. Do not batch all the updates until the end.
- At most one step may be `in_progress` at a time during normal execution.
- Before you finish the task, mark every step `completed`.
- Do not pad the plan with contentless steps like "read the code" or "answer the user".
- After a successful call, just keep working. Do not paste the plan back into the chat.
```

Prompt 引导行为。运行时校验保证数据，不因有 Prompt 而省略。

## 4. 一次调用

1. Assistant Message 落库，并进入 Chat State。
2. Runner 开 Tool Span，发 `ToolCallStarted`。
3. `resolve` 得到 `UpdatePlan`。Span 记录 resolved 名称。
4. doom loop 计数（§4.2）。
5. Span 记录会话模式与 `allow` + `control_tool`。
6. 解析参数，检查 §2.1 与 §2.2。
7. `commit_plan_update` 在一个事务中写 `turn_plans`，并追加成功 Tool Result。
8. Span 记录结果。
9. Tool Result 进入 Chat State。
10. Runner 内存中的 `current_plan` 换成新计划。
11. 发 `PlanUpdated`，带完整快照。
12. 发 `ToolCallFinished`。

成功结果的文本固定为 `Plan updated`。计划本体只在 Tool Call 参数中。

### 4.1 失败

| 情况 | 行为 |
|---|---|
| 参数不是合法 JSON | 在 `resolve` 之前返回失败结果 `invalid tool input: …`。Turn 继续 |
| 解析失败（缺少 `plan`、未知字段、未知状态） | 失败结果，错误码 `InvalidArguments`，文本 `failed to parse update_plan arguments: <serde 错误>`。不写 `turn_plans`。Turn 继续 |
| 违反不变量或上限 | 失败结果，错误码 `InvalidArguments`，文本见下。不写 `turn_plans`。Turn 继续 |
| `commit_plan_update` 失败 | 事务回滚。Turn 以 `persistence_error` 失败。这次调用没有 Tool Result，也没有 `PlanUpdated` |
| 提交后 Chat State 追加失败 | Turn 失败。`turn_plans` 已是新计划；内存计划不变；不发 `PlanUpdated` |
| `PlanUpdated` 没有订阅者 | 已提交的状态不变 |

违反不变量时的文本（下标从 0 开始）：

```text
plan must not exceed 128 steps
plan step <index> must not be blank
plan step <index> exceeds 1024 characters
at most one step may be in_progress, but steps <first> and <second> are both in_progress
```

### 4.2 取消、同批调用与 doom loop

- `run_tool_call` 开始时检查取消。提交开始后，取消不撤销计划。
- 同一模型响应中，前一个 Tool Call 使 Turn 终止时，后面的调用都不执行，包括 `update_plan`。每个调用得到 cancelled 结果 `tool was not executed because the turn already terminated`，Trace 记 `cancelled` + `system`。不写 `turn_plans`，不发 `PlanUpdated`。
- doom loop 对 `update_plan` 同样生效。同名、同参数（规范化 JSON）的调用连续达到阈值时，结果为 `doom loop detected for tool 'update_plan'`，Turn 以 `doom_loop` 结束。默认 Agent 的阈值是 3。其他任何调用都会重置计数。

## 5. 权限与 Trace

- `update_plan` 不经执行前判定，不经沙箱，也不出卡片。这与模式无关。
- 它照常有 Tool Span，所以 Span 数与 `turns.tool_call_count` 一致。
- Span 属性：`permissionDecision = allow`，`permissionDecisionSource = control_tool`；记录 `sessionMode` 与 `sessionModeOrigin`；不记录 `sandboxMode`。
- 时间线把它归入“Core 控制工具”一类（[permissions.md §14.1](permissions.md)），并用计划图标（`desktop/src/features/traces/components/traceToolIcons.tsx`）。

理由见 [Agent Note：update_plan 是 Core 控制工具](../../.agents/notes/implemented/architecture/2026-08-07-update-plan-core-control-tool.md)。

## 6. 持久化

```sql
-- crates/openwork-core/migrations/202608070001_create_turn_plans.sql
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
CREATE INDEX turn_plans_turn_id_idx ON turn_plans (turn_id);
```

- 每个 Turn 一行，只存最新快照。清空的计划也是一行，`steps` 为 `[]`。
- 数据库只检查顶层数组与外键。元素形状与不变量由应用层检查。
- `updated_at` 由 Rust 侧的 `china_now()` 显式写入。同一个值进入 Snapshot 与事件，对外序列化带 `+08:00`。
- 删除 Session 时，Turn 与计划级联删除。

`SessionStorage` 中与计划相关的方法：

```rust
async fn load_turn_plan(&self, turn_id: &TurnId) -> Result<Option<TurnPlan>, String>;
async fn load_session_turn_plans(&self, session_id: &SessionId) -> Result<Vec<TurnPlan>, String>;
async fn commit_plan_update(
    &self,
    turn_id: &TurnId,
    plan: &TurnPlan,
    success_tool_result: &Message,
) -> Result<(), String>;
async fn finish_turn(
    &self,
    turn_id: &TurnId,
    outcome: &TurnOutcome,
    unfinished_plan_steps: Option<usize>,
) -> Result<(), String>;
```

- `commit_plan_update` 是写 `turn_plans` 的唯一路径。它先检查 Tool Result Message 恰好含一个 `ToolResult` 块，再在一个事务中锁定 Turn、执行 `INSERT … ON CONFLICT (turn_id) DO UPDATE`、追加 Tool Result Message、提交。
- `load_session_turn_plans` 按 `turns.sequence` 返回一个 Session 的全部计划，包括空计划。
- `load_turn_plan` 只有测试调用。
- `finish_turn` 见 §10。

理由见 [Agent Note：计划存为每个 Turn 的最新快照](../../.agents/notes/implemented/architecture/2026-08-07-persisted-turn-plan-snapshot.md)。

## 7. 模型看到什么

### 7.1 没有压缩时

下一次 Model Call 从 Conversation 看到这次调用：Assistant 的 `update_plan` Tool Call 参数，与 `Plan updated` 结果。Core 不另外注入计划。计划不在 System Context 中，也不是 world state 的 section。

### 7.2 压缩后

压缩把 Conversation 换成用户消息重放、摘要与 reminder（[compaction.md §5](compaction.md)）。原来的 Tool Call 与结果不再可见。`PlanStateContributor`（`src/plan/projection.rs`）把计划写进 reminder：

| 项 | 值 |
|---|---|
| key | `turn_plan` |
| `schema_version` | 1 |
| `failure_policy` | `RequiredWhenEnabled` |
| 输入 | `CompactionStateCollectInput.plan`，来自 Runner 的 `current_plan` |

reminder 中的 section：

```text
## Current plan
scoping the work
- [completed] read schema
- [in_progress] add migration
- [pending] wire runner
```

- `explanation` 不是空白时，写在步骤之前。
- reminder 统一转义文本：`&`、`<`、`>` 转为实体，换行转为 `\n`。
- contributor 不查库，不解析 Tool Call。
- collector 按 key 排序：`file_changes` 的 `## Edited paths` 在前，`## Current plan` 在后。

| 压缩时的 `plan` | `collect` 返回 | 结果 |
|---|---|---|
| `Some`，非空 | `{"explanation", "steps"}` | 覆盖 `extensions["turn_plan"]`，输出 section |
| `Some`，空 | `{"explanation", "steps": []}` | 覆盖为空，不输出 section |
| `None` | `None` | 保留 `extensions` 中上次的值，照旧输出 |

以下情况的 `plan` 是 `None`：

- Turn 内压缩，而这个 Turn 还没有成功的 `update_plan`；
- 手动压缩（只在没有活动 Turn 时允许）；
- rewind（[compaction.md §8.3](compaction.md)）。

因此，新 Turn 在调用 `update_plan` 之前压缩时，reminder 仍带着上次压缩记下的计划，这份计划可能属于更早的 Turn。

reminder 的上限是 `MAX_REMINDER_CHARS`（32 768 字符）。符合 §2.2 上限的计划仍可能超出它：128 步乘 1 024 字符是 131 072 字符，`explanation` 也不限长。超出时压缩失败，Turn 以 `compaction_error` 结束。

理由见 [Agent Note：压缩后经 reminder 重新给出计划](../../.agents/notes/implemented/architecture/2026-08-07-plan-in-compaction-reminder.md)。

## 8. Session Runtime 协议

```rust
pub enum SessionRuntimeSnapshot {
    Idle,
    Running { /* … */ plan: Option<TurnPlanSnapshot> },
    Terminal { turn_id, client_request_id, outcome, plan: Option<TurnPlanSnapshot> },
}

pub enum SessionUpdate {
    // …
    PlanUpdated {
        explanation: Option<String>,
        plan: Vec<PlanStep>,
        updated_at: String,          // 带 +08:00
    },
}

pub struct TurnPlanSnapshot { explanation, steps, updated_at: String }   // 不带 turn_id
pub struct TurnPlanRecord { turn_id: String, explanation, steps, updated_at: String }
pub struct LoadedSession { session, messages, plans: Vec<TurnPlanRecord> }
```

- `PlanUpdated` 的 wire 类型是 `plan_updated`。每次带完整快照，只在提交成功后发出。
- payload 不带 `turn_id`。envelope 带 `session_id`、`turn_id` 与单调递增的 `sequence`。
- 事件丢失不影响业务状态。
- Actor 把 `PlanUpdated` 完整替换进 `Running.plan`。新 Turn 的 `Running.plan` 从 `None` 开始。Turn 结束时，`Running.plan` 移到 `Terminal.plan`。
- `Idle` 不带计划。
- `LoadedSession.plans` 来自 `load_session_turn_plans`，是列表，包括空计划。
- `desktop/src/bridge/compat.ts` 手写同形的 TypeScript 类型：`RuntimePlanStep`、`RuntimeTurnPlanSnapshot`、`RuntimeTurnPlan`、`RuntimeLoadedSession.plans`。

## 9. Desktop

卡片的视觉规则见 [desktop.md §7.1](../desktop.md)。本节只写数据来源与投影规则。

- `runtimeReducer`：`plan_updated` 完整替换 `plan`；空数组置为 `null`；`turn_started` 清空 `plan`；重复或乱序的 sequence 不改变 `plan`；子 Agent Session 忽略 `plan_updated`。
- `runtimeViewFromSnapshot` 读取 Running 或 Terminal 的 `plan`。空计划视为没有计划。
- `sessionStore.plansBySession` 保存 `loadSession` 返回的 `plans`。Turn 结束时，Desktop 重新加载 Session，计划随之刷新。
- `buildTranscript` 只投影整个 Session 中最新的一份非空计划。候选是活动 Turn 以外的持久化计划，加上活动 Turn 的实时计划；取 `updatedAt` 最大的一份。
- 活动 Turn 清空计划后，更早 Turn 的计划重新成为候选。
- 卡片固定在该计划所属 Turn 的第一个 `update_plan` Tool Call 所在的消息上。没有这种消息时，卡片放在该 Turn 的第一条 Assistant 消息上。
- “已更新 N 次”数的是该 Turn 中 `update_plan` Tool Call 的不同 id。失败的调用也计入。
- 消息流隐藏 `update_plan` 的 Tool Call 与 Tool Result。只含这些块的消息被隐藏，它的 id 并入锚点消息的 `sourceMessageIds`。
- `ToolActivityList` 不把 `update_plan` 显示为工具活动。
- 只读 transcript（子 Agent 详情）隐藏 `update_plan` 块，不显示计划卡。
- `PlanCard` 保持服务端顺序，显示 `completed / total`。每个状态有文字标签。`explanation` 为空时不留空位。空计划不渲染。组件不自行推进步骤。

## 10. 未收尾计划的观测

```sql
-- crates/openwork-core/migrations/202608070002_add_turn_plan_completion_signal.sql
ALTER TABLE turns ADD COLUMN plan_unfinished_step_count INTEGER;
ALTER TABLE turns ADD CONSTRAINT turns_plan_unfinished_non_negative
    CHECK (plan_unfinished_step_count IS NULL OR plan_unfinished_step_count >= 0);
```

| 值 | 含义 |
|---|---|
| `NULL` | 这个 Turn 没有成功的 `update_plan`，或 Turn 状态是 `interrupted` |
| `0` | 最后的计划全部 `completed`，或计划被清空 |
| `> 0` | 最后的计划中 `pending` 与 `in_progress` 的步数 |

- Runner 在 Turn 结束时用 `current_plan` 计算这个值，经 `finish_turn` 写入。完成、失败与取消的 Turn 都写入。
- `interrupted` 的 Turn 由 `mark_running_interrupted` 改状态，不经 `finish_turn`。
- 没有界面显示这个值。

查询正常完成的 Turn 中有多少没有收尾：

```sql
SELECT count(*) FILTER (WHERE plan_unfinished_step_count > 0) AS forgot_to_finish,
       count(*)                                               AS completed_with_plan
FROM turns
WHERE status = 'completed' AND plan_unfinished_step_count IS NOT NULL;
```

理由见 [Agent Note：用 turns 上的计数观测未收尾的计划](../../.agents/notes/implemented/architecture/2026-08-07-plan-unfinished-step-signal.md)。

## 11. 验收

第 1–11 条沿用原设计文档验收标准的顺序。第 12 条起来自原文档测试清单与观测一节中前 11 条没有覆盖的要求。测试路径相对 `crates/`，前端测试写出文件与用例名。

带 Postgres 的测试需要 `TEST_DATABASE_URL`，没有设置时静默跳过。

1. 一个复杂 Turn 中，模型能创建计划，并多次推进。
   - 测试：`openwork-core/tests/session_runtime.rs::update_plan_commits_then_broadcasts_a_complete_snapshot`；`openwork-core/tests/postgres_turn_plans.rs::successive_calls_keep_only_the_latest_snapshot`
   - 缺口：运行时测试只有一次调用，多次推进只在存储层断言。
   - 手动：2026-08-07 的一次真实多步骤 Turn（`turn-7ed55dea…`）状态 `completed`，调用 `update_plan` 5 次，`plan_unfinished_step_count = 0`，没有出卡片。
2. Core 拒绝非法计划，旧计划保持不变；失败结果写明违反了哪一条。
   - 测试：`openwork-core/src/plan/mod.rs::rejects_missing_plan_unknown_fields_and_unknown_status`；`openwork-core/src/plan/mod.rs::rejects_blank_steps`；`openwork-core/src/plan/mod.rs::rejects_two_in_progress_steps`；`openwork-core/src/plan/mod.rs::bounds_step_count_and_step_length`；`openwork-core/src/plan/tool.rs::parse_failures_explain_themselves_to_the_model`；`openwork-core/tests/session_runtime.rs::an_invalid_plan_fails_the_call_without_changing_stored_state`
   - 缺口：运行时测试在没有旧计划时断言存储为空。没有测试断言“已有计划时，非法调用不改变它”。
3. 每次成功调用在数据库、Conversation、Snapshot 与 Desktop 上是同一份完整快照。
   - 测试：`openwork-core/tests/session_runtime.rs::update_plan_commits_then_broadcasts_a_complete_snapshot`；`openwork-core/tests/postgres_turn_plans.rs::commit_writes_the_plan_and_the_success_tool_result_together`；`desktop/src/features/chat/runtimeReducer.test.ts › "stores the complete snapshot from a plan_updated event"`；`desktop/src/features/chat/runtimeReducer.test.ts › "replaces the whole plan instead of merging with the previous steps"`
   - 缺口：没有 Rust 测试断言 Actor 把 `PlanUpdated` 折进 Snapshot。
4. 刷新 Desktop 后，Session 中最新的非空计划从持久层恢复。
   - 测试：`openwork-core/tests/postgres_turn_plans.rs::a_new_turn_does_not_inherit_the_previous_plan`（`load_session_turn_plans` 返回上一个 Turn 的计划）；`desktop/src/features/chat/transcript.test.ts › "pins a historical plan to the first update_plan call and hides its JSON"`；`desktop/src/features/chat/transcript.test.ts › "shows only the latest plan across a conversation"`
   - 缺口：`OpenWorkCore::load_session` 返回 `plans` 没有测试。
5. 同进程漏掉事件后，Snapshot 恢复计划。
   - 测试：`desktop/src/features/chat/runtimeReducer.test.ts › "restores the active plan from a snapshot after a missed event"`；`desktop/src/features/chat/runtimeReducer.test.ts › "keeps the plan on a terminal snapshot so a reconnect does not lose the card"`；`desktop/src/features/chat/runtimeReducer.test.ts › "does not roll the plan back on a duplicate or out-of-order sequence"`
   - 缺口：Core 一侧填写 `Running.plan` 与 `Terminal.plan` 没有测试。
6. 压缩后，模型仍知道当前步骤。
   - 测试：`openwork-core/tests/session_runtime.rs::a_mid_turn_compaction_reprojects_the_current_plan_into_the_reminder`；`openwork-core/tests/session_runtime.rs::clearing_the_plan_removes_it_from_the_next_reminder`；`openwork-core/src/plan/projection.rs::renders_every_status_with_a_distinct_marker`；`openwork-core/src/plan/projection.rs::an_empty_plan_overwrites_rather_than_carrying_the_old_one_forward`；`openwork-core/src/plan/projection.rs::no_turn_context_carries_the_previous_value_forward`
   - 缺口：没有测试在两份非空计划之间连续压缩；rewind 只有 contributor 单元测试；没有测试接近 reminder 上限的计划。真实压缩下的模型行为没有手动记录。
7. 普通工具的权限行为不变；`update_plan` 不出卡片，但留下 Tool Span，来源是 `control_tool`。
   - 测试：`openwork-core/tests/session_runtime.rs::update_plan_never_asks_for_permission_even_in_default_mode`；`desktop/src/features/traces/permissionCategory.test.ts › "classifies %j as %s"`；`desktop/src/features/traces/components/TraceTimeline.test.tsx › "renders distinct plan and multi-agent control icons"`。普通工具的权限行为见 [permissions.md](permissions.md) 的验收。
   - 缺口：没有 Rust 测试断言 `update_plan` 的 Span 记 `allow` + `control_tool`。
8. 简单任务不会因 Prompt 规则产生无意义的计划。
   - 状态：手动，还没有记录。用 §10 的列验证：统计 `plan_unfinished_step_count IS NULL` 的 Turn 占比，并抽查简单请求的 Turn。
9. 代码中没有 Plan mode。`TurnToolset` 只按根 Session 与子 Agent 决定是否广告 `update_plan`。
   - 状态：手动：在 `crates` 与 `desktop/src` 检索 `PlanMode`、`plan_mode`，只有注释命中。
10. 计划不是 world state 的 section；没有通用的控制工具插件层，也没有计划版本表。
    - 状态：手动：`src/context/world_state/` 只有 `agents_md`、`project_context`、`sandbox_policy`、`skills_catalog` 四个 section；`ResolvedTurnTool` 是封闭枚举；迁移中与计划相关的只有 `turn_plans` 与 `plan_unfinished_step_count`。
11. 压缩投影走 `CompactionStateContributor`，没有第二条 reminder 管线。
    - 测试：`openwork-core/tests/session_runtime.rs::a_mid_turn_compaction_reprojects_the_current_plan_into_the_reminder`（断言 `extensions` 含 `turn_plan`）
    - 手动：`PlanStateContributor` 是 `CompactionStateCollector::default` 的成员；检索不到其他写 `Current plan` 的代码。
12. `update_plan` 在工具定义中恰好出现一次，并能分派；与已注册工具重名时，构造工具面即失败。
    - 测试：`openwork-core/src/session/toolset.rs::advertises_update_plan_exactly_once_and_resolves_it`；`openwork-core/src/session/toolset.rs::every_advertised_name_resolves`；`openwork-core/src/session/toolset.rs::a_registered_tool_with_the_same_name_fails_at_construction`；`openwork-core/src/plan/tool.rs::advertises_the_three_statuses_and_the_single_in_progress_rule`；`openwork-core/src/plan/tool.rs::schema_rejects_extra_fields_and_requires_plan`
13. 子 Agent 的工具面不广告、也不分派 `update_plan`。
    - 测试：`openwork-core/src/session/toolset.rs::a_disabled_control_tool_is_neither_advertised_nor_dispatched`
14. Prompt 规则只在广告 `update_plan` 时出现。
    - 状态：无测试。`TurnToolset::system_prompt` 按 `update_plan_enabled` 追加规则，但没有测试断言。
15. 三种状态稳定地序列化；空计划与全部 `completed` 的计划被接受；步骤顺序与首尾空白保留；缺少 `explanation` 时保存 `NULL`。
    - 测试：`openwork-core/src/plan/mod.rs::round_trips_every_status_through_the_wire_form`；`openwork-core/src/plan/mod.rs::accepts_all_completed_and_an_empty_plan`；`openwork-core/src/plan/mod.rs::preserves_the_submitted_order`；`openwork-core/src/plan/mod.rs::keeps_the_original_step_text_including_surrounding_whitespace`；`openwork-core/src/plan/mod.rs::accepts_an_absent_explanation_without_inheriting_an_old_one`；`openwork-core/tests/postgres_turn_plans.rs::successive_calls_keep_only_the_latest_snapshot`；`openwork-core/tests/postgres_turn_plans.rs::an_empty_plan_is_stored_as_an_explicit_clear`
16. `commit_plan_update` 原子写入计划与成功 Tool Result；Assistant Tool Call 的 sequence 早于 Tool Result；任一写入失败时整体回滚。
    - 测试：`openwork-core/tests/postgres_turn_plans.rs::commit_writes_the_plan_and_the_success_tool_result_together`；`openwork-core/tests/postgres_turn_plans.rs::a_rejected_tool_result_leaves_neither_the_plan_nor_a_message`；`openwork-core/src/storage/postgres/plan.rs::rejects_messages_that_are_not_a_single_tool_result`
    - 缺口：非法 Tool Result 在事务开始之前就被拒绝。没有测试在 upsert 之后让写入失败，事务回滚没有被直接验证。
17. `PlanUpdated` 只在提交与 Chat State 追加之后发出；提交失败时，Turn 失败，不发 `PlanUpdated`。
    - 测试：`openwork-core/tests/session_runtime.rs::update_plan_commits_then_broadcasts_a_complete_snapshot`；`openwork-core/tests/session_runtime.rs::a_failed_plan_commit_fails_the_turn_without_broadcasting`
    - 缺口：测试只断言存储收到了 `plan_commit`，没有比较它与事件的先后。
18. 新 Turn 不继承旧计划。
    - 测试：`openwork-core/tests/postgres_turn_plans.rs::a_new_turn_does_not_inherit_the_previous_plan`；`desktop/src/features/chat/runtimeReducer.test.ts › "does not carry a plan into the next turn"`
    - 缺口：Runner 的 `current_plan` 与 Actor 的 `Running.plan` 没有跨 Turn 测试。压缩 reminder 会结转上次的计划（§7.2），没有测试。
19. 删除 Session 时，计划级联删除。
    - 测试：`openwork-core/tests/postgres_turn_plans.rs::deleting_the_session_cascades_to_its_plans`
20. 同批调用中前一个调用终止 Turn 时，`update_plan` 只得到 cancelled 结果，不写 `turn_plans`，不发 `PlanUpdated`。
    - 状态：无测试。
21. 连续提交同一份计划达到阈值时，Turn 以 `doom_loop` 结束。
    - 测试：`openwork-core/tests/session_runtime.rs::three_identical_read_calls_still_end_the_turn_as_doom_loop`
    - 缺口：测试用的是 `read`。`update_plan` 走同一个 `is_doom_loop`，但没有专门的测试。
22. `plan_unfinished_step_count`：没有计划记 `NULL`，全部收尾记 `0`，未收尾记实际步数；取消的 Turn 照样记数，按 `status` 过滤可以排除它。
    - 测试：`openwork-core/tests/session_runtime.rs::a_turn_reports_its_unfinished_plan_steps_when_it_finishes`；`openwork-core/tests/session_runtime.rs::a_turn_that_finished_its_plan_reports_zero`；`openwork-core/tests/session_runtime.rs::a_turn_without_a_plan_reports_nothing_rather_than_zero`；`openwork-core/tests/postgres_turn_plans.rs::completion_signal::a_turn_without_a_plan_records_null_not_zero`；`openwork-core/tests/postgres_turn_plans.rs::completion_signal::a_fully_finished_plan_records_zero`；`openwork-core/tests/postgres_turn_plans.rs::completion_signal::an_unfinished_plan_on_a_completed_turn_is_the_case_worth_querying`；`openwork-core/tests/postgres_turn_plans.rs::completion_signal::a_cancelled_turn_keeps_its_unfinished_steps_without_being_flagged`
23. Desktop 完整替换快照，不合并；空计划移除卡片；每个状态有文字语义；卡片不自行推进步骤；消息流不出现 `update_plan` 的 JSON 与 `Plan updated`。
    - 测试：`desktop/src/features/chat/runtimeReducer.test.ts › "treats an empty plan as an explicit clear"`；`desktop/src/features/chat/transcript.test.ts › "drops the card when the active turn cleared its plan"`；`desktop/src/features/chat/transcript.test.ts › "ignores stored plans that have no steps"`；`desktop/src/features/chat/transcript.test.ts › "keeps ten update_plan calls as one card at the first call position"`；`desktop/src/features/chat/transcript.test.ts › "hides update_plan blocks instead of rendering them as tool rows"`；`desktop/src/features/chat/components/PlanCard.test.tsx › "shows completed over total"`；`desktop/src/features/chat/components/PlanCard.test.tsx › "keeps the server order"`；`desktop/src/features/chat/components/PlanCard.test.tsx › "gives every status a text label, not just a colour"`；`desktop/src/features/chat/components/PlanCard.test.tsx › "renders the explanation only when present"`；`desktop/src/features/chat/components/PlanCard.test.tsx › "renders nothing for an empty plan"`；`desktop/src/features/chat/components/PlanCard.test.tsx › "does not advance a pending step on its own"`；`desktop/src/features/chat/components/ToolActivityList.test.tsx › "does not render update_plan as a generic tool activity"`
24. ModelRequestBuilder、Chat State 与 Provider Adapter 都不读取计划存储；`PlanStateContributor::collect` 不访问 `SessionStorage`。
    - 状态：手动：`CompactionStateCollectInput` 只有 `messages` 与 `plan` 两个字段；在 `openwork-core/src/context/`、`openwork-chat-state`、`openwork-models` 中检索 `TurnPlan` 与 `turn_plans`，没有结果。
