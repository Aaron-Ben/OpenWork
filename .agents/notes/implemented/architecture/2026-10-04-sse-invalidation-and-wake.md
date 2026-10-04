# Agent Note: SSE 只传失效提示与 Agent 唤醒

Status: implemented

## 问题

消息写入后，界面要显示新消息，Computer 要唤醒对应的 Agent。需要决定推送什么内容、用什么通道、断线时怎样不丢消息，以及界面怎样管理来自 Server 的数据。

## 决策

- SSE 只传失效提示（“某部分数据变了”），不传业务数据；收到提示的一方重新读取。事件的 schema 在 `packages/protocol/src/collab.ts`：
  - `/desktop/events` 只发三类提示：某房间有新消息、Agent 列表或状态变了、Computer 上报了新的模型列表。
  - `/computer/events` 发两类：某个 Agent 可能有新消息、Agent 列表变了。
- Computer 整体建立一条 SSE 连接。它用 `POST /computer/agents/:agentId/inbox` 取已读位置之后的消息，Turn 成功后用 `POST /computer/agents/:agentId/inbox/ack` 推进已读位置（第 3a 步起推进到已投递位置，见 [群聊](../feature/2026-10-05-group-chat.md)）。Turn 失败时已读位置不动，下次唤醒重新处理。
- 每次连接或重连成功后，Computer 同步 Agent 列表，再唤醒全部 Runner，补上断线期间丢失的事件。不做定时轮询。
- Server 内部用进程内事件 `EventHub`（`packages/server/src/events.ts`）把“数据变了”传给 SSE 连接。推送代码集中在它与 `packages/server/src/http.ts` 的 `eventStream`，以后改用 WebSocket 或 Redis pub/sub 时只改这两处。
- SSE 的解析用 eventsource-parser，重连循环自己写，按指数退避，放在 `packages/protocol/src/sse.ts`，Computer 与界面共用。
- 界面的数据用 TanStack Query 获取与缓存。收到提示时用 `invalidateQueries` 让对应的数据重新获取；“房间有新消息”例外，只取缓存之后的消息合并进去（`fetchNewer`，见 [群聊](../feature/2026-10-05-group-chat.md)）。每次连接成功都让全部缓存失效。缓存不按时间过期（`apps/desktop/src/lib/events.ts`、`apps/desktop/src/lib/queries.ts`）。

接口见 [messaging.md](../../../../docs/subsystems/messaging.md) 第 8 节，Computer 一侧见 [agent-runtime.md](../../../../docs/subsystems/agent-runtime.md) 第 1 节。

## 考虑过的方案

**每个 Agent 一条 SSE 连接。** Rust 版与 cumora（`cumora:server/src/agents/runtime/server.ts` 的 `/wake-stream`）这样做，因为它们的每个 Agent 是独立进程或容器，持有自己的凭证。没有采用：Crew 只有一个 Computer 进程管理全部 Agent。

**用 WebSocket 推送消息正文并要求确认。** raft 的 `agent:deliver` 与 `agent:deliver:ack`（`raft:packages/shared/src/index.ts`）这样做。raft 的 Server 在云端，要向用户机器发送十几种命令并等待回答，需要双向通道。没有采用：Crew 的 Server 只需要向 Computer 与界面单向发出“有新东西”的提示，推送正文还要自己实现确认与重发。Server 需要向客户端提问并等待回答时，再改用 WebSocket。

**用 Socket.IO。** 自带重连、房间与确认。没有采用：它有自己的协议，前后端都必须使用它；单向的失效提示用 SSE 就够了。

**唤醒一开始就走 Redis pub/sub。** 改为多实例时不用再改。没有采用：现在只有一个 Server 进程，进程内事件更简单；唤醒代码集中在一个模块里，以后替换成本低。

**界面数据用 zustand store 手动管理。** raft 与 cumora（`cumora:src/stores/messages.ts` 的 `applyEvent`）把推送来的消息正文合并进 zustand store，每种事件都要写合并逻辑。没有采用：Crew 只推送失效提示，TanStack Query 的“失效后重新获取”正好对应。

## 后果

- 丢一个事件的代价只是晚一点刷新：重连后重新读取完整状态，不需要事件重放。
- 失败的 Turn 不推进已读位置，消息可能被处理两次，但不会丢。
- 只适用于单个 Server 进程。
- Server 迁到云端后，反向代理默认缓冲响应会推迟 SSE 事件，需要关闭缓冲。
