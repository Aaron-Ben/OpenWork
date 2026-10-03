# Agent Note: Plan mode 与 update_plan 分开

Status: proposed

## 问题

`update_plan` 回答“这次任务做到哪一步”。Plan mode 回答“先与用户商定一份方案”：它改变允许的行为、工具面与最终输出。两者都叫 plan，容易被做成同一个状态机，或共享一个含义不清的 `plan` 状态。OpenWork 还没有 Plan mode。`TurnToolset` 只按根 Session 与子 Agent 决定是否广告 `update_plan`。

## 提议

- 把 Plan mode 做成 Collaboration Mode。由它的权威状态选择工具面。
- Plan mode 下，`TurnToolset` 不广告、也不分派 `update_plan`。
- Plan mode 的最终方案不写入 `turn_plans`。
- 从 Plan mode 转入执行时，如果需要任务清单，在新的执行 Turn 中显式调用 `update_plan`。
- 两者不共享名为 `plan` 的全局状态。

当前的 `update_plan` 见 [update-plan.md](../../../../docs/subsystems/update-plan.md)。

## 考虑过的方案

**照 Codex 在 handler 里拒绝。** Codex 的 `codex-rs/core/src/tools/handlers/plan.rs` 在 `turn.mode() == ModeKind::Plan` 时返回错误。注册条件是 `config.update_plan_enabled`（`codex-rs/core/src/tools/spec_plan.rs` 的 `add_core_utility_tools`），不看模式。Plan mode 下这个配置是否被关闭：未确认。不建议采用：模型可能看到一个调用必然失败的工具。

**用同一个功能实现两者。** 不建议采用：两者的生命周期与输出契约不同。

## 验收条件

- Plan mode 的 Turn 中，工具定义里没有 `update_plan`，调用它得到 `ToolNotFound`。
- Plan mode 的 Turn 结束后，`turn_plans` 没有这个 Turn 的行。
- Default mode 的 `update_plan` 行为与验收不变。

## 风险

- 模式切换发生在 Turn 中间时，工具面怎样变化，还没有定。
- 用户可能期望 Plan mode 的方案自动变成任务清单。这个提议要求模型在执行 Turn 中自己建立清单。
