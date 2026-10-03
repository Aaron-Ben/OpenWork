# Agent Note: 一次响应里的 Tool Call 串行执行

Status: implemented

## 问题

一次模型响应可以含多个 Tool Call。它们可以并行执行，也可以逐个执行。并行更快，但副作用的先后、结果写回的先后、审批卡片的先后都会变得难以推断。一个调用失败或被拒绝时，其余调用处于什么状态也更难说清。

Provider 又要求：Assistant 的每个 Tool Call 都有结果，并且结果紧跟在它后面。

## 决策

- `TurnRunner::run_loop`（`crates/openwork-core/src/session/run_loop/mod.rs`）按 Provider 给出的顺序逐个执行 Tool Call。每个调用的结果写库、追加进 Chat State 之后，才开始下一个。
- 一个调用让 Turn 终止时（用户拒绝、取消、doom loop、写库失败），同一响应里剩余的调用不执行，按原顺序各得到一条 `cancelled` 结果。Conversation 里不留没有结果的 Tool Call。
- 审批卡片随之一次一张。卡片本身的取舍见 [Agent Note：两个按钮与串行审批](2026-08-01-two-button-cards-and-serial-approvals.md)。

事实见 [session-runtime.md §4、§5](../../../../docs/subsystems/session-runtime.md)。

## 考虑过的方案

**并行执行 Tool Call。** 没有采用。原设计文档写明：先保证消息顺序与副作用可以理解。以后如果并行化，只能并行已经通过权限判定、彼此不冲突的调用，结果仍按原顺序写回。

## 后果

- 互不相关的只读调用也要排队。一次响应里的调用越多，Turn 越慢。
- 结果顺序、写库顺序、界面顺序都与模型给出的顺序一致。排查问题时不需要还原并发时序。
- doom loop 的计数是“连续”的。串行让“连续”有确定的含义。
- 终止之后补出的 `cancelled` 结果让下一次请求合法，但模型看到的是“没有执行”，不是“执行失败”。
- 剩余调用补结果的路径目前没有测试，见 [session-runtime.md §12](../../../../docs/subsystems/session-runtime.md) 第 13 条。
