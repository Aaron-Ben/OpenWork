# Agent Note: 不上报 Runner 暂停

Status: implemented

## 问题

Engine 限流或未登录时，Computer 让该 Agent 退避一段时间。限流按 Engine 给出的 retry-after 或本地 60 秒退避。未登录暂停 15 分钟。

原计划让 Desktop 显示这段暂停。Runner heartbeat 增加 `paused`，`AgentView.activity` 增加 `paused`，`CardView.agentState` 增加 `notified`，设计稿 v3 也画了暂停样式。这要新增协议状态、Server 内存状态与界面分支。

要定的问题是：暂停是否值得成为一个独立的、跨进程上报的状态。

## 决策

不上报 Runner 暂停（用户决定，2026-09-25）。

- heartbeat 只上报 `running` 或 `error`：`crates/openwork-collab/src/protocol/computer.rs` 的 `RunnerState`。
- `AgentActivity` 只有五种：working、queued、error、idle、archived。`CardAgentState` 只有 working 与 queued（`protocol/desktop.rs`，计算在 `server/activity.rs`）。
- 退避只在 Computer 本地生效：`computer/scheduling.rs` 的 `engine_backoff_after` 与 `OPERATOR_FIX_BACKOFF`。导致退避的那次 Run 照常以失败结算。
- 退避期间，Agent 按其他条件显示，通常是排队或空闲。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §5、§6，界面见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md) §4.1。

## 考虑过的方案

**heartbeat 上报 `paused`，界面显示暂停（原计划）。** 没有采用：暂停状态是 OpenWork 自己加的。Cumora 只把退避原因写进本地日志：`server/src/agents/computer/daemon.ts` 的 `engineBackoffWhy` 只出现在 `console.log` 中。实现 `AgentView.activity` 时，Computer 端已经加过暂停上报，按这一决定撤回。设计稿 v3 的暂停样式也随之去掉。

## 后果

- 协议、Server 内存状态与界面各少一个状态。`activity` 的判定顺序更短：归档 → 工作中 → 出错 → 排队 → 空闲。
- 代价：用户在 Agent 页看不到“这个 Agent 在等限流解除”。限流期间，Agent 可能显示为空闲，看起来像没在工作。
- heartbeat 的 `error` 只来自 Runner 启动失败或 Runner 任务退出（`computer/daemon.rs` 的 `record_runner_error`）。Turn 中的 Engine 未登录不改变它，只表现为失败的 Run。
- 要查退避原因，看运行记录页中失败的 Run，见 [运行记录页](../feature/2026-09-24-run-records-page.md)。
- 测试：`server::activity::tests::activity_takes_the_first_matching_state_in_the_documented_order`、`agent_activity::agent_activity_follows_runs_card_wakes_and_runner_heartbeats`。
