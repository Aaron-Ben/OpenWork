# Agent Note: 压缩后经 reminder 重新给出计划

Status: implemented

## 问题

模型平时从 Conversation 看到计划：`update_plan` 的 Tool Call 参数与 `Plan updated` 结果。压缩把 Conversation 换成用户消息重放、摘要与 reminder 三条，这两条消息随之离开投影。之后 reminder 是计划唯一的载体。它出错时，模型会重做已经完成的步骤。

## 决策

- 不在每次 Model Call 另外注入计划。
- `PlanStateContributor`（`crates/openwork-core/src/plan/projection.rs`）是压缩 collector 的第二个 contributor。Runner 把内存中的当前计划带进压缩请求。contributor 不查库，也不解析 Tool Call。
- 空计划覆盖旧值；没有计划输入时，collector 结转旧值。
- 状态标记用 wire 名称，例如 `[in_progress]`。
- 计划不进 System Context，也不是 world state 的 section。
- 步数与步长有上限。超限整体拒绝，不截断。

设计见 [update-plan.md §2.2、§7](../../../../docs/subsystems/update-plan.md)。

## 考虑过的方案

**每次 Model Call 注入当前计划。** 没有采用：内容与 Conversation 中的调用重复，两份渲染可能漂移。

**放进 System Context。** 没有采用：计划在 Turn 内变化快，会破坏稳定前缀。压缩也管不了它的生命周期。

**照 Codex 做增量交付账本。** Codex 在 `codex-rs/core/src/context/world_state/` 记录模型见过什么。没有采用：计划只需要压缩时重新投影。OpenWork 后来有了 world state（`crates/openwork-core/src/context/world_state/`），计划仍不在其中。

**另建一条投影管线。** 没有采用：collector 已有 key 去重、失败策略、`extensions` 持久化与长度检查。

**用 `[x]`、`[>]` 这类符号。** 没有采用：reminder 由 XML 包裹，`>` 会转义成 `&gt;`。这也多出一套要对齐的词汇。

**超限时截断。** 没有采用：模型会以为提交成功，而存下的是另一份计划。

## 后果

- 上限没有保证 reminder 不超长。128 步乘 1 024 字符远超 32 768 字符的 reminder 上限，`explanation` 也不限长。超长时压缩失败，Turn 失败。
- 结转规则让 rewind 与手动压缩保留计划。它也让新 Turn 在调用 `update_plan` 前的压缩带上旧计划。
- contributor 依赖内存计划与 `turn_plans` 一致。提交路径先写库，再更新内存。
