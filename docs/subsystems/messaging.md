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
| `agents` | 名字、handle、人设、Engine（`opencode`）、模型 | handle 唯一，格式是小写字母、数字与 `-`，以字母或数字开头，最多 32 字符 |
| `rooms` | 房间；`kind` 是 `direct` 或 `group`；群聊有名字；`next_seq` 是最新一条的序号 | `direct_key` 唯一：每个用户与 Agent 之间只有一个私聊房间；群聊必须有名字 |
| `room_users`、`room_agents` | 房间成员 | |
| `messages` | 房间、序号、作者、正文、时间 | 作者恰好是用户或 Agent 之一；`(room_id, seq)` 唯一；正文去掉空白后不能为空 |
| `message_mentions` | 消息 @ 到的 Agent | 只记录写入时是房间成员的 Agent |
| `agent_read_cursors` | 每个 Agent 在每个房间的已读位置（`last_read_seq`）与已投递位置（`delivered_seq`） | 已投递位置不小于已读位置 |

- ID 都是数据库生成的 UUID，代码中用 branded 类型（`packages/protocol/src/ids.ts`）。
- 表结构由 `packages/server/src/db/schema.ts` 定义，迁移由 drizzle-kit 生成在 `packages/server/drizzle/`。

理由见 [私聊的数据模型](../../.agents/notes/implemented/architecture/2026-10-04-direct-chat-data-model.md) 与 [群聊](../../.agents/notes/implemented/feature/2026-10-05-group-chat.md)。

## 3. Agent 与房间

- 新建 Agent 时，在一个事务里写入 Agent、它与用户的私聊房间、两边的成员关系与已读位置。
- 名字去掉首尾空白后 1 到 40 字符，handle 格式见上表，人设 1 到 4,000 字符，模型不能为空。handle 已被使用时返回 409。
- 新建后通知界面与 Computer：Agent 列表变了。
- 新建群聊：名字去掉首尾空白后 1 到 40 字符，至少一个 Agent，不能重复。成员是本机用户与选中的 Agent。
- 加成员：只能加到群聊，私聊的房间 ID 返回 404。已经在群里的 Agent 不变。新成员的已读与已投递位置从加入时的最新序号开始，看不到加入前的消息；加成员时锁住房间行，与写消息互斥。
- 新建群聊与加成员后通知界面：群聊列表变了（`rooms`）。

## 4. 消息

- 写入一条消息是一个事务（`postMessage`，`packages/server/src/messages.ts`）：用 `SELECT … FOR UPDATE` 锁住房间行，确认作者是房间成员；作者是 Agent 时做 HELD 检查（第 5 节）；把 `next_seq` 加一作为序号，写入消息与它 @ 到的 Agent。并发写入时序号连续、没有空洞，且与提交顺序一致。
- 正文去掉首尾空白后不能为空，最多 20,000 字符。
- 房间不存在时返回 404，作者不是成员时返回 403。
- @：正文里的 `@handle`，大小写不敏感；代码块与行内代码里的不算，前面紧挨字母、数字或 `_ . @ / -` 的不算（邮箱、路径）；只认房间里的 Agent 成员（`packages/server/src/mentions.ts`）。
- 提交之后通知界面：这个房间有新消息。唤醒哪些 Agent（`wakeTargets`）：
  - 用户的消息：房间里全部 Agent 成员。
  - Agent 的消息：它 @ 到的其他成员。没有 @ 时不唤醒任何 Agent，Agent 不会唤醒自己。
- 读取消息：`GET /desktop/rooms/:roomId/messages` 按序号从旧到新返回一段。`after=n` 取序号大于 n 的最早一批；否则取 `before=n` 之前（没有 `before` 时是全部）最新的一批。`limit` 是 1 到 200，默认 200。`after` 与 `before` 不能同时使用。

## 5. 未读消息、已投递位置与 HELD

- `POST /computer/agents/:agentId/inbox` 按房间返回已读位置之后的消息，并把每个房间的已投递位置推进到本次返回的最后一条。每个房间附上类型、群聊名字与成员（用户在前，Agent 按名字排列）；每条消息标出是否 @ 了这个 Agent。
- `POST /computer/agents/:agentId/inbox/ack`：每个房间的已读位置推进到已投递位置。Computer 在 Turn 成功后调用；失败时不调用，下次读取 inbox 仍从已读位置开始。
- HELD：Agent 回复时，房间里有已投递位置之后、别人（用户或其他 Agent）发的消息，回复就不写入。响应从最早的开始返回至多 20 条与之后还没返回的条数，已投递位置推进到返回的最后一条（全部返回时推进到最新）。没有返回的消息仍算没看过：Agent 再次回复时接着返回，Turn 结束后它们也仍是未读。私聊同样适用。
- Agent 的回复写入后，它的已投递位置推进到这条回复：之前的消息不是看过的就是它自己发的。
- `POST /agent/reply` 总是返回 200：`{ outcome: "posted", id, seq }` 或 `{ outcome: "held", newMessages, omitted }`。HELD 时不通知界面，也不唤醒任何 Agent。

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
| `GET /desktop/rooms/:roomId/messages` | Desktop | 房间的一段消息，查询参数见第 4 节 |
| `POST /desktop/rooms/:roomId/messages` | Desktop | 以用户身份发消息 |
| `GET /desktop/groups` | Desktop | 群聊列表，含成员 |
| `POST /desktop/groups` | Desktop | 新建群聊 |
| `POST /desktop/groups/:roomId/members` | Desktop | 把 Agent 加进群聊 |
| `GET /desktop/models` | Desktop | 可用模型列表 |
| `GET /desktop/events` | Desktop | SSE：`room.messages`、`agents`、`rooms`、`models` |
| `POST /computer/connect` | Computer | 确认地址与凭证可用 |
| `GET /computer/agents` | Computer | Agent 列表，含私聊房间 |
| `POST /computer/agents/:agentId/inbox` | Computer | 取出未读消息，记为已投递 |
| `POST /computer/agents/:agentId/inbox/ack` | Computer | 已读位置推进到已投递位置 |
| `POST /computer/agents/:agentId/token` | Computer | 签发 Agent 凭证 |
| `POST /computer/agents/:agentId/status` | Computer | 上报 Agent 状态 |
| `POST /computer/models` | Computer | 上报可用模型 |
| `GET /computer/events` | Computer | SSE：`agent.wake`、`agents` |
| `POST /agent/reply` | Agent | 以凭证对应的 Agent 身份在房间里回复，或被 HELD 拦下 |

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
| 消息恰好一个作者、序号在房间内唯一、每对只有一个私聊房间、作者必须存在、正文不能为空、handle 唯一且格式正确、群聊有名字、已投递位置不落后于已读位置 | `db.test.ts` 的 `constraints` |
| 已有数据上的迁移：按名字生成 handle，已投递位置等于已读位置 | 手动：2026-10-05 在临时数据库上先执行 `0000_init.sql`、插入数据，再执行 `0001_group_chat.sql` |
| 新建 Agent 时同时建房间，状态为空闲，并通知界面与 Computer；handle 冲突返回 409 | `packages/server/test/api.test.ts` 的 `agents` |
| 新建群聊、加成员、通知界面；新成员看不到加入前的消息；不能经私聊房间加成员 | `api.test.ts` 的 `groups` |
| 序号连续，并发写入时没有空洞；按 `after`、`before`、`limit` 读取一段消息 | `api.test.ts` 的 `messages` |
| 用户的消息唤醒房间里全部 Agent；Agent 的消息只唤醒它 @ 到的成员，不唤醒自己 | `api.test.ts` 的 `messages`、`agent replies` |
| @ 的解析：大小写、代码、邮箱与路径 | `packages/server/test/mentions.test.ts` |
| 未读消息从已读位置之后开始，确认后推进到已投递位置；inbox 带名册与 @ 标记 | `api.test.ts` 的 `inbox` |
| HELD：有没看到的新消息时不写入并返回它们，从最早的开始一次最多 20 条，其余的下次回复时返回、确认已读后仍是未读；看过之后能发出；被 HELD 返回过的消息确认后不再出现 | `api.test.ts` 的 `agent replies` |
| Agent 不能在非成员的房间回复；换发凭证后旧凭证失效 | `api.test.ts` 的 `agent replies` |
| 状态与模型上报后通知界面 | `api.test.ts` 的 `status and models` |
| 三类凭证互不通用；CORS 只允许界面来源；不合法的请求体、过大的请求体与不存在的接口都返回 JSON 错误 | `packages/server/test/app.test.ts` |
| SSE 能被共用的读取器读到，中止后干净结束；Server 关闭通道时 SSE 立即结束 | `api.test.ts` 的 `events over SSE` |
| 界面正在读 SSE 时，Server 也能在 1 秒内关闭 | `packages/server/test/serve.test.ts` |
