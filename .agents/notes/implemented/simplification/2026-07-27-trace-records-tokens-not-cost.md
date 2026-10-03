# Agent Note: Trace 只记 token，不记成本

Status: implemented

## 问题

Trace 记录每次调用的 token。用户自然想知道一次 Turn、一个 Session 花了多少钱。

成本功能曾经完整实现，并通过了全部验收。之后发现它在日常使用中基本失效：任何失败或取消的 Model Call 都会把整个 Turn 的成本标为未知；一个 Session 里只要有一个未知 Turn，汇总就显示“—”。取消是日常操作，一次取消就让这个 Session 的成本永久不可见。

## 决策

- Trace 与 `turns` 只记录四个 token 列，不存单价，不算金额，不做汇总。单价列、金额列、汇总查询与界面全部删除。
- 保留两样与计价无关的东西：Provider 的 token 口径表（[trace.md §11](../../../../docs/subsystems/trace.md)），以及 `TokenUsage.cache_creation_input_tokens` 的解析（`crates/openwork-models/src/model/response.rs`）。
- 需要估算花费的人用 token 乘以单价。

## 考虑过的方案

**修复成本功能。** 根因是 `turns.cost_amount` 用一个可空列表达两种状态：“还没记录任何成本”与“有一项无法定价，总额不可知”。一个 NULL 区分不了两者，累加逻辑只能用 `model_submission_count = 1 AND NOT EXISTS (压缩)` 这类代理条件猜“我是不是第一个”。这些条件在顺利路径上成立，在任何失败路径上都不成立。例如 overflow 重试的 Turn，即使每一步都有价格，最终成本仍为空。修复要引入显式的成本状态列，并区分“请求未发出”和“已发出但结果未知”；`deliveryState` 已经有后者的信息。这是一次完整的设计迭代，不是补丁。没有采用：这个功能的价值不值得这个复杂度。

## 后果

- 界面上没有金额。跨 Provider 比较 token 时，仍要先按 `resolved_provider_kind` 分组，因为 `cached_input_tokens` 是否计入 `input_tokens` 各家不同。
- Provider 口径表是开发成本功能时查到的。它描述 token 语义，所以保留。
- 以后重做成本时，先解决状态二义性，再写代码。
