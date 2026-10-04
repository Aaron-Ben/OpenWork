# 消息与接口

Server 保存用户、Agent、房间与消息，提供界面、Computer 与 Agent 三组接口，并用 SSE 推送失效提示。代码在 `packages/server/`，跨进程的类型在 `packages/protocol/src/collab.ts`。进程关系见 [architecture.md](../architecture.md)。

## 1. 启动

- 需要的环境变量：`DATABASE_URL`、`CREW_RENDERER_ORIGIN`、`CREW_MIGRATIONS_DIR`。缺少任何一个时启动失败。
- 启动顺序：执行 `CREW_MIGRATIONS_DIR` 中的迁移，确保本机用户存在，在 `127.0.0.1` 的随机端口上监听，然后向 stdout 写 ready。
- stdin 关闭或收到 SIGTERM 时关闭：先结束全部 SSE 连接，再关闭 HTTP 服务；其余正在进行的请求最多等 2 秒，小于主进程给的 3 秒宽限（`packages/server/src/serve.ts`）。

## 2. 数据模型

| 表 | 内容 | 约束 |
|---|---|---|
| `users` | 本机用户 | 只有一行，名字是 “User” |
| `agents` | 名字、人设、Engine（`opencode`）、模型 | |
| `rooms` | 房间；`kind` 只有 `direct`；`next_seq` 是下一个序号 | `direct_key` 唯一：每个用户与 Agent 之间只有一个私聊房间 |
| `room_users`、`room_agents` | 房间成员 | |
| `messages` | 房间、序号、作者、正文、时间 | 作者恰好是用户或 Agent 之一；`(room_id, seq)` 唯一；正文去掉空白后不能为空 |
| `agent_read_cursors` | 每个 Agent 在每个房间读到的序号，初始为 0 | |

- ID 都是数据库生成的 UUID，代码中用 branded 类型（`packages/protocol/src/ids.ts`）。
- 表结构由 `packages/server/src/db/schema.ts` 定义，迁移由 drizzle-kit 生成在 `packages/server/drizzle/`。

理由见 [私聊的数据模型](../../.agents/notes/implemented/architecture/2026-10-04-direct-chat-data-model.md)。

## 3. Agent 与房间

- 新建 Agent 时，在一个事务里写入 Agent、它与用户的私聊房间、两边的成员关系与已读位置。
- 名字去掉首尾空白后 1 到 40 字符，人设 1 到 4,000 字符，模型不能为空。
- 新建后通知界面与 Computer：Agent 列表变了。

## 4. 消息

- 写入一条消息是一个事务：确认作者是房间成员，执行 `UPDATE rooms SET next_seq = next_seq + 1 ... RETURNING` 取得序号（同时锁住房间行），写入消息。并发写入时序号连续、没有空洞，且与提交顺序一致。
- 正文去掉首尾空白后不能为空，最多 20,000 字符。
- 房间不存在时返回 404，作者不是成员时返回 403。
- 提交之后通知：界面收到“这个房间有新消息”；房间里除作者以外的 Agent 收到“唤醒”。Agent 自己的回复不会唤醒它自己。

## 5. 未读消息与已读位置

- `GET /computer/agents/:agentId/inbox` 按房间返回已读位置之后的消息。
- `POST /computer/agents/:agentId/inbox/ack` 把已读位置推进到给定序号。已读位置只前进（`GREATEST`）；序号超过房间已有的消息时返回 400。

## 6. 运行期状态

以下状态只存在 Server 内存中，应用重启后清空（`packages/server/src/state.ts`）：

- **Agent 凭证：** Computer 为每个 Agent 申请一个随机凭证。再次申请时，旧凭证立即失效。
- **Agent 状态：** `idle`（默认）、`working`、`error`（带原因），由 Computer 上报。变化时通知界面。
- **可用模型：** Computer 上报的模型列表。上报时通知界面。

## 7. 接口

| 接口 | 凭证 | 作用 |
|---|---|---|
| `GET /desktop/agents` | Desktop | Agent 列表，含房间与状态 |
| `POST /desktop/agents` | Desktop | 新建 Agent |
| `GET /desktop/rooms/:roomId/messages` | Desktop | 房间的全部消息，按序号排列 |
| `POST /desktop/rooms/:roomId/messages` | Desktop | 以用户身份发消息 |
| `GET /desktop/models` | Desktop | 可用模型列表 |
| `GET /desktop/events` | Desktop | SSE：`room.messages`、`agents`、`models` |
| `POST /computer/connect` | Computer | 确认地址与凭证可用 |
| `GET /computer/agents` | Computer | Agent 列表，含私聊房间 |
| `GET /computer/agents/:agentId/inbox` | Computer | 未读消息 |
| `POST /computer/agents/:agentId/inbox/ack` | Computer | 推进已读位置 |
| `POST /computer/agents/:agentId/token` | Computer | 签发 Agent 凭证 |
| `POST /computer/agents/:agentId/status` | Computer | 上报 Agent 状态 |
| `POST /computer/models` | Computer | 上报可用模型 |
| `GET /computer/events` | Computer | SSE：`agent.wake`、`agents` |
| `POST /agent/reply` | Agent | 以凭证对应的 Agent 身份在房间里回复 |

- 凭证用 `Authorization: Bearer`。缺少或不对时返回 401 与 `{ "error": "凭证无效" }`。
- 错误响应一律是 `{ "error": 原因 }`。请求校验失败时返回 400 与第一条校验错误；请求体不是合法 JSON 时返回 400；请求体超过 1 MB 时返回 413；没有匹配的接口时返回 404；未预料的错误返回 500 与 “Server 内部错误”。
- 接口的方法、路径、参数与响应由 `packages/protocol/src/api.ts` 的契约定义，Server 用 `route()` 按契约注册（`packages/server/src/http.ts`）。
- 只有界面的来源（`CREW_RENDERER_ORIGIN`）可以跨域调用 `/desktop/*`。
- SSE 每 15 秒发一行注释保持连接。断线期间的提示不补发，客户端重连后重新读取全部数据。重连从 1 秒开始指数退避，最长 30 秒（`packages/protocol/src/sse.ts`）。
- SSE 响应带 `Connection: close`：流结束时连接一起关闭，不以 keep-alive 的形式留着拖住关闭。

理由见 [Express 5 与 protocol 的接口契约](../../.agents/notes/implemented/architecture/2026-10-04-express-api-contract.md) 与 [SSE 只传失效提示与 Agent 唤醒](../../.agents/notes/implemented/architecture/2026-10-04-sse-invalidation-and-wake.md)。

## 8. 验收

| 条目 | 测试 |
|---|---|
| 迁移建出全部表；本机用户只有一个 | `packages/server/test/db.test.ts` 的 `migrations`、`ensureLocalUser` |
| 消息恰好一个作者、序号在房间内唯一、每对只有一个私聊房间、作者必须存在、正文不能为空 | `db.test.ts` 的 `constraints` |
| 新建 Agent 时同时建房间，状态为空闲，并通知界面与 Computer | `packages/server/test/api.test.ts` 的 `agents` |
| 序号连续，并发写入时没有空洞 | `api.test.ts` 的 `messages` |
| 用户的消息唤醒 Agent；Agent 的回复不唤醒自己 | `api.test.ts` 的 `messages`、`agent replies` |
| 未读消息从已读位置之后开始，已读位置只前进 | `api.test.ts` 的 `inbox` |
| Agent 不能在非成员的房间回复；换发凭证后旧凭证失效 | `api.test.ts` 的 `agent replies` |
| 状态与模型上报后通知界面 | `api.test.ts` 的 `status and models` |
| 三类凭证互不通用；CORS 只允许界面来源；不合法的请求体、过大的请求体与不存在的接口都返回 JSON 错误 | `packages/server/test/app.test.ts` |
| SSE 能被共用的读取器读到，中止后干净结束；Server 关闭通道时 SSE 立即结束 | `api.test.ts` 的 `events over SSE` |
| 界面正在读 SSE 时，Server 也能在 1 秒内关闭 | `packages/server/test/serve.test.ts` |
