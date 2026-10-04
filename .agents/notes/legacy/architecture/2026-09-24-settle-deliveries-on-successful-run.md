# Agent Note: 成功的 Run 结算全部 delivery，沉默也算处理过

Status: legacy

## 问题

delivery 原来只在三种情况下结算：Agent 发了消息（`action`），调了 `openwork ack`（`ack`），triage 判为不需要回应（`triage_false`）。协作契约从没提到 `openwork ack`，人类消息又不经过 triage 模型。所以读了房间、选择沉默的 Agent 永远不结算。下一次 poll 又拿到同一批消息，再跑一轮完整 Turn。

真实模型实测中，被点名的是别人时，沉默的 Agent 每 20 秒跑一轮。5 分钟产生了 34 个 Run，而且不会停。

## 决策

- `Runs::finish`（`crates/openwork-collab/src/server/runs.rs`）在 Run 以 `completed` 结束时，先把 `eligible_reason` 为空的 delivery 记为 `completed`。然后结算本 Run 的全部 delivery，推进 `collab_room_members.last_read_seq`。
- 同一事务里还结算卡片唤醒（`CardWakes::settle_in`），并算出 outcome：有动作为 `acted`，沉默为 `silent`，有文本但没发布为 `unpublished`。
- `failed`、`cancelled`、`interrupted` 不结算，下次重新读取。
- 迁移 `202609240001_settle_completed_runs.sql` 给 `eligible_reason` 的 CHECK 加入 `completed`。

这与 Cumora daemon 相同：引擎没有出错时，Turn 结束后调用 `ackSeen`，把本轮看到的消息标为已读（`server/src/agents/computer/daemon.ts`）。

规则见 [collaboration.md §8.4、§14](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

<!-- agent-note: 原始记录没有备选方案 -->

## 后果

- Agent 沉默不再导致反复唤醒。复现测试 `messaging::a_completed_silent_run_settles_its_delivery_so_the_agent_is_not_woken_again` 先失败（`last_read_seq` 为 0），修复后通过。
- “成功”的意思变了：Engine 正常结束，就算本批消息处理过。模型没在增量里看到的消息也会被结算。所以每轮增量必须写明未显示的条数与读取命令（collaboration.md §7.2）。
- 超出本批预算的消息不在 delivery 中，不受影响，会在后续 Run 中出现。
- 失败路径仍是 at-least-once。失败、取消或中断后，同一批消息会再来一次，模型调用和回复可能重复。
- 与 Cumora 的不同：Cumora 的 `ackSeen` 是 daemon 另发的尽力而为请求（`runtimeBest`）。OpenWork 在 Server 结束 Run 的事务里结算，与 Run 状态一起提交，不会只成功一半。
