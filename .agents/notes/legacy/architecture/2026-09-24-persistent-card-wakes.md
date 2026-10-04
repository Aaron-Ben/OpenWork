# Agent Note: 卡片唤醒持久化，由唤醒记录指向处理它的 Run

Status: legacy

## 问题

卡片改派给某个 Agent，或描述里新点名某个 Agent 时，这个 Agent 应该醒来处理。这类工作不经过 triage：有人直接把工作交给了它。

Cumora 的卡片唤醒是尽力而为的。`server/src/agents/kanban-wake.ts` 的 `wakeKanbanAgents` 在看板写入提交后另行唤醒，失败只写日志。OpenWork 的 Runner 可能正忙、正在退避，或者 Run 失败后要重试。只靠一次性唤醒，交出去的卡片可能没人处理，用户也看不到它在排队。

还有一个问题：一个 Run 可能同时处理多张卡片。Server 要能回答“这个 Run 处理了哪些卡片”，才能只在成功后结算它们。

## 决策

- 卡片唤醒写入 `collab_card_wakes`（迁移 `202609240007_card_wakes.sql`）。部分唯一索引 `uq_collab_card_wakes_pending` 保证每个 Agent 的每张卡片只有一条待处理记录。
- `crates/openwork-collab/src/server/card_wakes.rs` 的 `CardWakes` 负责全流程：`targets_in` 判定唤醒谁，`record_in` 写入或合并，`with_pending` 随收件箱读出，`attach_in` 在 Run 打开时认领，`settle_in` 在 Run 成功后结算。后两者由 `server/runs.rs` 调用。
- 合并时 `revision` 加 1，并清空 `run_id`。trigger 携带记录的 id 与版本号，Run 打开时只认领版本号未变的记录。
- `collab_card_wakes.run_id` 表示“这个 Run 处理这些卡片”，Run 的 trigger 为 `card`。`collab_runs.focus_card_id` 只用于 Agenda：约束 `collab_runs_agenda_focus_shape` 要求非 Agenda Run 的这一列为空。
- 一个卡片 Turn 最多 10 张卡片，按 `created_at, id` 取最早的（`CARD_TURN_MAX_CARDS`）。prompt 写明还有几张在排队（`computer/prompt.rs` 的 `card_turn_prompt`）。
- Agent 触发的卡片唤醒与消息唤醒共用 `openwork:turn-rate:<agent>` 计数，每分钟 30 次。超出时不写入；Redis 出错时放行（`server/agent_commands/cards.rs` 的 `within_wake_limit`）。Desktop 触发的不限（`server/desktop_cards.rs`）。
- Runner 收到 `card` trigger 时走 `computer/runner/card.rs`，不经过 triage。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §11.4、§13.3.6 与 §13.6。

## 考虑过的方案

**照 Cumora 尽力而为。** 唤醒只通过 SSE 送达，不留记录。没有采用：Runner 忙、Redis 断开或 Run 失败时，这次交接就丢了。持久记录让 SSE 只负责“尽快”，不负责“送到”。

**Run 带上 `focus_card_id`。** 讨论时的做法是让卡片 Run 记录它处理的卡片。没有采用：一列只能写一张卡片，无法表达一个 Run 处理多张卡片。这一列也已经专属于 Agenda。改由唤醒记录的 `run_id` 指向 Run，一个 Run 可以认领任意多条记录。

**照 Cumora 不限张数，只截断说明。** Cumora 合并多次唤醒的 brief，再截到 12,000 字（`server/src/agents/runtime/wake-options.ts` 的 `MAX_BRIEF_BODY_CHARS`）。截掉的卡片直接丢失，也不告诉模型。没有采用：OpenWork 的唤醒是持久的，排不下的可以留到下一轮，所以固定为 10 张，并写明剩余张数。

## 后果

- Run 失败、取消或中断时，记录保留，下一个 Run 重新认领。Redis 清空也不丢卡片工作。
- Desktop 能显示排队状态：`server/activity.rs` 用未结算的记录算出 `queued`，用指向 running Run 的记录算出正在处理哪张卡片。
- 代价：多一张表，读收件箱与打开 Run 之间多一道版本号核对。
- Run 进行中又有新变化时，记录脱离这个 Run。Run 成功后它仍待处理，Agent 会再醒一次，因为它没看到这次变化。
- 超出限额的 Agent 唤醒直接丢弃，不留记录。卡片本身的变化仍在看板上。
- 同一事务写入的多条记录 `created_at` 相同，此时按 id 排序，不代表真实先后（卡片唤醒的真实模型测试中，一次积压 12 张卡片的场景）。
- 领取只把负责人改成发起者本人，所以不产生唤醒。领取规则见 [Column 类型与卡片领取](2026-09-24-column-kind-and-card-claim.md)。
- 测试：`card_wakes::acc_14_real_changes_wake_once_and_merge_until_a_successful_run_settles_them`、`card_wakes::acc_14_agent_card_wakes_are_rate_limited_and_a_turn_carries_at_most_ten`、`computer::prompt::tests::acc_14_card_turn_prompt_lists_the_cards_and_the_board_commands`。
