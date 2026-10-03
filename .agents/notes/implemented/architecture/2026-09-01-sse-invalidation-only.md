# Agent Note: SSE 与 Redis 只传失效提示，正确性靠 PostgreSQL

Status: implemented

## 问题

Desktop、Computer 与每个 Runner 都要尽快知道“可能有新东西”。实时通道会断线、重复或丢失事件，Redis Pub/Sub 也不保证送达。如果业务正确性依赖每个事件都到达，就要给事件编号、持久保存，并支持重放。

## 决策

- SSE 与 Redis Pub/Sub 只传 `InvalidationEvent` 与 `WakeEvent`：种类、主题 id 与 revision，不传业务正文（`crates/openwork-collab/src/protocol/events.rs`）。
- 收到事件的一方重新读取 PostgreSQL。Computer 每 60 秒取一次完整 desired snapshot，Runner 每 20 秒读一次 durable inbox（`process.rs` 的 `ComputerOptions`）。
- 三类 SSE 共用 `protocol::sse::reconnecting_invalidation_loop`。
- Server 按 Agent ID 分发 wake 事件（`server/redis.rs` 的 `AgentWakeHub`）。

规则见 [collaboration.md §4](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**事件重放日志。** 设计明确不建立。丢失的事件由周期重读补上。

**把所有 Agent 的事件广播给每个 Runner，由 Runner 过滤。** 没有采用。一个 Agent 的事件只送到它自己的连接。

**用每 Agent 的连接取代 management SSE。** 做不到。新 Agent 还没有 Runner 与 JWT，Computer 只能经 management SSE 得知它。

## 后果

- 丢失一个事件，最多延迟到下一次周期重读。
- 重复的事件只多一次读取，不改变结果。
- 测试：`tests/architecture.rs::all_sse_consumers_share_the_protocol_reconnect_loop`、`server::redis::tests::agent_wake_hub_does_not_fan_out_other_agents_events`、`tests/messaging.rs::per_agent_sse_is_isolated_and_the_durable_inbox_does_not_depend_on_it`。
