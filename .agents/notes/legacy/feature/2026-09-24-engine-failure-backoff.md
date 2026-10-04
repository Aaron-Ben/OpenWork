# Agent Note: Engine 未登录暂停 15 分钟，限流暂停 60 秒

Status: legacy

## 问题

失败的 Run 不结算 delivery，下次 poll 会再拿到同一批消息。OpenCode 未登录时，每次 poll 都重新拉起 Engine，并留下一条失败的 Run。限流时，立即重试会继续消耗同一份额度。这两种失败都不会因为马上重试而消失。

## 决策

`engine_backoff_after`（`crates/openwork-collab/src/computer/scheduling.rs`）把一次失败映射为暂停时长：

- `EngineError::Unauthenticated`：`OPERATOR_FIX_BACKOFF`，15 分钟。这个数字来自 Cumora `server/src/agents/computer/daemon.ts` 的 `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`。
- `EngineError::RateLimited`：有 retry-after 时用它，否则用 `RATE_LIMIT_BACKOFF`，60 秒。
- 其他错误：`None`，照常重试。

`AgentRunner::execute_main_run`（`computer/runner/mod.rs`）在正式 Turn 失败后设置 `engine_backoff_until`。`drive_once` 在读 inbox 之前检查它。所以暂停期间，聊天、卡片 Turn 与 Agenda 都不启动，triage 也不运行。导致暂停的 Run 照常以失败结算。

OpenCode adapter 按错误文本识别失败（`computer/opencode/mod.rs`）。先识别限流：`rate_limit`、`429`、`quota`、`503` 等。再识别未登录：`not logged in`、`unauthorized`、`401` 等。

规则见 [collaboration.md §6、§14](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**向 Desktop 上报暂停。** 原计划在 heartbeat 中上报 `paused`，设计稿 v3 也画了暂停样式。2026-09-25 用户决定撤销它：暂停状态是 OpenWork 自己加的，Cumora 只把退避原因写进本地日志（`daemon.ts` 的 `engineBackoffWhy`）。现在暂停只在 Computer 本地生效。

## 后果

- 未登录的 Agent 每 15 分钟最多拉起一次 Engine。Cumora 在这个常量的注释里记录了没有暂停时的空转规模。
- 用户重新登录后，最多要等 15 分钟才恢复。Desktop 看不到暂停，只看到失败的 Run。
- 当前 OpenCode adapter 不解析 retry-after，`retry_after` 总是 `None`。所以限流实际总是暂停 60 秒。Cumora 也固定用 60 秒（`ENGINE_BACKOFF_AFTER_RATE_LIMIT_MS`）。
- 按文本识别是尽力而为。不在两个清单里的失败不暂停，例如 `Model not found` 会在每次 poll 重试。
- 额度不足的文本含 `quota` 时按限流处理，只暂停 60 秒。没有 `quota` 的额度文本不暂停。
- 这条规则只管正式 Turn。triage 模型的失败另有规则，见 [triage 失败处理](../architecture/2026-09-24-triage-failure-handling.md)。
- 验收：`computer::scheduling::tests::engine_failures_that_need_the_user_pause_the_agent_longer_than_rate_limits`。
