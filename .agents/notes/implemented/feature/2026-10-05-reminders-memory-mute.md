# Agent Note: 提醒、记忆与静音（第 6 步）

Status: implemented

## 问题

- Agent 只在有新消息时醒来。它答应“半小时后看一下 CI”“每天早上汇总一次”时，到点没有人叫它，这件事就落空了。
- Agent 的 OpenCode 会话在换模型或常驻规则变化时重开，重开后它不记得之前做过什么、用户偏好什么。
- 群里用户的每条消息都唤醒全部 Agent。和某个 Agent 无关的群（比如它只在里面偶尔被点名），它每次都被叫醒、白跑一轮，自己也没有办法说“这个群别叫我”。

路线图见 [重写的路线图](../../proposed/architecture/2026-10-04-typescript-rewrite.md)“实现顺序”第 6 步：Agent 用 `crew` 给自己定时或周期提醒，到时唤醒它自己；Agent 目录里有 `MEMORY.md`，由 Agent 自己维护；静音房间，是用户不再看到未读，还是 Agent 不再被唤醒，到这一步再定。

### 参考项目的做法

2026-10-05 读源码确认。

**提醒**

- raft：表 `reminders`，只能给自己定，必须挂在一条消息上；一次性（`--delay-seconds`、`--fire-at`）或周期（`every:30m`、`daily@09:00`、`weekly:mon,fri@09:00`，带时区，最短 30 秒，最长一年）；计时器在 Agent 所在机器的守护进程里，Server 只核对。到点后 Agent 收到收件箱里的一项“Reminder due”，**不发聊天消息**：用户认为定提醒是 Agent 的私事。错过的一次性提醒在机器连上时补触发，周期提醒跳过错过的那几次。没有数量上限。界面只读（`raft:packages/server/src/db/schema.ts`，`raft:packages/server/src/services/recurrence.ts`，`raft:packages/daemon/src/apps/reminder/reminderCache.ts`，规则在 `raft:packages/daemon/src/drivers/raftCliGuide.ts` 的 `buildRemindersSection`：用提醒代替长时间等待，提醒只叫醒自己）。
- cumora：没有单独的提醒，是共享日历里 `agent_task` 类型的事件；Server 每 60 秒扫一遍，到点在目标会话里发一条作者为 `calendar` 的系统消息，Agent 读到“A Calendar event is due”。错过超过 1 小时的一次性事件直接标为完成、不再触发（`cumora:server/src/calendar.ts`，`cumora:server/src/db/migrate.ts`）。
- 旧版 OpenWork 没有提醒，Agent 不能自己定时唤醒。

**记忆**

- raft：`<agent 目录>/MEMORY.md` 加 `notes/`，创建 Agent 时写一份模板；**不放进提示词**，常驻规则让 Agent 每次开会话先读它、有长期价值的东西写进去；文件超过 64KB 时提醒 Agent 精简。界面只读（`raft:packages/daemon/src/workspaces.ts`）。
- cumora：本地 Agent 的 `memory/MEMORY.md` 截取前 4,000 字符放进每轮输入；云端版另有向量检索（`cumora:server/src/agents/memory-scope.ts`）。规则写着“说‘我会记住’却不写文件，就是不会记住”。
- 旧版 OpenWork 有意不做记忆。

**静音**

- raft：人和 Agent 共用一张表，静音从当时的最新消息开始生效。人静音后不再收到通知，未读数变成不显眼的灰色；Agent 静音后整个房间不再投递、不再唤醒，但 @ 它的消息、私聊、它关注的讨论串、分配给它的任务仍然送达。私聊不能静音（`raft:packages/server/src/services/inboxMutePolicy.ts`）。
- cumora：人和 Agent 共用 `conversation_mutes`，可以定时长（15 分钟到一周）；Agent 静音时把已读位置推到最新，之后只投递 @ 它或引用它的那条消息（`cumora:server/src/agents/scheduler.ts` 的 `shouldDeliverToMutedAgent`）。
- 旧版 OpenWork 只给 Agent 静音：被 @ 或引用时，把静音以来的未读一起投递，让它有上下文（旧版的静音 Note 已在第 9 步删除，原文见 `git show dd8779b:.agents/notes/legacy/feature/2026-09-25-room-mutes.md`）。界面没有静音入口。

## 决策

2026-10-05 用户同意下面全部推荐。

1. **拆分与顺序。** 6a 记忆、6b 提醒、6c 静音，按这个顺序做。记忆最小；提醒最能演示；静音与第 7 步的 triage 相关，放最后。
2. **提醒的数据与命令。** 一张 `reminders` 表：属于哪个 Agent、在哪个房间定的、标题、触发时间、周期规则、状态。Agent 只能给自己定。周期规则照 raft 只有三种：每隔 N 分钟或小时、每天几点、每周几几点，不做 cron。命令 `crew remind <room-id> <标题> --in 30m | --at 18:00 | --every 1h | --daily 09:00 | --weekly mon,fri@09:00`，加 `crew remind list` 与 `crew remind cancel <id>`。每个 Agent 最多 20 个未触发的提醒，周期最短 5 分钟，一年以内，时间按本机时区。
3. **由谁计时、到点后发生什么。** Server 计时，只排下一个到期的：Crew 的 Server 与 Computer 在同一台机器上，不需要 raft 那种在 Agent 所在机器计时、Server 核对的做法。到点在定提醒的房间发一条通知（`kind = 'system'`）“Alice 的提醒到了：检查 CI”，只唤醒提醒的主人。与 raft 相反、给用户看：Crew 只有一个用户，看得到 Agent 为什么突然开口更重要，演示也直观；还直接复用通知、唤醒与运行记录，不加新的唤醒来源。
4. **错过的提醒。** 启动时，过期的一次性提醒补触发一次，通知里写明应在什么时候触发；周期提醒只补一次，然后排下一次，不重放错过的几次。
5. **记忆放在哪里、怎样用。** Agent 工作目录里的 `MEMORY.md`，新建 Agent（Computer 准备目录）时写一份模板，已有时不动。不放进每轮输入（照 raft）：放进常驻规则会让每次修改都重开会话，放进每轮输入会让每轮都多出这些 token。Computer 开新会话时，本轮输入多一句“这是新会话，先读 MEMORY.md”；文件超过 16KB 时这句话里提醒精简。常驻规则写清什么值得记，以及“说我会记住却不写文件，就是不会记住”（cumora 的说法）。
6. **记忆在界面上。** 私聊的右栏加一个只读的“记忆”视图，由 Electron 主进程读本机文件。不做编辑。
7. **静音。** 只给 Agent：`crew mute <room-id> [--for 2h]` 与 `crew unmute <room-id>`。静音后用户在群里的消息不再唤醒它；@ 它的消息、它关注的讨论串、分配给它的任务仍然唤醒它，被唤醒时静音期间的未读一起给它，让它有上下文（照旧版 OpenWork）。私聊不能静音。群聊顶栏的成员标签上显示“已静音”，用户可以点开解除。用户自己的静音先不做：只有一个用户，房间也不多。

8. **通知的样子（2026-10-05 用户同意，设计稿 `apps/desktop/out/mockups/step6-notices-mute.html`）。** `messages` 加一列 `notice`（JSON：通知的类型与数据，例如 `{type: "task.claimed", number: 3}`、`{type: "reminder", title, repeat, setAt, dueAt}`），界面据此画图标与提醒卡片，不从文字里解析；正文照旧保留，Agent 读到的仍是文字。颜色只用四种：蓝是任务的变化，绿是完成，黄是退回与晚到，紫是提醒与静音。提醒到点用一张淡紫色小卡片：标题、一次性或周期、什么时候定的，晚到时写明原定时间。
9. **静音的补充（同上）。** Agent 静音或被用户解除时，群里写一行紫色通知；静音到期不写（要另外计时），成员标签自动恢复。Agent 自己的提醒也能穿透静音。顶栏成员标签变灰并带划掉的铃铛，点开看到静音到几点、什么仍会叫醒它，可以解除；加成员对话框的成员列表里也能解除。
10. **静音的数据与收件箱（2026-10-05 开始 6c 前补充，用户同意全部推荐）。**
    - 存在 `room_agents` 上：`muted_at` 与 `muted_until`（为空是一直静音）。静音是“这个 Agent 在这个群里”的属性，退群时随成员关系一起删掉；只有这一个用处，不另建表。
    - `--for` 选填，15 分钟到 7 天（同 cumora）；不写就一直静音，直到 `crew unmute`：最典型的是“这个群和我无关”，是长期的。到期不计时，读的时候过了截止时间就当没静音，也不写通知。
    - 收件箱：Agent 被别的事唤醒时，不给它静音群的未读，否则它在私聊里被叫醒也会读到并回复静音群，静音形同虚设。静音群里出现能穿透的消息（@ 它、它自己的提醒）时，这个群的全部未读一起给它，让它有上下文。讨论串是单独的房间，不受群静音影响；任务通知发在任务的讨论串里并 @ 负责人，照常唤醒。照 raft：静音的房间不投递。
    - 这一轮的输入里，静音群的标题后面写明 “you muted it until <时间>”（一直静音时只写 “you muted it”），Agent 被 @ 叫醒时知道为什么一下子收到很多旧消息，也知道可以 `crew unmute`。

### 实现

- **6a 记忆（2026-10-05 完成）。** 文件名与位置是 protocol 的 `MEMORY_FILE` 与 `agentWorkSegments`，Computer 与 Desktop 主进程共用；模板与大小在 `packages/computer/src/home.ts`（`seedMemory`、`memorySize`），开新会话的提醒在 `packages/computer/src/prompt.ts`（`sessionNote`）；界面经主进程读文件（`apps/desktop/electron/memory.ts`），至多读 256KB。OpenCode 报告旧会话失效、自动改开新会话时，这一轮输入里没有“先读记忆”这句，要等下一次 Computer 自己判断为新会话。

- **6b 提醒（2026-10-05 完成）。** 表 `reminders`（迁移 `0006_reminders.sql`），代码在 `packages/server/src/reminders.ts`：计时器 `ReminderScheduler` 只排下一个到期的，最长睡一小时，启动时先补触发；Server 的“现在”由 `ServerContext.now` 提供，测试可以固定。提醒的通知以主人的名义写下却要唤醒主人，HELD 因此把它当作没看过（`packages/server/src/messages.ts` 的 `heldMessages`）：否则一轮进行中提醒到点、这一轮又在同一房间回复时，提醒被当成已读，下一轮读不到（2026-10-05 排查发现）。6c 静音若也有“自己名下却要唤醒自己”的通知，照此处理。通知经 `postMessageIn` 的 `wake` 选项只唤醒主人，主人自己写的通知也唤醒它。提醒的标题至多 200 字符。`crew remind` 的 `--at 18:00` 是下一个 18:00。到点的通知带 `notice`（`type: "reminder"`、标题、周期、定于何时、原定时间），界面画成淡紫色小卡片；任务的通知也带上类型（迁移 `0007_message_notice.sql`），图标与颜色由 `apps/desktop/src/lib/notices.ts` 的 `noticeLook` 决定。

- **6c 静音（2026-10-05 完成）。** 列 `room_agents.muted_at`、`muted_until`（迁移 `0008_room_mutes.sql`），静音与解除在 `packages/server/src/mutes.ts`；唤醒与收件箱的判断在 `packages/server/src/messages.ts`（`mutedNow`），按数据库的 `now()` 判断到期。用户替 Agent 解除的通知写它的名字、不 @ 它，也不唤醒它（`quiet`）：唤醒会让它为一行告知白跑一轮；@ 会记成点到它，它以后再静音时，这条旧通知会被当成 @ 它而穿透静音。被别处唤醒时静音群的未读留着不投递，所以 Agent 自己写的“静音了这个群”也会在它下次被 @ 时一起给它，正好说明前因后果。界面在 `apps/desktop/src/components/ChatView.tsx` 的 `MutedChip` 与加成员对话框；界面开着期间到期的静音由 `apps/desktop/src/lib/mutes.ts` 的 `activeMutes` 自己去掉，不等 Server。真实模型演示见 `docs/subsystems/agent-runtime.md` 的验收。

## 考虑过的方案

- **在 Agent 所在的机器上计时、Server 只核对（raft）。** raft 的 Agent 可能在别的机器上，计时器跟着 Agent 走。Crew 的 Server 与 Computer 在同一台机器上，Server 计时就够，少一个进程间同步。
- **提醒到点不发聊天消息，只在 Agent 的收件箱里放一项（raft）。** raft 认为定提醒是 Agent 的私事。Crew 只有一个用户，看得到 Agent 为什么突然开口更重要，演示也直观；还直接复用通知、唤醒与运行记录，不加新的唤醒来源。
- **把记忆放进每轮输入（cumora 截取前 4,000 字符）或常驻规则。** 放进常驻规则会让每次修改都重开会话，放进每轮输入会让每轮都多出这些 token。改为开新会话时提醒它先读文件（照 raft）。
- **用户自己的静音。** 只有一个用户，房间也不多，先不做。
- **静音时把已读推到最新，之后只投递点名的那一条（cumora）。** Agent 被 @ 时看不到前因后果。改为照旧版 OpenWork，把静音期间的未读一起给它。
- **单独建一张 `room_mutes` 表。** 静音是“这个 Agent 在这个群里”的属性，只有这一个用处，放在成员关系上，退群时随之删掉。
- **静音到期时在群里写一行通知。** 要另外计时；成员标签到期自动恢复，用户已经看得到。

## 后果

- 好处：Agent 答应“过一会儿再看”的事有人叫它；换模型、换规则开了新会话也记得用户与之前的事；与它无关的群不再每条消息都叫它白跑一轮。三者都复用已有的通知、唤醒、收件箱与运行记录，没有新的唤醒来源。
- 提醒的通知以主人的名义写下却要唤醒主人，打破了“自己写的不唤醒自己、也不拦自己的回复”的前提：HELD 为它开了例外（见“实现”）。以后再有这样的通知，要同样处理。
- 两个时钟：提醒按 `ServerContext.now` 计时（测试可以固定），静音按数据库的 `now()` 判断到期。两者在同一台机器上，差别可以忽略；但测试里固定了 `now` 时，静音不跟着变。
- 静音群的未读一直积压，直到被 @ 或提醒时一次给 Agent。群很活跃、静音很久时，这一轮的输入会很长；目前没有上限，真实使用中出现问题再截短。
- 记忆靠 Agent 自己读写文件，它不读、不写时就记不住；OpenCode 报告旧会话失效、自动改开新会话的那一轮没有“先读记忆”这句。
- 用户只能解除静音，不能替 Agent 静音；Agent 不想听又没想到静音时，只能等第 7 步的 triage。
- 验证：提醒、静音与改状态的通知见 `packages/server/test/api.test.ts` 的 `reminders`、`mutes`、`tasks`；`crew remind`、`crew mute` 的输出与规则见 `packages/computer/test/__snapshots__/shim-output.md`、`AGENTS.md`，静音群的标题见 `turn-prompt.md`；记忆模板与新会话的提醒见 `home.test.ts`、`prompt.test.ts`；界面的通知样式与静音判断见 `apps/desktop/test/lib.test.ts`。真实模型的记忆、提醒与静音演示见 [agent-runtime.md](../../../../docs/subsystems/agent-runtime.md) 的验收。
