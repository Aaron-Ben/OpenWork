# Agent Note: Column 类型替换终态标记，领取即推进并可超时接手

Status: legacy

## 问题

原来 Column 只有 `is_terminal`，只能区分“完成”列。Agent 领取卡片后，系统不知道哪一列表示“进行中”。工作做完了，看板仍显示 Todo。Cumora 遇到过同一问题，`server/src/agents/board-columns.ts` 的注释记录了它（issue #69）。

领取还要处理负责人不再工作的卡片。卡片不能被一个停下的 Agent 永远占着。但负责人还在工作时，别人也不能把卡片接走，否则会重复劳动。

接手要看“多久没更新”。如果旁边卡片进出也算更新，这个计时就不可靠。

## 决策

- `collab_board_columns.kind` 取 `todo`、`doing`、`done` 或空。迁移 `202609240006_column_kind.sql` 按 `is_terminal` 与默认标题 `Todo` / `Doing` 回填，然后删除 `is_terminal`。protocol 用 `ColumnKind` 与 `BoardColumnView.kind`。
- Agenda 与 Run 的卡片候选用 `kind IS DISTINCT FROM 'done'` 排除完成列（`server/agenda.rs`、`server/runs.rs`）。
- 领取在 `crates/openwork-collab/src/server/board/claim.rs` 的 `Board::claim_card_in`。`claim_target` 只把 `todo` 列的卡片推进到最左的 `doing` 列，照 Cumora `board-columns.ts` 的 `claimTargetColumn`。
- `refusal` 决定能否领取：`done` 列一律拒绝；别人负责时，负责人已归档才可接手，或者卡片超过 `TAKEOVER_IDLE_MINUTES`（20 分钟）没更新且负责人没有 running Run。负责人状态在卡片行锁内读取（`locked_holder`）。
- 拒绝返回 `CONFLICT`，文本写明当前负责人，并提示去做别的卡片。
- 只有卡片本身的创建、修改、改派、领取或移动才刷新 `updated_at`。`server/board/mod.rs` 的 `renumber` 只写 position，`touch_card` 只刷新被移动的那一张。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §11.1、§11.3 与 §13.6。

## 考虑过的方案

**保留 `is_terminal`，另加 `kind`。** 决定直接替换。迁移注释写明：语义已完整转入 `kind`，不保留两份。两个字段并存时，可能一个说完成、一个说进行中。

**照 Cumora 只看 20 分钟。** Cumora `cli.ts` 的 `card claim` 只比较 `updated_at`。没有采用：编码任务的一个 Turn 常超过 20 分钟，而且中途不更新卡片。只看时间，原负责人还在工作时卡片就会被接走。所以加上“负责人没有 running Run”，并让已归档的负责人立即让出卡片。

**重排时一并刷新同列卡片的更新时间。** 这是 Column 类型与领取最初的实现。没有采用：同列前面的卡片移走后，后面每张卡片的计时都被清零，超时接手可能永远不触发。修正时先写了失败的测试步骤，再改实现。

**照 Cumora 给 position 留空档。** Cumora 的 `card move` 取 `MAX(position) + 1000`，只改被移动的那一张。修正时只把它作为对照。OpenWork 保留从 0 开始的连续 position 与可延迟唯一约束，只在重排时不写更新时间。

## 后果

- 列名可以随意改，语义只看 `kind`。新建的列默认未分类，领取不移动其中的卡片。迁移前的自定义列迁移后同样是未分类。
- “负责人没有 running Run”按 Agent 判断，不按卡片判断。负责人在做别的卡片时，这张卡片同样不能被接手。
- `done` 列的卡片领取时直接拒绝。Cumora 的 SQL 不检查所在列，只是不移动卡片。
- Desktop 看板的 `updatedAt` 也只反映卡片本身的变化。
- 领取不产生卡片唤醒，见 [卡片唤醒持久化](2026-09-24-persistent-card-wakes.md)。
- 测试：`server::board::claim::tests::acc_13_claim_moves_only_from_todo_to_the_leftmost_doing`、`acc_13_takeover_needs_an_archived_holder_or_an_idle_card_without_a_running_run`、`board_agenda::acc_13_claim_advances_todo_and_takes_over_only_archived_or_idle_stale_work`。
