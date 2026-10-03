# Agent Note: 用 turns 上的计数观测未收尾的计划

Status: implemented

## 问题

Prompt 要求模型在结束前把每一步标成 `completed`。常见的失败是：模型建了计划，却没在 Turn 结束前收尾。Core 不能强制这一点。Turn 因错误、取消或调用上限而结束时，留下未完成步骤是正确的记录。如果没有统计，这条规则是否生效，只能翻聊天记录。

## 决策

- `turns.plan_unfinished_step_count`（迁移 `202608070002_add_turn_plan_completion_signal.sql`）由 `finish_turn` 写入。值来自 Runner 在 Turn 结束时的当前计划。
- `NULL` 表示没有计划，`0` 表示全部收尾，大于 0 是未收尾的步数。
- 失败与取消的 Turn 照样记数。查询时按 `status = 'completed'` 过滤。

设计见 [update-plan.md §10](../../../../docs/subsystems/update-plan.md)。

## 考虑过的方案

**把“全部 completed”定为不变量。** 没有采用：这等于要求失败的 Turn 声称自己做完了。

**记在 Trace 里。** 没有采用：Trace payload 有保留期，过期后被清理。这个指标要跨月比较提示词改动前后。`turns` 上已有 `model_call_count`、`tool_call_count` 这类聚合列。

**默认值 0。** 没有采用：没有计划与全部收尾会混在一起，比例的分母失真。

**写入时排除失败与取消的 Turn。** 没有采用：计数是事实。按状态区分留给查询。

## 后果

- `interrupted` 的 Turn 由 `mark_running_interrupted` 直接改状态，不经 `finish_turn`。这一列保持 `NULL`，与“没有计划”分不开。
- 清空计划（`plan: []`）记 `0`，与全部收尾相同。
- 没有界面显示这个数。要用 SQL 查询。
- 计数取内存中的计划。提交成功、但 Chat State 追加失败时，内存仍是旧计划。
