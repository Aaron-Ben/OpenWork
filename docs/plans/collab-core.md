# 开发计划：补齐 Cumora 协作核心

**这是一份执行计划，不是功能文档。** 设计的唯一权威是 [collaboration.md](../collaboration.md) 与 [collaboration-desktop.md](../collaboration-desktop.md)；本文只回答按什么顺序做、怎样算完成。全部工作包完成后删除本文。

## 1. 来源

对照 Cumora 已提交源码（`/Volumes/Extreme SSD/Code/cumora/server/src/agents/`，`main` @ `b7687a1`）。OpenWork 已经实现 Cumora BYOA 的骨架（收件箱、唤醒、HELD、triage、Agenda、Engine 沙箱），缺的是决定协作质量的部分：主模型每轮看到的房间信息、点名路由、防循环和几道服务端闸。本计划只补这些核心缺口；Memory、steer、Calendar 等不做（collaboration.md 开头的范围）。

`collab-opencode.md` 的 C7 并入本计划的 K1；C4–C6 与该计划记录的遗留问题不在本计划内。

## 2. 工作包

按依赖顺序执行。每个工作包的验收条目对应 collaboration.md §16。

| WP | 内容 | 设计 | 完成条件 |
|---|---|---|---|
| K1 | 每轮增量：时间、房间标题行、显示名与身份、消息 id、40 行上限与就地说明、名册；`AGENTS.md` 补 `@<id>`、`--quote` 与“谈到卡片时写出 id”；增量不再重复 persona；Agenda 与卡片 Turn 同样带时间与名册 | §7 | §16 #8 |
| K5 | 引用回复：`collab_messages.quoted_message_id`、`reply --quote`、Desktop `send_message.quotedMessageId`、引用行渲染、引用穿透 mute | §9.3、§13.3.3 | §16 #12 |
| K2 | 点名路由：点名对象的确定、triage 第 2′ 步的 `me`/`each` 判断、`routing` source、fail-open | §8.2、§8.3 | §16 #9 |
| K3 | lap floor 与用户查看：`collab_rooms.user_viewed_seq`、Desktop 上报、triage 第 4 步 | §8.3、§13.3.4 | §16 #10 |
| K4 | 逐字重复拦截：锁内检查、`DUPLICATE` 拒绝码与模型可见文本 | §9.2 | §16 #11 |
| K8 | 对齐 Cumora 的差距：连发闸（`MONOLOGUE`、`--continue`、`collab_messages.run_id`）；私聊不做逐字重复拦截；CLI 输出与 triage 输入的上限、`messages --json`、`messages` 推进 seen；triage 模型失败的 fail closed；`AGENTS.md` 规则与增量开头照搬原文；Desktop 回到前台补报 `collab_room_viewed` | §7.1–§7.3、§8.3、§9.2、§9.4；collaboration-desktop.md §7.2 | §16 #11、#20–#22；collaboration-desktop.md §12 #10 |
| K6 | Column `kind` 替换 `is_terminal`；领取即推进；超时接手（含 running Run 条件） | §11.1、§11.3 | §16 #13、#15 |
| K7 | 卡片唤醒：`collab_card_wakes`、触发条件、合并、Run `card` trigger、结算、限额 | §11.4、§13.3.6 | §16 #14 |
| K10 | 常驻契约补齐 Cumora `standingPrompt` 的两段：开头的 teammate 一句与 “Read the relevant thread …”，以及 “Drive what you own forward …”（去掉依赖 Calendar 的半句） | §7.1 | §16 #8 |
| K9 | 静音：Agent 的 `mute` / `follow` / `mute list`，期限、直聊不可静音、静音时封住未读尾巴；`collab_room_members.muted` 换成到期时间 | §10.1、§13.3.4 | §16 #23 |
| U1 | 取消：运行记录保留在普通导航（E29、E30）；原 U1 中 Runner heartbeat 的 `paused` 不做（E31） | — | — |
| U2a | Agent 当前状态：`AgentView.activity`、`CardView.agentState`；`agent_activity` invalidation；房间里“正在处理”的提示改由 activity 给出，不再读 Run 列表（`RoomView.workingAgentIds` 随房间列表放在 U2b） | collaboration-desktop.md §4.1、§4.3、§5 | collaboration-desktop.md §13 #13 |
| U2b | 房间页：`RoomView` 的未读与“正在处理”（`workingAgentIds`）、平铺列表、筛选、置顶、群组头像拼图、未读、`collab_room_open`、说明行、引用（可跳回原消息）、`@` 补全、卡片链接与摘要卡、右侧卡片预览与 Agent 资料、固定高度工作条；Agent 私聊页 | collaboration-desktop.md §4.2、§4.5、§7 | collaboration-desktop.md §13 #10–12、#15 |
| U2c | Agent 页 | collaboration-desktop.md §8 | collaboration-desktop.md §13 #13 |
| U2d | 看板页：Desktop 创建、编辑、拖动卡片（新 Desktop 命令，产生卡片唤醒）、列类型标记、卡片状态、卡片详情 | collaboration.md §11.2；collaboration-desktop.md §4.3、§9 | collaboration-desktop.md §13 #14 |
| U2e | 识别色令牌与视觉收尾、三种语言文案 | collaboration-desktop.md §11 | collaboration-desktop.md §13 #16 |

## 3. 每个工作包的顺序

1. 先改测试（验收条目先写成失败的测试），再改代码；
2. 模型可见文本（增量、路由题目、`DUPLICATE` 文本、卡片 Turn 说明）逐字断言；
3. 协议形状变化同步 `desktop/src/bridge/compat.ts` 与契约测试；
4. `scripts/check.sh` 通过后更新进度文件。
