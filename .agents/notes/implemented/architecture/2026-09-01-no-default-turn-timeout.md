# Agent Note: 正式 Turn 默认没有超时

Status: implemented

## 问题

一个正式 Turn 可能跑很久：编码任务会编译、跑测试，中途长时间没有输出。Engine 也可能真的卡住。超时太短，会杀掉正常工作；完全没有超时，又要靠别的路径终止卡住的进程。

## 决策

- `EngineRuntimeConfig.turn_timeout` 是 `Option<Duration>`（`crates/openwork-collab/src/computer/engine.rs`）。Computer 创建 Runner 时传 `None`（`computer/daemon.rs` 的 `RunnerFactory::start`）。
- OpenCode adapter 只在 `absolute_timeout` 有值时设置截止时间（`computer/opencode/mod.rs`）。没有“无输出”计时。
- 分类调用仍有固定的 60 秒上限（`CLASSIFY_TIMEOUT`）。
- 用户停止 Agent 或退出 Desktop 时，取消路径终止 Engine 进程组。

规则见 [collaboration.md §6](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**5 分钟无输出超时，加 30 分钟总超时。** 这是此前的实现（`NO_OUTPUT_TIMEOUT` 与 `MAIN_TIMEOUT`）。没有保留：长时间无输出本身不表示 Engine 已失效。

## 后果

- 长时间编译或测试的 Turn 不会被误杀。
- 真正卡住的 Engine 会一直占用该 Agent 与一个主模型并发名额，直到用户停止 Agent 或退出 Desktop。
- 产品以后需要上限时，给 `turn_timeout` 一个值即可。
- 测试：`tests/opencode_adapter.rs::main_turn_has_no_default_silence_or_wall_clock_timeout`、`::an_explicit_main_turn_timeout_remains_available`。
