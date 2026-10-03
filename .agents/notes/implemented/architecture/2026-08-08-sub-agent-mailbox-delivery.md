# Agent Note: 子 Agent 的结果以 user-role 信封进 mailbox，在 Model Call 前排空

Status: implemented

## 问题

子 Agent 异步完成。它的结论要进入父的 Conversation，但有四个约束：

- provider 要求 `assistant(tool_calls)` 之后紧跟全部 `tool_results`，中间不能插消息。
- 父会话空闲时，自动开一个 Turn 会产生用户没有发起的 Model Call 与费用。
- 父要尽早看到结果，最好在同一个 Turn 里。
- 进程重启后，内存里还没交给父的结果会丢失。

## 决策

- 子 `SessionActor` 在 Turn 结束时自己投递（`deliver_terminal_outcome`，`crates/openwork-core/src/session/actor.rs`）。`DeliverAgentMessage` 只入队，永不创建 Turn。
- run loop 在每次 Model Call 前排空 mailbox（`drain_agent_messages`，`crates/openwork-core/src/session/run_loop/mod.rs`）。这一点总在上一轮全部 Tool Result 之后。
- 消息以 User role、`message_kind = 'agent_message'` 写入，正文是 `<agent_message>` 信封（`crates/openwork-core/src/session/agent_message.rs`）。
- `MessageKind::AgentMessage` 是 contextual kind。`last_real_user`（`crates/openwork-core/src/session/compaction/compacted_view.rs`）按 kind 判断，不按写入顺序判断。
- mailbox 不持久化。父的每个用户 Turn 开始前，Core 用确定性消息 ID 对账补发。
- `wait_agent` 等 mailbox 出现任意消息，不接受目标列表，返回值不含正文。

规则见 [multi-agent.md §5.1、§6、§8](../../../../docs/subsystems/multi-agent.md)。

## 考虑过的方案

**assistant role。** 语义上它不是用户说的话。Codex 的 `InterAgentCommunication::to_response_input_item` 用 assistant role（`codex-rs/protocol/src/protocol.rs`）。没有采用：Anthropic adapter 直接透传 role（`crates/openwork-models/src/adapters/anthropic_messages/request.rs`）。排空点紧挨 Model Request，assistant 消息会成为最后一条，被当成 prefill，模型会接着它往下写。在 adapter 里补救要让 `openwork-models` 知道 Agent，依赖方向反了。

**按写入顺序找最后一条 user 消息。** Skill 正文写在用户消息之前，按顺序选恰好正确。agent message 在 Turn 中途写在用户消息之后，会被当成 last-user replay，把真实请求挤出压缩投影。没有采用：改为给 `ConversationItem` 加 kind。

**父空闲时自动开 Turn。** 产生用户没有授权的费用，还引入后台任务语义。没有采用。

**投递阶段状态机。** Codex 用 `MailboxDeliveryPhase`（`codex-rs/core/src/state/turn.rs`）记住本 Turn 是否已输出最终回答，并据此把消息留到下一个 Turn（`codex-rs/core/src/session/input_queue.rs`）。OpenWork 的 run loop 在没有 Tool Call 时立即结束 Turn，最终回答之后没有排空点，效果相同。原设计文档写 Codex 需要它是因为 review 与 compact 这类 Turn，源码里没有找到依据（未确认）。

**`wait_agent` 等待指定的子 Agent。** Codex v1 的 `wait_agent` 有 `targets` 参数，v2 去掉了（`codex-rs/core/src/tools/handlers/multi_agents_spec.rs`）。没有采用：等 A 时 B 先完成会让父看不到 B。

**`wait_agent` 返回子 Agent 的回答。** 同一内容会在 Conversation 里出现两次，两份可能不一致。没有采用。

**持久化 mailbox。** 重启时子 Turn 必然中断。能多保住的只有“已完成但父还没读”这一窄窗口，对账已经覆盖它。没有采用。

**后台 watcher 监听子 Turn。** 每个子 Agent 多一个 tokio task，回传也脱离子 Agent 自己的上下文。没有采用。

## 后果

- 子 Agent 在父 Turn 运行中完成时，父在同一 Turn 的下一次 Model Call 就能看到结果。
- 父输出最终回答后到达的结果，要等用户下一次输入。
- 这依赖“无 Tool Call 立即结束 Turn”（[session-runtime.md](../../../../docs/subsystems/session-runtime.md) §4 第 7 条）。如果以后在 Turn 结束前再调用一次模型，就要补上投递阶段状态机。
- 对账的消息 ID 必须与实时回传一致。现在 `Cancelled` 在实时路径记为 `interrupted`，在对账路径记为 `failed`，ID 不一致。
