# Agent Note: 提醒、记忆与静音（第 6 步）

Status: proposed

## 问题

- Agent 只在有新消息时醒来。它答应“半小时后看一下 CI”“每天早上汇总一次”时，到点没有人叫它，这件事就落空了。
- Agent 的 OpenCode 会话在换模型或常驻规则变化时重开，重开后它不记得之前做过什么、用户偏好什么。
- 群里用户的每条消息都唤醒全部 Agent。和某个 Agent 无关的群（比如它只在里面偶尔被点名），它每次都被叫醒、白跑一轮，自己也没有办法说“这个群别叫我”。

路线图见 [重写的路线图](../architecture/2026-10-04-typescript-rewrite.md)“实现顺序”第 6 步：Agent 用 `crew` 给自己定时或周期提醒，到时唤醒它自己；Agent 目录里有 `MEMORY.md`，由 Agent 自己维护；静音房间，是用户不再看到未读，还是 Agent 不再被唤醒，到这一步再定。

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
- 旧版 OpenWork 只给 Agent 静音：被 @ 或引用时，把静音以来的未读一起投递，让它有上下文（[静音 Note](../../legacy/feature/2026-09-25-room-mutes.md)）。界面没有静音入口。

## 决策

2026-10-05 用户同意下面全部推荐。

1. **拆分与顺序。** 6a 记忆、6b 提醒、6c 静音，按这个顺序做。记忆最小；提醒最能演示；静音与第 7 步的 triage 相关，放最后。
2. **提醒的数据与命令。** 一张 `reminders` 表：属于哪个 Agent、在哪个房间定的、标题、触发时间、周期规则、状态。Agent 只能给自己定。周期规则照 raft 只有三种：每隔 N 分钟或小时、每天几点、每周几几点，不做 cron。命令 `crew remind <room-id> <标题> --in 30m | --at 18:00 | --every 1h | --daily 09:00 | --weekly mon,fri@09:00`，加 `crew remind list` 与 `crew remind cancel <id>`。每个 Agent 最多 20 个未触发的提醒，周期最短 5 分钟，一年以内，时间按本机时区。
3. **由谁计时、到点后发生什么。** Server 计时，只排下一个到期的：Crew 的 Server 与 Computer 在同一台机器上，不需要 raft 那种在 Agent 所在机器计时、Server 核对的做法。到点在定提醒的房间发一条通知（`kind = 'system'`）“⏰ Alice 的提醒：检查 CI”，只唤醒提醒的主人。与 raft 相反、给用户看：Crew 只有一个用户，看得到 Agent 为什么突然开口更重要，演示也直观；还直接复用通知、唤醒与运行记录，不加新的唤醒来源。
4. **错过的提醒。** 启动时，过期的一次性提醒补触发一次，通知里写明应在什么时候触发；周期提醒只补一次，然后排下一次，不重放错过的几次。
5. **记忆放在哪里、怎样用。** Agent 工作目录里的 `MEMORY.md`，新建 Agent（Computer 准备目录）时写一份模板，已有时不动。不放进每轮输入（照 raft）：放进常驻规则会让每次修改都重开会话，放进每轮输入会让每轮都多出这些 token。Computer 开新会话时，本轮输入多一句“这是新会话，先读 MEMORY.md”；文件超过 16KB 时这句话里提醒精简。常驻规则写清什么值得记，以及“说我会记住却不写文件，就是不会记住”（cumora 的说法）。
6. **记忆在界面上。** 私聊的右栏加一个只读的“记忆”视图，由 Electron 主进程读本机文件。不做编辑。
7. **静音。** 只给 Agent：`crew mute <room-id> [--for 2h]` 与 `crew unmute <room-id>`。静音后用户在群里的消息不再唤醒它；@ 它的消息、它关注的讨论串、分配给它的任务仍然唤醒它，被唤醒时静音期间的未读一起给它，让它有上下文（照旧版 OpenWork）。私聊不能静音。群聊顶栏的成员标签上显示“已静音”，用户可以点开解除。用户自己的静音先不做：只有一个用户，房间也不多。

8. **通知的样子（2026-10-05 用户同意，设计稿 `apps/desktop/out/mockups/step6-notices-mute.html`）。** `messages` 加一列 `notice`（JSON：通知的类型与数据，例如 `{type: "task.claimed", number: 3}`、`{type: "reminder", title, repeat, setAt, dueAt}`），界面据此画图标与提醒卡片，不从文字里解析；正文照旧保留，Agent 读到的仍是文字。颜色只用四种：蓝是任务的变化，绿是完成，黄是退回与晚到，紫是提醒与静音。提醒到点用一张淡紫色小卡片：标题、一次性或周期、什么时候定的，晚到时写明原定时间。
9. **静音的补充（同上）。** Agent 静音或被用户解除时，群里写一行紫色通知；静音到期不写（要另外计时），成员标签自动恢复。Agent 自己的提醒也能穿透静音。顶栏成员标签变灰并带划掉的铃铛，点开看到静音到几点、什么仍会叫醒它，可以解除；加成员对话框的成员列表里也能解除。

### 实现进度

- **6a 记忆（2026-10-05 完成）。** 文件名与位置是 protocol 的 `MEMORY_FILE` 与 `agentWorkSegments`，Computer 与 Desktop 主进程共用；模板与大小在 `packages/computer/src/home.ts`（`seedMemory`、`memorySize`），开新会话的提醒在 `packages/computer/src/prompt.ts`（`sessionNote`）；界面经主进程读文件（`apps/desktop/electron/memory.ts`），至多读 256KB。OpenCode 报告旧会话失效、自动改开新会话时，这一轮输入里没有“先读记忆”这句，要等下一次 Computer 自己判断为新会话。

- **6b 提醒（2026-10-05 完成）。** 表 `reminders`（迁移 `0006_reminders.sql`），代码在 `packages/server/src/reminders.ts`：计时器 `ReminderScheduler` 只排下一个到期的，最长睡一小时，启动时先补触发；Server 的“现在”由 `ServerContext.now` 提供，测试可以固定。通知经 `postMessageIn` 的 `wake` 选项只唤醒主人，主人自己写的通知也唤醒它。提醒的标题至多 200 字符。`crew remind` 的 `--at 18:00` 是下一个 18:00。到点的通知带 `notice`（`type: "reminder"`、标题、周期、定于何时、原定时间），界面画成淡紫色小卡片；任务的通知也带上类型（迁移 `0007_message_notice.sql`），图标与颜色由 `apps/desktop/src/lib/notices.ts` 的 `noticeLook` 决定。

## 考虑过的方案

<!-- agent-note: 原始记录没有备选方案 -->

## 验收条件

待定。

## 风险

待定。
