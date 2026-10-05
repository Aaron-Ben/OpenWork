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
| `rooms` | 房间；`kind` 是 `direct`、`group` 或 `thread`；群聊有名字；`next_seq` 是最新一条的序号；讨论串有所在的群聊（`parent_room_id`）与挂着的消息（`parent_message_id`） | `direct_key` 唯一：每个用户与 Agent 之间只有一个私聊房间；群聊必须有名字；讨论串有且只有讨论串有两个父字段；每条消息最多一个讨论串 |
| `room_users`、`room_agents` | 房间成员 | |
| `messages` | 房间、序号、作者、类型（`kind`：聊天 `text` 或通知 `system`）、正文、时间；Agent 的消息还有所在的一轮（`run_id`）与发出前被 HELD 拦下的次数（`held_before`） | 作者恰好是用户或 Agent 之一，通知的作者是做这件事的人；`(room_id, seq)` 唯一；正文去掉空白后不能为空 |
| `tasks` | 任务：房间、房间内的编号、标题、状态（`todo`、`in_progress`、`in_review`、`done`、`closed`）、负责人（Agent）、创建者、宿主消息、领取与完成时间 | `(room_id, number)` 唯一；每条消息最多一个任务；创建者恰好一个；进行中与待审一定有负责人 |
| `message_mentions` | 消息 @ 到的 Agent | 只记录写入时是房间成员的 Agent |
| `agent_read_cursors` | 每个 Agent 在每个房间的已读位置（`last_read_seq`）与已投递位置（`delivered_seq`）；在讨论串里有一行就是关注了它 | 已投递位置不小于已读位置 |
| `user_read_cursors` | 用户在每个房间读到的序号，用来算未读数 | |
| `runs` | 运行记录：Agent 的一轮，结果（`running`、`succeeded`、`failed`、`cancelled`、`interrupted`）、错误、完整输入、起止时间、token 与费用合计、步数、回复数、HELD 次数 | 每个 Agent 最多一轮 `running`；`failed` 必须有错误；只有 `running` 没有结束时间 |
| `run_triggers` | 一轮被哪个房间的哪几条消息唤醒 | |
| `run_events` | 一轮里的每一步：`step`、`tool`、`text`、`step_end`、`reply`、`held`，序号在一轮内递增 | |

- ID 都是数据库生成的 UUID，代码中用 branded 类型（`packages/protocol/src/ids.ts`）。
- 表结构由 `packages/server/src/db/schema.ts` 定义，迁移由 drizzle-kit 生成在 `packages/server/drizzle/`。

理由见 [私聊的数据模型](../../.agents/notes/implemented/architecture/2026-10-04-direct-chat-data-model.md)、[群聊](../../.agents/notes/implemented/feature/2026-10-05-group-chat.md)、[运行观测](../../.agents/notes/implemented/feature/2026-10-05-run-observability.md)、[讨论串](../../.agents/notes/implemented/feature/2026-10-05-threads.md) 与 [任务](../../.agents/notes/implemented/feature/2026-10-05-tasks.md)。

## 3. Agent 与房间

- 新建 Agent 时，在一个事务里写入 Agent、它与用户的私聊房间、两边的成员关系与已读位置。
- 名字去掉首尾空白后 1 到 40 字符，handle 格式见上表，人设 1 到 4,000 字符，模型不能为空。handle 已被使用时返回 409。
- 新建后通知界面与 Computer：Agent 列表变了。
- 新建群聊：名字去掉首尾空白后 1 到 40 字符，至少一个 Agent，不能重复。成员是本机用户与选中的 Agent。
- 加成员：只能加到群聊，私聊的房间 ID 返回 404。已经在群里的 Agent 不变。新成员的已读与已投递位置从加入时的最新序号开始，看不到加入前的消息；加成员时锁住房间行，与写消息互斥。
- 新建群聊与加成员后通知界面：群聊列表变了（`rooms`）。
- 新建私聊或群聊时，用户的已读位置从 0 开始；迁移 `0002_user_read_cursors.sql` 把已有房间的已读位置设为当时的最新序号。

## 4. 消息

- 写入一条消息是一个事务（`postMessage`，`packages/server/src/messages.ts`）：用 `SELECT … FOR UPDATE` 锁住房间行，确认作者是房间成员；作者是 Agent 时做 HELD 检查（第 6 节）；把 `next_seq` 加一作为序号，写入消息与它 @ 到的 Agent。并发写入时序号连续、没有空洞，且与提交顺序一致。
- 正文去掉首尾空白后不能为空，最多 20,000 字符。
- 房间不存在时返回 404，作者不是成员时返回 403。
- @：正文里的 `@handle`，大小写不敏感；代码块与行内代码里的不算，前面紧挨字母、数字或 `_ . @ / -` 的不算（邮箱、路径）；只认房间里的 Agent 成员（`packages/server/src/mentions.ts`）。
- 提交之后通知界面：这个房间有新消息。唤醒哪些 Agent（`wakeTargets`）：
  - 用户的消息：房间里全部 Agent 成员。
  - Agent 的消息：它 @ 到的其他成员。没有 @ 时不唤醒任何 Agent，Agent 不会唤醒自己。
- 读取消息：`GET /desktop/rooms/:roomId/messages` 按序号从旧到新返回一段。`after=n` 取序号大于 n 的最早一批；否则取 `before=n` 之前（没有 `before` 时是全部）最新的一批。`limit` 是 1 到 200，默认 200。`after` 与 `before` 不能同时使用。

### 讨论串

- 讨论串是群聊里一条消息下的房间（`kind = 'thread'`）。回复是这个房间的普通消息，序号、HELD、已读位置都按它自己算，不进群聊的时间线。
- 写消息时带 `threadOf`（那条消息的 ID）就发到它的讨论串，讨论串还没有时在同一个事务里创建（`openThread`）。私聊里不能开，讨论串里不能再开，返回 400；消息不在这个房间返回 404。原因文字是 protocol 的 `THREAD_REFUSALS`。
- 创建时：群聊里的用户从 0 开始读；消息的作者是 Agent 时，它关注讨论串。
- 讨论串没有成员行，谁能发言、@ 谁算数都看所在群聊的成员。
- 关注者就是在讨论串里有已读位置的 Agent，已读位置从 0 开始，所以新关注者能读到已有的回复。在讨论串里发言或被 @ 到时关注。
- 唤醒：讨论串里用户的消息唤醒全部关注者，不唤醒群里的其他 Agent；还没有 Agent 关注时，群里的 Agent 全部关注并被唤醒。Agent 的消息与群聊一样，只唤醒它 @ 到的其他成员。
- 写入后通知界面讨论串与所在群聊各有新消息：群聊里的讨论串摘要变了。
- `GET /desktop/rooms/:roomId/threads`：群聊里的全部讨论串，按创建先后排列：挂着的消息、回复数、最后回复时间、发过言的人（至多 5 个）与用户的未读回复数。

### 任务

代码在 `packages/server/src/tasks.ts`，状态与流转表在 `packages/protocol/src/tasks.ts`。

- 每个任务都有宿主消息。新建任务：以创建者的身份在房间里发一条正文为标题的消息，再把它变成任务；分配了负责人时这条消息不唤醒任何 Agent。转成任务：标题取消息正文的第一个非空行，至多 200 字符。讨论串里的消息与通知不能转，一条消息只能转一次。
- 编号在房间内递增；新建与转换时锁住房间行，编号不重复。给的房间是讨论串时，换成它所在的群聊。
- 每次改动与它的通知写在同一个事务里。通知是 `kind = 'system'` 的消息，作者是做这件事的人：群聊里发到宿主消息的讨论串（还没有时创建），私聊没有讨论串，发到时间线。通知不做 HELD 检查，只唤醒它 @ 到的 Agent。
- 通知只在两种时候 @ 负责人：分配给它；别人把它的任务退回（待审或完成退回到进行中或待办）。被 @ 的负责人关注讨论串并被唤醒。
- 领取：带条件的更新，只有待办、并且没有负责人或负责人就是自己时成功，负责人设成自己、状态改成进行中；失败时写明原因，例如已经由谁负责。
- 改状态：按流转表检查；进行中与待审要有负责人；带条件地更新（状态仍是读到的那个），完成时记下完成时间。
- 换负责人：只换人；取消负责人时，进行中或待审的任务回到待办；完成或关闭的任务不再分配；负责人必须是房间里的 Agent。
- 拒绝时返回 404 或 409，正文带 `refusal`（原因的代码与数据），`crew` 据此写英文说明。
- 收件箱里的宿主消息带任务的编号、状态与负责人（`task`），讨论串挂着的消息也带。

## 5. 用户的会话列表与未读数

- `GET /desktop/conversations`：用户所在的全部房间，按最后活动（最后一条消息的时间，没有消息时是房间的创建时间）从新到旧排列。每项有类型、名字（群聊的名字，私聊是 Agent 的名字）、Agent 成员、最后一条消息（正文截到 200 字符）与未读数（`packages/server/src/conversations.ts`）。
- 未读数：用户已读位置之后、不是用户自己发的消息条数，加上这个群聊的讨论串里同样算出的条数。最后活动也算上讨论串里的消息。讨论串不单独出现在列表里。
- `POST /desktop/rooms/:roomId/read`：把用户的已读位置推进到 `seq`。只前进；超过房间已有的序号时停在最新一条。用户不在这个房间时返回 404。
- 界面在房间打开、窗口在前台时调用它：打开房间时一次，之后每来新消息、窗口回到前台时再一次。

## 6. Agent 的未读消息、已投递位置与 HELD

- `POST /computer/agents/:agentId/inbox` 按房间返回已读位置之后的消息，并把每个房间的已投递位置推进到本次返回的最后一条。每个房间附上类型、群聊名字与成员（用户在前，Agent 按名字排列）；讨论串附上所在群聊的名字与成员，以及挂着的那条消息（`parent`）；每条消息标出是否 @ 了这个 Agent。
- `POST /computer/agents/:agentId/inbox/ack`：每个房间的已读位置推进到已投递位置。Computer 在 Turn 成功后调用；失败时不调用，下次读取 inbox 仍从已读位置开始。
- HELD：Agent 回复时，房间里有已投递位置之后、别人（用户或其他 Agent）发的消息，回复就不写入。响应从最早的开始返回至多 20 条与之后还没返回的条数，已投递位置推进到返回的最后一条（全部返回时推进到最新）。没有返回的消息仍算没看过：Agent 再次回复时接着返回，Turn 结束后它们也仍是未读。私聊同样适用。
- Agent 的回复写入后，它的已投递位置推进到这条回复：之前的消息不是看过的就是它自己发的。
- `POST /agent/reply` 总是返回 200：`{ outcome: "posted", id, roomId, seq }` 或 `{ outcome: "held", roomId, newMessages, omitted }`。`roomId` 是消息实际所在的房间：带 `threadOf` 时是讨论串。HELD 时不通知界面，也不唤醒任何 Agent。第一次在讨论串里发言的 Agent 已读位置从 0 开始，所以讨论串里已有回复时，它的第一条回复会先被拦下。

## 7. 运行记录与 Agent 状态

运行记录的代码在 `packages/server/src/runs.ts`：

- **开始一轮：** Computer 登记被唤醒的消息（每个房间的起止序号）与完整输入，拿到 run ID。这个 Agent 还有没结束的一轮时，先把它标为 `interrupted`。
- **上报事件：** Computer 按顺序追加 Engine 事件；`step_end` 带的用量累加到这一轮，`step` 计入步数。工具的输入输出与模型文字每段至多 4,096 字符。写入前把 NUL 与落单的 UTF-16 代理项换成 U+FFFD：PostgreSQL 的 text 与 jsonb 不接受它们（`storableText`）。
- **结束：** 写结果与错误。已经结束的一轮再上报事件或结果时返回 409，不存在时返回 404。
- **回复与 HELD：** `POST /agent/reply` 到达时，Server 把“发出”或“被拦下”写进这个 Agent 正在跑的那一轮，并在发出的消息上记下 run ID 与这一轮里、这个房间、上一条回复之后被 HELD 拦下的次数。不在任何一轮里时不记。记录失败只写日志，`crew reply` 照常返回发出或被拦下的结果。
- **中断：** `POST /computer/connect` 把全部 `running` 的轮次标为 `interrupted`：上一个 Computer 不会再写结果。
- 每一步写入后向界面发 `run.activity`（带 run ID 与涉及的房间）；开始与结束时另发 `agents`。
- 讨论串唤醒的一轮：记录里的唤醒来源带上所在的群聊（`parentRoomId`）；按群聊列运行记录时包括它的讨论串里的轮次；涉及的房间同时算上讨论串与所在的群聊。

Agent 的状态由运行记录推出（`agentStatuses`），不单独保存：

- 有 `running` 的一轮：`working`，带 run ID 与这一轮涉及的房间；
- 否则 Computer 报告了它跑不起来（沙箱不可用、目录不安全）：`error`，`roomIds` 为空，表示不限于某个房间；
- 否则最近结束的一轮是 `failed`：`error`，带错误与那一轮的房间；
- 其余是 `idle`。

以下状态只存在 Server 内存中，应用重启后清空（`packages/server/src/state.ts`）：

- **Agent 凭证：** Computer 为每个 Agent 申请一个随机凭证。再次申请时，旧凭证立即失效。
- **Agent 跑不起来的原因：** 由 Computer 报告或清除。变化时通知界面。
- **可用模型：** Computer 上报的模型列表。上报时通知界面。

## 8. 接口

| 接口 | 凭证 | 作用 |
|---|---|---|
| `GET /desktop/agents` | Desktop | Agent 列表，含房间与状态 |
| `POST /desktop/agents` | Desktop | 新建 Agent |
| `GET /desktop/rooms/:roomId/messages` | Desktop | 房间的一段消息，查询参数见第 4 节 |
| `POST /desktop/rooms/:roomId/messages` | Desktop | 以用户身份发消息；带 `threadOf` 时发到那条消息的讨论串 |
| `GET /desktop/rooms/:roomId/threads` | Desktop | 群聊里的讨论串摘要 |
| `GET /desktop/rooms/:roomId/tasks` | Desktop | 房间里的任务 |
| `POST /desktop/rooms/:roomId/tasks` | Desktop | 新建任务 |
| `POST /desktop/rooms/:roomId/tasks/convert` | Desktop | 把消息转成任务 |
| `POST /desktop/rooms/:roomId/tasks/:number/status` | Desktop | 改任务的状态 |
| `POST /desktop/rooms/:roomId/tasks/:number/assignee` | Desktop | 换负责人或取消负责人 |
| `GET /desktop/conversations` | Desktop | 会话列表：最后一条消息与未读数 |
| `POST /desktop/rooms/:roomId/read` | Desktop | 推进用户的已读位置 |
| `GET /desktop/groups` | Desktop | 群聊列表，含成员 |
| `GET /desktop/runs` | Desktop | 运行记录，从新到旧，至多 100 轮；`roomId` 或 `agentId` 筛选 |
| `GET /desktop/runs/:runId` | Desktop | 一轮的概要、完整输入与每一步 |
| `POST /desktop/groups` | Desktop | 新建群聊 |
| `POST /desktop/groups/:roomId/members` | Desktop | 把 Agent 加进群聊 |
| `GET /desktop/models` | Desktop | 可用模型列表 |
| `GET /desktop/events` | Desktop | SSE：`room.messages`、`agents`、`rooms`、`run.activity`、`models` |
| `POST /computer/connect` | Computer | 确认地址与凭证可用；把没结束的轮次标为中断 |
| `GET /computer/agents` | Computer | Agent 列表，含私聊房间 |
| `POST /computer/agents/:agentId/inbox` | Computer | 取出未读消息，记为已投递 |
| `POST /computer/agents/:agentId/inbox/ack` | Computer | 已读位置推进到已投递位置 |
| `POST /computer/agents/:agentId/token` | Computer | 签发 Agent 凭证 |
| `POST /computer/agents/:agentId/problem` | Computer | 报告或清除 Agent 跑不起来的原因 |
| `POST /computer/agents/:agentId/runs` | Computer | 开始一轮 |
| `POST /computer/runs/:runId/events` | Computer | 追加 Engine 事件 |
| `POST /computer/runs/:runId/finish` | Computer | 一轮结束 |
| `POST /computer/models` | Computer | 上报可用模型 |
| `GET /computer/events` | Computer | SSE：`agent.wake`、`agents` |
| `POST /agent/reply` | Agent | 以凭证对应的 Agent 身份在房间里回复（带 `threadOf` 时在讨论串里），或被 HELD 拦下 |
| `POST /agent/tasks/list`、`create`、`convert`、`claim`、`status`、`assign` | Agent | 以凭证对应的 Agent 身份操作任务 |

- 凭证用 `Authorization: Bearer`。缺少或不对时返回 401 与 `{ "error": "凭证无效" }`。
- 错误响应一律是 `{ "error": 原因 }`。请求校验失败时返回 400 与第一条校验错误；请求体不是合法 JSON 时返回 400；请求体超过 1 MB 时返回 413；没有匹配的接口时返回 404；未预料的错误返回 500 与 “Server 内部错误”。
- 接口的方法、路径、参数与响应由 `packages/protocol/src/api.ts` 的契约定义，Server 用 `route()` 按契约注册（`packages/server/src/http.ts`）。
- 只有界面的来源（`CREW_RENDERER_ORIGIN`）可以跨域调用 `/desktop/*`。
- SSE 每 15 秒发一行注释保持连接。断线期间的提示不补发，客户端重连后重新读取全部数据。重连从 1 秒开始指数退避，最长 30 秒（`packages/protocol/src/sse.ts`）。
- SSE 响应带 `Connection: close`：流结束时连接一起关闭，不以 keep-alive 的形式留着拖住关闭。

理由见 [Express 5 与 protocol 的接口契约](../../.agents/notes/implemented/architecture/2026-10-04-express-api-contract.md) 与 [SSE 只传失效提示与 Agent 唤醒](../../.agents/notes/implemented/architecture/2026-10-04-sse-invalidation-and-wake.md)。

## 9. 验收

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
| 会话列表按最后活动排列，带最后一条消息；未读数只算别人的消息，标记已读后减少，已读位置只前进 | `api.test.ts` 的 `conversations` |
| 已有房间的历史消息在迁移后不算未读 | 手动：2026-10-05 在临时数据库上执行到 `0001`、插入房间，再执行 `0002_user_read_cursors.sql`，已读位置等于房间的最新序号 |
| HELD：有没看到的新消息时不写入并返回它们，从最早的开始一次最多 20 条，其余的下次回复时返回、确认已读后仍是未读；看过之后能发出；被 HELD 返回过的消息确认后不再出现 | `api.test.ts` 的 `agent replies` |
| Agent 不能在非成员的房间回复；换发凭证后旧凭证失效 | `api.test.ts` 的 `agent replies` |
| 运行记录：状态由运行记录推出并只属于这一轮的房间；事件有序、用量累加；回复与 HELD 记进这一轮并标在消息上；失败显示为出错直到下一轮成功；已结束的一轮拒绝上报；连上时中断旧轮次；同一 Agent 最多一轮在跑；按房间或 Agent 列出；Computer 报告的问题在每个房间显示为出错 | `api.test.ts` 的 `runs`；约束见 `db.test.ts` 的 `constraints` |
| 模型上报后通知界面 | `api.test.ts` 的 `models` |
| 三类凭证互不通用；CORS 只允许界面来源；不合法的请求体、过大的请求体与不存在的接口都返回 JSON 错误 | `packages/server/test/app.test.ts` |
| SSE 能被共用的读取器读到，中止后干净结束；Server 关闭通道时 SSE 立即结束 | `api.test.ts` 的 `events over SSE` |
| 界面正在读 SSE 时，Server 也能在 1 秒内关闭 | `packages/server/test/serve.test.ts` |
| 讨论串：在消息下开出并复用、不进群聊时间线、摘要；关注者与唤醒规则；一个 Agent 关注多个讨论串时各自的已读位置与分段；拒绝私聊、嵌套与别的房间的消息；收件箱附上所在群聊与挂着的消息；新关注者的第一条回复被 HELD；未读算进群聊；运行记录按群聊列出 | `api.test.ts` 的 `threads` |
| 任务：编号、宿主消息与讨论串里的通知、私聊的通知在时间线；转成任务的标题与拒绝；两个 Agent 同时领取只有一个成功；流转表与负责人；只在分配与退回时唤醒负责人；Agent 不被自己的通知拦下、用户不把自己的通知算作未读；收件箱里的任务后缀；换负责人 | `api.test.ts` 的 `tasks` |
