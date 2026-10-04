# Agent Note: 群聊（第 3a 步）

Status: implemented

## 问题

第 2 步只有私聊：每个房间一个用户、一个 Agent。产品的核心是多个 Agent 在同一个房间里协作，这需要回答几个私聊里不存在的问题：

- 一条消息该唤醒房间里的哪些 Agent。全部唤醒时，Agent 会抢答；Agent 的消息又会唤醒其他 Agent，可能来回刷屏。
- Agent 写回复期间房间里又来了新消息，它发出的回复可能已经过时，或者和别人刚说的重复。
- Agent 要知道房间里有谁、怎样点名别人。
- 群聊消息多，界面每次收到提示都重新拉取整个房间的历史，代价随房间变长而增长。

路线图见 [重写的路线图](../../proposed/architecture/2026-10-04-typescript-rewrite.md)“实现顺序”第 3 步。triage 属于 3b，另写 Note。2026-10-05 的两次讨论中，用户同意了下面全部决策。

## 决策

**唤醒与发言约束**

- 用户的消息唤醒房间里全部 Agent 成员；Agent 的消息只唤醒它 @ 到的其他成员（`wakeTargets`，`packages/server/src/messages.ts`）。没人点名时，Agent 之间不会来回接话，演示时一句话能讲清楚。
- 不做 triage。Agent 的 `AGENTS.md` 加入英文的发言约束（`packages/computer/src/instructions.ts`）：被点名或明显在问自己时才回复；别人在处理的问题不插手，不复述别人的工作；不发只表示同意、收到或等待的消息。写法参考 `raft:packages/daemon/src/drivers/raftCliGuide.ts` 的 Conversation etiquette。
- 每轮输入中，群聊附上名册；消息行写出作者的 handle，@ 到本 Agent 的消息标 `[mentions you]`（`packages/computer/src/prompt.ts`）。

**handle 与 @**

- Agent 有独立的 `handle`：小写字母、数字与 `-`，全局唯一，新建时必填，不能修改。数据库约束与 protocol 的 `Handle` 是同一条规则。已有的 Agent 由迁移 `packages/server/drizzle/0001_group_chat.sql` 按名字生成。
- Server 写入消息时解析 `@handle`（`packages/server/src/mentions.ts`），只认房间里的 Agent 成员，存进 `message_mentions`。3b 的 triage 与第 5 步的任务分配都要用它。

**群聊与成员**

- 用户新建群聊并选择成员，之后可以加成员；不做移除成员、改名与删除群聊，Agent 不能建群（`packages/server/src/groups.ts`）。
- 新成员的已读与已投递位置从加入时的最新序号开始，看不到加入前的消息。这与 Rust 版一致。

**HELD**

- `agent_read_cursors` 多一列已投递位置：Computer 读取 inbox 时推进到本次返回的最后一条，Agent 的回复写入后推进到这条回复。
- Agent 回复时，房间里有已投递位置之后、别人发的消息，就不写入，从最早的开始返回一批，已投递位置只推进到返回的最后一条。新消息多于一批时，Agent 再次回复会看到下一批；没有返回的消息在 Turn 结束后仍是未读。私聊同样适用。HELD 检查与写入在同一个锁住房间行的事务里（`postMessage`）。
- 不保存草稿：`crew reply` 打印新消息，说明没有发出，退出码 1；仍然需要时 Agent 再运行一次。
- Turn 成功后，已读位置推进到已投递位置：被 HELD 返回过的消息算已读。

**界面与增量拉取**

- 侧栏分“群聊”与“同事”两组；群聊视图显示作者的 handle、成员与状态，可以加成员；新建 Agent 时填 handle，按名字给出建议。输入 `@` 没有自动补全。
- 打开房间时取最新 100 条；收到 `room.messages` 提示后用 `after` 只取新消息，合并进缓存（`apps/desktop/src/lib/queries.ts` 的 `fetchNewer`）；顶部有“加载更早的消息”。SSE 仍只传失效提示。
- 被 HELD 拦下的事件不在界面上显示，放到第 4 步“运行观测”。

行为、数字与验收见 [messaging.md](../../../../docs/subsystems/messaging.md) 第 3 至 5 节与 [agent-runtime.md](../../../../docs/subsystems/agent-runtime.md) 第 2、6 节。

## 考虑过的方案

**先做 triage，再做群聊。** 见路线图“考虑过的方案”。

**任何消息都唤醒房间里全部 Agent。** raft 的做法：除了发送者本人与静音的成员，全部收到，由提示词约束何时插话，没有轮数上限（`raft:packages/server/src/services/messageService.ts` 的 `broadcastAndDeliver`）。没有采用：raft 的 Engine 大多常驻，被唤醒后停下的成本低；Crew 每个 Turn 都启动一次 OpenCode，把完整提示词发给主模型，Agent 互相唤醒可能停不下来。

**由 Agent 报告自己读到哪里。** raft 的 `crew reply` 对应命令带上 `seenUpToSeq`，由客户端记录（`raft:packages/cli/src/commands/message/send.ts`），因为它的 Agent 有好几条读消息的命令。没有采用：Crew 只有“每轮推送”这一条入口，Server 记录更简单，Agent 也无法报错位置。

**被拦下的回复保存为草稿。** raft 保存草稿并提供 `--send-draft` 与 `--anyway`，省掉重新输入长正文，同一条回复被拦 3 次后建议强制发送。没有采用：Crew 的代价只是模型多输出一遍正文。观察到房间太忙、一直发不出去时再加强制发送。

**HELD 返回最新的一批，已投递位置推进到最新。** 第一版这样实现，提交前的评审发现：新消息多于一批时，没有返回的较早消息被算作已投递，Turn 成功后确认已读，Agent 再也看不到它们，而 `crew` 没有读消息的命令。改为从最早的开始返回，只推进到返回的最后一条。raft 只返回最新 3 条预览，因为它的 Agent 可以用 `raft message read` 补读（`raft:packages/server/src/routes/internalAgentApi.ts`）。

**handle 就是名字。** raft 的 Agent 名字在工作区内唯一，直接用来 @。没有采用：名字可以是中文，不适合在消息里输入。

## 后果

- 3a 不需要 Redis：已投递位置在 PostgreSQL，唤醒仍走进程内事件。raft 的 HELD 草稿同样存在 PostgreSQL（`attested_send_pending_drafts`）。
- 全部唤醒时，用户的每条消息让群里每个 Agent 都运行一次 Turn。3b 的 triage 用 3a 的实测数据决定是否值得。
- HELD 只防止过时的回复，不防止两个 Agent 看到同一状态后同时发送。raft 的文档承认同样的局限（`raft:manual/agent-knowledge/structural-enforcement.md`）。
- Agent 只能靠 @ 叫来其他 Agent；忘了 @ 时，其他 Agent 要等用户下一条消息才看到它的话。
- 改了常驻规则的文本，全部 Agent 下一轮开新 session。
- SSE 重连后，每个房间回到最新 100 条，之前加载的更早消息需要重新加载。
- 测试：Server 的唤醒规则、HELD、群聊与窗口读取在 `packages/server/test/api.test.ts`；`crew` 的 HELD 输出在 `packages/computer/test/shim.test.ts`；改掉 HELD 检查或唤醒规则时，对应的测试会失败（2026-10-05 手动变异确认）。
