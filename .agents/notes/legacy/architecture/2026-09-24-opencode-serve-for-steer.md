# Agent Note: 为 steer 改用每 Agent 一个常驻 opencode serve

Status: legacy

## 问题

steer 指 Turn 进行中把新消息插入这个 Turn。没有 steer 时，Agent 在长 Turn 中收不到新消息，只能等 Turn 结束后的下一轮。用户在 Agent 编码时补充一句话，要等整个 Turn 跑完才生效。

当前 OpenCode adapter 每个 Turn 启动一次 `opencode run`，prompt 经 stdin 一次写完（`crates/openwork-collab/src/computer/opencode/mod.rs` 的 `execute_turn`）。进程运行中没有通道接收新消息。

## 提议

- OpenCode adapter 改为每个 Agent 一个常驻的 `opencode serve`。
- Turn 进行中到达的消息，作为同一 session 的新 prompt 送入。opencode `opencode:packages/opencode/src/session/prompt.ts` 的 `prompt()` 调用 `loop()`，`loop()` 经 `state.ensureRunning` 接入同一进程中正在运行的 loop。

## 考虑过的方案

**保持每 Turn 一个 `opencode run`。** 这是当前实现，没有 steer。Cumora 的 OpenCode adapter 也一样：`cumora:server/src/agents/computer/engine.ts` 的注释称 OpenCode 为 one-shot engine，`OpenCodeAdapter` 没有实现 `steer`。

**所有 Agent 共用一个 `opencode serve`。** 这是最初的设计（2026-08-18 的协作设计文档）。它被每 Turn 一个 `opencode run` 取代。当时记录的风险是单点：它退出时全体 Agent 停止。

## 验收条件

- Turn 进行中，用户的新消息在同一 Turn 内被模型读到。
- 常驻进程退出后，Computer 重建它，并按 session id 恢复上下文。
- 常驻进程仍在本 Agent 的 Seatbelt 围栏内运行。

## 风险

- 常驻进程要管理生命周期：启动、健康检查、崩溃重建与关闭。
- Engine 沙箱与登录信息的注入方式要重新验证。
