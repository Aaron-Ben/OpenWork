# Agent Note: update_plan 是 Core 控制工具

Status: implemented

## 问题

`update_plan` 改的是一个 Turn 的计划，不读写工作区，也不启动进程。`openwork-tools` 的普通工具只拿到 `ToolCallContext`，它不知道当前 Turn。`ToolRisk` 只有只读、工作区变更与进程执行三类。`FinalizedToolset` 的 `validate`、`authorize`、`call` 三个入口各自按名称查表。

## 决策

- 定义、解析与校验在 `crates/openwork-core/src/plan/`。`openwork-tools` 不知道这个工具。
- `TurnToolset`（`crates/openwork-core/src/session/toolset.rs`）在普通工具集之上加控制工具。Runner 只调用一次 `resolve`，之后按 `ResolvedTurnTool` 分支。
- `UpdatePlan` 分支不经执行前判定与沙箱。Trace 记 `allow` + `control_tool`。
- 同一个 `TurnToolset` 产生工具定义与 Prompt 规则。
- 重名在构造 `TurnToolset` 时失败。
- doom loop 对 `update_plan` 照常计数。

设计见 [update-plan.md §3、§4、§5](../../../../docs/subsystems/update-plan.md)。

## 考虑过的方案

**注册为普通工具，风险标为 `ReadOnly`。** 没有采用：权限语义会失真。它还要给所有工具的 Context 加 Turn 与计划存储，只为一个工具服务。

**各阶段按字符串判断 `"update_plan"`。** 没有采用：控制工具的判断会复制到校验、权限、执行多处，改一处容易漏一处。

**Trace 复用 `builtin` 来源。** 没有采用：回看时要分得出“它是控制工具”与“内置规则放行了它”。

**先抽象 `ControlTool` trait 或注册表。** 没有采用：当时只有一个控制工具。后来的五个子 Agent 工具只给枚举加了一个 `Agent` 变体。

**doom loop 豁免 `update_plan`。** 没有采用：连续提交同一份计划，本身就是没有进展的信号。

Codex 的做法相同：`codex-rs/core/src/tools/handlers/plan.rs` 的 `PlanHandler` 声明 `is_builtin_control_tool`，成功输出是 `Plan updated`。

## 后果

- 新增控制工具时，要改 `ResolvedTurnTool`、重名检查与 Runner 的分支。每个新控制工具要单独判断能不能免审批。
- 控制工具的参数不经注册表的 schema 校验，由 `plan` 模块解析。
- 子 Agent 没有这个工具。它调用 `update_plan` 时得到 `ToolNotFound`。
- 只有在计划上没有进展时，doom loop 才会拦下 `update_plan`。其他调用会重置计数。
