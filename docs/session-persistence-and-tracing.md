# Session、Message 与 Event Journal

Last reviewed: 2026-07-11

> Status: current implementation snapshot. 字段、事件目录和设计决策见 [Event Journal 与会话持久化重构设计](../plans/event-journal-persistence-refactor.md)。

## 1. 当前数据库只有四张表

显式 migration 完成后，业务库结构为：

| 表 | 作用 |
| --- | --- |
| `schema_migrations` | migration 版本记录 |
| `providers` | Provider 配置与加密 API Key |
| `provider_models` | Provider 模型和 `lite/plus/pro` 用户分类 |
| `recorded_events` | Thread、Turn 和 Message 的 append-only 事实日志 |

以下四张遗留表已由 forward migration 删除：

```text
sessions
messages
llm_events
tool_runs
```

现有开发数据不做 backfill。`openwork-session` 和只为它生成通用 CRUD 的 `openwork-db-macros` crate 也已经删除。

## 2. Session API 为什么仍然存在

前端仍使用 `session_create`、`session_list`、`session_load` 等兼容命令，但数据库领域语义已经切换为 Thread/Turn：

| 前端概念 | Journal aggregate/event |
| --- | --- |
| 创建 Session | Thread aggregate 的 `thread_created` |
| 重命名 Session | `thread_title_changed` |
| 删除 Session | `thread_deleted`，不删除历史事实 |
| 开始聊天 | Turn aggregate 的 `turn_started` |
| 用户消息 | `user_message_recorded` |
| Assistant 消息 | `assistant_message_recorded` |
| Tool 消息 | `tool_message_recorded` |
| 正常结束 | `turn_completed` |
| 取消 | `turn_cancelled` |
| Doom loop | `turn_doom_loop_detected` |
| 失败 | `turn_failed` |

代码位置：

```text
crates/openwork-persistence/src/session/
  types.rs       Desktop 兼容 DTO
  store.rs       Journal 写入与当前内存投影
```

## 3. 当前读写流程

创建和维护 Session：

```text
Desktop session command
  -> openwork-persistence::SessionStore
  -> EventJournal.append(Thread event, ExpectedVersion)
  -> 读取时按 global_position 重放 Thread events
```

一次聊天：

```text
ChatRuntime
  -> 从 recorded_events 回放历史 Message
  -> 在调用模型前持久化 turn_started + user_message_recorded
  -> Agent::run
  -> UI delta 只实时发送给 Desktop
  -> 持久化本轮 Assistant/Tool Message + Turn 终态
  -> Desktop reload，重新从 Journal 回放
```

用户输入在任何模型或工具动作之前落库。成功、取消和 doom-loop 返回的完整 Message trace 会与 Turn 终态在同一批 append 中提交。

## 4. 当前投影方式

目前没有新增 `threads` 或 `messages` 投影表。`SessionStore` 分页读取 `recorded_events`，在内存中重放：

- Thread 列表和标题状态；
- 某个 Thread 的 Message 顺序；
- 删除状态和最近更新时间。

这保证当前数据库保持四张表，适合开发期数据量。数据量增大后，如果全量重放成为性能瓶颈，再增加可删除、可重建的 `threads/messages/action_runs/approvals` 查询投影；投影不是新的事实来源。

## 5. UI Stream 不再写数据库

`text_delta`、`reasoning_delta`、`tool_call_delta` 只用于当前实时渲染，不再通过 detached `tokio::spawn` 写入 `llm_events`。

这样消除了“看起来已经持久化，但实际可能乱序或丢失”的路径。数据库只接收需要恢复的 Recorded Fact。

## 6. 仍未完成的 Durable Action/Approval

当前已持久化 Thread、Turn、Message 和 Turn 终态，但以下语义还未直接写入 Journal：

- `action_requested`
- `approval_requested`
- `approval_resolved`
- `action_started`
- `action_completed` / `action_failed` / `action_outcome_unknown`

工具调用和结果会随 Assistant/Tool Message 在 Turn 结束时保存，因此聊天回放不会丢失；但这不等于已经具备崩溃中途恢复和副作用对账。

下一阶段必须在 Core 的真实语义点执行：

```text
Persist Intent -> Execute -> Persist Outcome
```

不能重新引入同步 callback + 后台 best-effort 写入。

## 7. Migration 与启动

从仓库根目录运行：

```bash
cargo run -p openwork-persistence --bin openwork-migrate
```

该命令创建/更新 Provider 和 Journal schema，并执行 `drop_legacy_session_tables`。然后启动：

```bash
cd apps/desktop
pnpm tauri dev
```

Desktop 启动只检查 `schema_migrations/providers/provider_models/recorded_events`，不会自动建表。

## 8. Crate 状态

| crate | 当前状态 |
| --- | --- |
| `openwork-persistence` | 拥有 Provider Repository、Event Journal、Session Repository 和 migration |
| `openwork-session` | 已删除，职责迁入 Persistence |
| `openwork-db-macros` | 已删除，不再需要通用 `PgEntity` |
| `openwork-database` | 已删除；连接池和 migration runner 已内聚到 Persistence |
