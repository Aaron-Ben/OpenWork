# Session、Message 与 Event Journal

Last reviewed: 2026-07-15

> Status: current implementation snapshot. 字段、事件目录和设计决策见 [Event Journal 与会话持久化重构设计](../plans/event-journal-persistence-refactor.md)。

## 1. 当前数据库有五张表

显式 migration 完成后，业务库结构为：

| 表 | 作用 |
| --- | --- |
| `schema_migrations` | migration 版本记录 |
| `providers` | Provider 配置与加密 API Key |
| `provider_models` | Provider 模型和 `lite/plus/pro` 用户分类 |
| `recorded_events` | Session、Turn、Step、ToolRun、Approval 和 Message 的 append-only 事实日志 |
| `trace_spans` | 可丢弃的运行诊断 Span；记录耗时、重试和归一化错误，不是恢复事实来源 |

以下四张遗留表已由 forward migration 删除：

```text
sessions
messages
llm_events
tool_runs
```

旧 `sessions/messages/llm_events/tool_runs` 数据不做 backfill。早期 `recorded_events` 中使用 Thread 命名的事实会由 forward migration 原位改名；`openwork-session` 和只为它生成通用 CRUD 的 `openwork-db-macros` crate 已经删除。

## 2. Session、Turn 与事件命名

前端、Tauri、Application、Persistence 和 Journal 统一使用 Session/Turn。Session 表示一个持续对话，可包含多个 Turn：

| 领域操作 | Journal aggregate/event |
| --- | --- |
| 创建 Session | Session aggregate 的 `session_created` |
| 重命名 Session | `session_title_changed` |
| 删除 Session | `session_deleted`，不删除历史事实 |
| 开始聊天 | Turn aggregate 的 `turn_started` |
| 用户消息 | `user_message_recorded` |
| 开始模型步骤 | `step_started` |
| Assistant 消息 | `assistant_message_recorded` |
| 请求工具 | `tool_run_requested` |
| 请求/解决审批 | `approval_requested` / `approval_resolved` |
| 开始执行工具 | `tool_run_started` |
| 工具执行终态 | `tool_run_completed/failed/denied/cancelled/outcome_unknown` |
| Tool 消息 | `tool_message_recorded` |
| Step 终态 | `step_completed` / `step_failed` |
| 正常结束 | `turn_completed` |
| 取消 | `turn_cancelled` |
| Doom loop | `turn_doom_loop_detected` |
| 失败 | `turn_failed` |

代码位置：

```text
crates/openwork-persistence/src/session/
  types.rs       Session DTO
  store.rs       Session/Turn Journal 入口
  lifecycle.rs   TurnRecorder 与生命周期重放投影
```

## 3. 当前读写流程

创建和维护 Session：

```text
Desktop session command
  -> OpenWorkApplication::sessions
  -> SessionApplicationService
  -> 内部 openwork-persistence::SessionStore
  -> EventJournal.append(Session event, ExpectedVersion)
  -> 读取时按 global_position 重放 Session events
```

一次聊天：

```text
OpenWorkApplication::turns
  -> TurnApplicationService
  -> 内部 ChatRuntime
  -> 从 recorded_events 回放历史 Message
  -> 在调用模型前持久化 turn_started + user_message_recorded
  -> Agent::run，通过 TurnRecorderPort 在每个 Step/ToolRun/Approval 语义点 await append
  -> UI delta 只实时发送给 Desktop
  -> Assistant/Tool Message 随对应 Step/ToolRun 立即持久化
  -> App 持久化 Turn 终态
  -> Desktop reload，重新从 Journal 回放
```

用户输入在任何模型或工具动作之前落库。工具副作用开始前必须先持久化 `tool_run_started`；工具返回后，ToolRun 终态与 Tool Message 在同一批 append 中提交。Turn 终态由 App 最后追加。

## 4. 当前投影方式

目前没有新增 `sessions` 或 `messages` 投影表。`SessionStore` 分页读取 `recorded_events`，在内存中重放：

- Session 列表和标题状态；
- 某个 Session 的 Message 顺序；
- 某个 Session 下的 Turn/Step/ToolRun 状态；
- 尚未解决的 pending approval；
- 删除状态和最近更新时间。

`SessionLoadResult` 同时返回 `messages` 和 `turns`；Desktop 用后者在重启后恢复审批卡片。Session/Message 仍不需要独立查询表。`trace_spans` 是独立的 best-effort 诊断投影，不改变 Journal 的事实来源地位。数据量增大后，如果全量重放成为性能瓶颈，再增加可删除、可重建的 `sessions/messages/tool_runs/approvals` 查询投影。

## 5. UI Stream 不再写数据库

`text_delta`、`reasoning_delta`、`tool_call_delta` 只用于当前实时渲染，不再通过 detached `tokio::spawn` 写入 `llm_events`。

这样消除了“看起来已经持久化，但实际可能乱序或丢失”的路径。数据库只接收需要恢复的 Recorded Fact。

## 6. Durable ToolRun/Approval 与恢复边界

当前 Core 会在真实语义点同步写入：

- `step_started`
- `assistant_message_recorded`
- `tool_run_requested`
- `approval_requested`
- `approval_resolved`
- `tool_run_started`
- ToolRun 终态与 `tool_message_recorded`
- `step_completed` / `step_failed`

`JournalTurnRecorder` 绑定一个既有 Turn，使用 `ExpectedVersion::Exact` 追加事实；并发版本冲突或持久化失败会返回给 Core，不会退化为 detached best-effort 写入。

副作用顺序固定为：

```text
Persist Intent -> Execute -> Persist Outcome
```

应用重启后可以恢复仍停在 `approval_requested` 的 Turn：App 从投影读取 pending approval，重建运行依赖，记录用户决定，并从当前 Step 继续。普通 `interrupted` Turn 暂不自动续跑；如果已有 `tool_run_started` 却没有终态，投影会标记 `outcome_unknown`，也不会自动重跑。后者需要外部副作用对账或人工处置，不能靠事件重放猜测工具是否执行成功。

## 7. Migration 与启动

从仓库根目录运行：

```bash
cargo run -p openwork-persistence --bin openwork-migrate
```

该命令创建/更新 Provider、Journal 和 Trace schema，并执行 `drop_legacy_session_tables`。然后启动：

如果数据库已经应用过早期使用 `thread` 命名的 Journal migration，migrator 会按 expand/data/contract 三步迁移：先允许 `session`，再把既有 aggregate、event type 和 payload key 改名，最后移除 `thread` 约束。事件 ID、聚合版本和全局位置不会改变。历史 migration 源码保持不可变。

```bash
cd apps/desktop
pnpm tauri dev
```

Desktop 启动只检查 `schema_migrations/providers/provider_models/recorded_events/trace_spans`，不会自动建表。

## 8. Crate 状态

| crate | 当前状态 |
| --- | --- |
| `openwork-persistence` | 拥有 Provider Repository、Event Journal、Session Repository 和 migration |
| `openwork-observability` | 拥有 best-effort Trace 缓冲、Span 归并和 Turn 生命周期装饰器 |
| `openwork-session` | 已删除，职责迁入 Persistence |
| `openwork-db-macros` | 已删除，不再需要通用 `PgEntity` |
| `openwork-database` | 已删除；连接池和 migration runner 已内聚到 Persistence |
