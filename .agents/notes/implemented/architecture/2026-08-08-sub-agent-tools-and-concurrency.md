# Agent Note: 五个控制工具，按活跃 Turn 限并发，按任务形态触发委派

Status: implemented

## 问题

主 Agent 要能派生、等待、查看、追加与中断子 Agent。工具太多，模型难选；太少，常见用法要绕路。

并发需要上限，否则一次响应可以开出任意多个子 Turn。上限按什么计数，决定了空闲的子 Agent 能不能被再次使用。

模型还要知道什么时候该委派。规则太保守，能力等于不存在；太宽松，模型会把小问题也交出去。

## 决策

- 五个工具：`spawn_agent`、`wait_agent`、`list_agents`、`followup_task`、`interrupt_agent`。它们由 Core 定义（`crates/openwork-core/src/agent/tool.rs`），与 `update_plan` 一样走 `ResolvedTurnTool`，不注册进 `openwork-tools`。
- 上限按同时活跃的子 Turn 计数，默认 3（`crates/openwork-core/src/agent/limiter.rs`）。名额是 RAII 的 `TurnSlot`，Turn 以任何方式结束都归还。
- 没有累计总数上限。
- `wait_agent` 的范围是 10 秒到 10 分钟，默认 60 秒。
- 工具描述与系统提示词按任务形态写“该用”与“不该用”，并写明并发上限。不要求用户先提出委派。

规则见 [multi-agent.md §5、§7.1](../../../../docs/subsystems/multi-agent.md)。

## 考虑过的方案

**`send_message`。** Codex 有这个工具（`codex-rs/core/src/tools/handlers/multi_agents_spec.rs`）。它给运行中的子 Agent 补信息，但不让它开工。对只读调查几乎没用。没有采用。

**`close_agent`。** Codex v1 有这个工具（同一文件）。空闲的子 Agent 不占名额，父会话卸载时一并关闭，不需要它。没有采用。

**按注册的子 Agent 数计数。** 空闲的子 Agent 会一直占名额，`followup_task` 就失去意义。没有采用。

**在成功路径上手动减一。** 派生与开始 Turn 有多个提前返回点，手工配平一定会漏，名额泄漏到重启。没有采用。

**累计总数上限。** 连续问不同问题是正常用法，按累计数封顶会误伤它。父 Turn 的 `max_model_calls` 已经间接限制派生次数。没有采用。

**照 Codex 只在用户明确要求时委派。** Codex 的 `spawn_agent` 描述写着除非用户或 AGENTS.md 明确要求，否则不要派生（`multi_agents_spec.rs`）。没有采用：explorer 只读、单层、并发 3，最坏情况只是多花 token。必须等用户开口才能用的能力，等于不存在。

**`wait_agent` 上限取 Codex 的 1 小时。** Codex 的默认上限是 `3600 * 1000` 毫秒，下限 10 秒，默认 30 秒（`codex-rs/core/src/config/mod.rs`）。没有采用 1 小时：卡住时只能靠用户取消 Turn。下限 10 秒用来挡住忙轮询。

**空闲子 Agent 的 LRU 卸载。** 原设计文档写 Codex 有这一机制，源码里没有核实（未确认）。没有采用：并发上限是 3，空闲 actor 的开销可以忽略。

## 后果

- 父可以用 `followup_task` 在空闲子 Agent 的已有上下文上继续调查。
- 第 4 个并发子 Turn 得到 `agent_limit_reached`，由模型决定等待还是缩小范围。
- 一个根会话之内的派生总数没有硬上限，只受 `max_model_calls` 约束。
- `wait_agent` 连续超时 3 次时，Turn 以 `doom_loop` 失败，防止空等。
