# Agent Note: 运行观测（第 4 步）

Status: implemented

## 问题

Agent 跑一轮时，界面只显示“正在回复”。用户看不到它在做什么，出了问题也无从查起；事后也不知道每一轮花了多少时间与费用、为什么被唤醒、有没有发出消息。具体有四个缺口：

- Agent 的状态不分房间：它在群聊里回复时，私聊里也显示“正在回复”。
- 被 HELD 拦下又改写的回复，界面上看不出来。
- 第 7 步的 triage 要统计“白跑”（被唤醒、完整运行一轮却没有发出消息）占的轮次与费用，需要每一轮的记录。
- 演示时看不到几个 Agent 同时动手干活的过程。

路线图见 [重写的路线图](../../proposed/architecture/2026-10-04-typescript-rewrite.md)“实现顺序”第 4 步。

两个参考项目都有这样的记录（2026-10-05 读源码确认）：

- cumora 每轮一行 `agent_runs`，加上按时间排列的 `agent_events`（`cumora:server/src/db/migrate.ts`）。表前的注释说明了理由：记录由后端写，界面只负责显示，模型流或工具调用卡住时也留得下证据。它另有 `tool_calls`、记每次模型调用用量与费用的 `llm_calls`，以及记每次 triage 判断与它唤醒了哪一轮的 `agent_triages`。观测页 `cumora:src/desktop/ObservabilityView.tsx` 按房间类型统计“没有发言的轮次”的比例与花费（`silentRuns`、`silentSpendUsd`）和每条消息唤醒了几轮，正是第 7 步要量的东西。旧数据由定期清理按天数删除（`cumora:server/src/db-gc.ts`）。
- raft 用 `agent_activity_events` 记 Agent 的活动与一串轨迹条目（`raft:packages/server/src/db/schema.ts`），界面有活动日志与实时活动栏（`raft:packages/web/src/components/agent/AgentActivityLog.tsx`）。它没有按轮的记录表；要看一轮的完整过程时，再去读 Engine 自己的会话文件。

## 决策

2026-10-05 讨论，用户同意下面全部推荐，并确认了设计稿 `apps/desktop/out/mockups/step4-run-observability.html`（不在 git 中）。

**OpenCode 的事件**（用 `deepseek/deepseek-flash` 与 OpenCode 1.18.18 实测）：`opencode run --format json` 每行一个事件。`step_start` 开始一步；`tool_use` 在工具**完成后**才输出，带工具名、输入、输出、起止时间与标题；`text` 是模型的文字；`step_finish` 带 token 与费用。推理内容不输出。所以界面只能显示“刚运行了什么”与“思考中”，显示不了“正在运行什么”。

**运行记录**

- Server 的 PostgreSQL 存 `runs`、`run_triggers`、`run_events`（`packages/server/src/runs.ts`）。每轮存完整输入：第 7 步做评测集时要知道 Agent 当时看到了什么。工具输入输出与文字截短后存。
- Computer 开始一轮时登记，Engine 的每个事件随时上报，结束时写结果（`packages/computer/src/runner.ts` 的 `RunReporter`）。上报失败只记日志，不让这一轮失败。
- 回复与 HELD 由 Server 在 `crew reply` 到达时写进这个 Agent 正在跑的一轮，并在消息上记下所在的一轮与发出前被拦下的次数；Computer 不解析 `crew` 的输出。
- 数据库保证每个 Agent 同一时间最多一轮在跑。Computer 连上时把没结束的轮次标为中断。

**状态**

- Agent 的状态由运行记录推出（`agentStatuses`），带着这一轮涉及的房间；界面按房间显示（`apps/desktop/src/lib/status.ts` 的 `statusIn`）。
- 沙箱不可用、目录不安全这类问题不属于任何一轮，由 Computer 经 `reportProblem` 报告，在每个房间显示为出错。用户同意。

**费用**

- OpenCode 按缓存里的模型价格表算费用。每个 Agent 的缓存目录是空的，OpenCode 不会自己下载，费用一律是 0。每轮启动前把用户缓存里的 `opencode/models.json` 复制进 Agent 的缓存（`packages/computer/src/home.ts` 的 `copyIntoCache`）。缓存目录 Agent 写得了，复制时不顺着符号链接写。

**界面**

- Agent 在这个房间跑一轮时，聊天里显示实时活动：最近三次工具调用、“思考中”、已运行的时间与第几步。
- Agent 的消息下面有“这一轮”，被 HELD 拦下过的多一行“↻ 看到新消息后改写了回复”。
- 运行记录面板（`apps/desktop/src/components/RunPanel.tsx`）从顶栏打开：私聊列出这个 Agent 的轮次，群聊列出全部成员的轮次并可按成员筛选；成功却没有发言的一轮标为“白跑”；展开一轮看用量、本轮输入与时间线。`crew reply` 的工具调用与“发出回复”重复，时间线里只显示后者。
- 汇总统计放到第 7 步。

行为与接口见 [messaging.md](../../../../docs/subsystems/messaging.md) 第 7 节与 [agent-runtime.md](../../../../docs/subsystems/agent-runtime.md) 第 2、3 节。

## 考虑过的方案

**像 raft 一样只记活动，不记按轮的记录。** 活动日志足够显示 Agent 在做什么，但统计不了“一轮花了多少、发没发言”，第 7 步的白跑统计需要按轮汇总。cumora 按轮记录，并且已经用它统计没有发言的轮次。

**一轮一行，把事件存成这一行里的 JSON 数组。** 只要一张表。没有采用：一轮在运行中不断追加事件，每次追加都要改写整行、越改越大；列表只需要概要，却要把每轮的事件一起读出来，或者在查询里排除它；回复与 HELD 也要往同一行里写。分开后，追加一步就是插入一行，列表只读 `runs`，展开一轮时才读事件。

**只存事件，概要在读取时汇总。** 也只要一张表。没有采用：列出 100 轮就要扫过它们全部的事件来算结果、用量与回复数；Agent 的状态要找“还没结束的一轮”，“每个 Agent 最多一轮在跑”这条约束也难在事件上表达。所以 `runs` 存着结果与合计，合计在追加事件的同一个事务里累加。

**唤醒的消息存成 `runs` 里的数组。** 没有采用：群聊的面板要按房间列出轮次，单独的 `run_triggers`（一轮、一个房间各一行）可以直接按房间查，删除房间时也跟着级联删除。一轮可能被几个房间的消息一起唤醒，所以不是 `runs` 上的一个房间列。

cumora 也是这样分的：一轮一行的 `agent_runs`，加按时间排列的 `agent_events`（`cumora:server/src/db/migrate.ts`）。用户问过为什么分开存，以上是回答。

**完全删掉 Computer 上报状态的接口。** 讨论时的推荐。实现时发现沙箱不可用这类问题不属于任何一轮，改为保留只报问题的 `reportProblem`。

## 后果

- 第 7 步可以直接统计白跑：2026-10-05 的真实演示里，没被点名的 Bob 两轮都是白跑，每轮约 8.6k token。
- 运行记录与完整输入都存在本机数据库里，会一直增长；还没有清理。cumora 按天数定期删除旧记录（`cumora:server/src/db-gc.ts`）。
- 工具输出可能包含文件内容，只截短、不脱敏；数据只在本机。
- 界面每收到一次 `run.activity` 就重新读取这一轮与运行记录列表，事件多时请求也多。
- 修复了第 3 步留下的“状态不分房间”。
- 提交前的评审发现：运行记录的写入会被 NUL 与切断的 emoji 弄失败，并因此让 HELD 失效。现在写入前清洗，记录失败也不改变 `crew reply` 的结果，见 [defensive-patterns.md](../../../../docs/defensive-patterns.md)“写进 PostgreSQL 的外来文本先清洗”。
- 测试：Server 的 `api.test.ts` 的 `runs`；Computer 的 `opencode-events.test.ts`（真实输出的样本）、`runner.test.ts` 的 `records each turn…`；界面的 `runs.test.ts`。
