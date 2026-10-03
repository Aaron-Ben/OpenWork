# Agent Note: 计划存为每个 Turn 的最新快照

Status: implemented

## 问题

Desktop 会重载页面，也会在同进程里漏掉事件后重连。Codex 把计划更新当作瞬时事件：`codex-rs/rollout/src/policy.rs` 的 `should_persist_event_msg` 对 `EventMsg::PlanUpdate` 返回 `false`。如果计划只存在于事件里，漏掉一次广播或刷新页面后，就无法重建计划。

## 决策

- `turn_plans` 每个 Turn 一行，只存最新快照。
- `commit_plan_update` 在一个事务里 upsert 计划，并追加成功 Tool Result。没有只写计划的接口。
- Conversation 中的 Tool Call 是审计事实；`turn_plans` 是当前状态；`PlanUpdated` 是可以丢失的投影。
- 事件带完整快照与 `updated_at`。Actor 把它折进 Snapshot。
- 历史计划按 Session 一次读出，返回列表。

设计见 [update-plan.md §6、§8](../../../../docs/subsystems/update-plan.md)。

## 考虑过的方案

**照 Codex 只广播事件。** 没有采用，理由见“问题”。

**从历史 Tool Call 重建计划。** 没有采用：失败的调用也在 Conversation 里。界面要的是最后生效的计划。

**`plan_revisions` 历史表。** 没有采用：Conversation 已记录每次调用，没有回滚需求。

**`plan_steps` 子表。** 没有采用：计划只整体替换、整体读取。拆表只增加写入与排序的工作。

**冗余 `session_id` 列与 `revision` 乐观锁。** 没有采用：`turn_id` 已定位 Session。一个 Runner 串行执行同一个 Turn 的调用。

**两次独立写入，失败时补偿。** 没有采用：补偿也会失败，那时没有第三处记录真相。

**事件不带 `updated_at`。** 没有采用：Actor 无法把事件无损折进 Snapshot，前端也要为实时与历史计划各写一套渲染。

**加载接口返回按 `turnId` 索引的对象。** 没有采用：Rust DTO 保持列表，前端在投影边界建一次索引。

## 后果

- 计划在 Tool Call 参数与 `turn_plans` 中各有一份。同一个事务让两者一致。
- 提交失败时 Turn 失败，这次调用没有 Tool Result。
- `turn_plans` 不记录修改次数。Desktop 的“已更新 N 次”数的是 Conversation 中的 Tool Call。
- Rust 与 TypeScript 各写一份计划类型（`desktop/src/bridge/compat.ts`），没有生成，也没有对拍测试。
