# Agent Note: 协作界面以 Cumora 桌面端为参照

Status: legacy

## 问题

协作模式需要一套完整的桌面界面：导航、房间列表、消息流、右侧栏、Agent 页与看板。从零设计这些界面，要反复试错。Cumora 是同类的多 Agent 协作产品，它的桌面端已经覆盖这些界面。

要定的是：参照 Cumora 到什么程度，哪些地方与它不同。

## 决策

界面结构逐项对照 Cumora 桌面端的组件。行为、尺寸与文案以 OpenWork 代码为准，见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md) §2 与 §7–§10。

| OpenWork（`desktop/src/features/collab/`） | Cumora |
|---|---|
| `components/CollabRail.tsx` | `src/desktop/Rail.tsx` |
| `rooms/RoomList.tsx`、`rooms/RoomListRow.tsx` | `src/desktop/ConversationsPane.tsx` |
| `rooms/MessageItem.tsx` 的引用 | `src/components/Message.tsx` 的 `QuoteCard` |
| `rooms/CardLinks.tsx` | `src/components/CardLink.tsx`；`Message.tsx` 的 `CardArtifactCard` |
| `rooms/CardPreviewPanel.tsx` | `src/desktop/BoardPeekPane.tsx` |
| `rooms/AgentProfilePanel.tsx` | `src/desktop/InfoPane.tsx` |
| `rooms/WorkBar.tsx` | `Message.tsx` 的 `TypingRow`：固定高度，用透明度切换 |
| `rooms/WhispersPage.tsx` | `src/desktop/WhispersView.tsx` |
| `agents/AgentManager.tsx` 的 `HireCard` | `src/desktop/AgentsView.tsx` 的 `HireCard` |
| `boards/AddCardInline.tsx` | `src/desktop/BoardsView.tsx` 的 `ColumnView` |

与 Cumora 不同的地方：

- 房间列表只有四个筛选：全部、未读、Agent、群组。Cumora 另有 Humans、Email 与 Whispers（`ConversationsPane.tsx` 的 `staticFilters`）。OpenWork 只有一个本机用户，也没有邮件渠道。
- Agent 之间的房间不进房间列表，只在“Agent 私聊”页显示。这个页面始终在 Rail 上。Cumora 只向 owner 显示它（`Rail.tsx` 按 `isOwner` 过滤 `whispers`）。
- 运行记录页不放在开发者模式后面，见 [运行记录页](2026-09-24-run-records-page.md)。
- 用户查看房间算作人类关注，见 [lap floor](../architecture/2026-09-24-lap-floor.md)。

## 考虑过的方案

**房间列表按类型分组，带分组标题。** 没有采用：头像已经区分群组与私聊。原设计文档说 Cumora 也因此删掉了分组标题。这一点未确认：当前 `ConversationsPane.tsx` 的列表项仍有 `label` 类型（`ConvoListItem`）。

**把 Agent 之间的私聊作为房间列表的一个筛选项。** 这是 Cumora 的做法（`ConversationsPane.tsx` 的 `Whispers` 筛选）。没有采用，改为单独的只读页。原始记录没有写理由。

## 后果

- 每个界面元素都有可以对照的源码。讨论界面时，可以直接引用 Cumora 的文件。
- 代价：Cumora 以后改版，OpenWork 不会自动跟随。上表只在 OpenWork 界面改动时更新。
- 筛选项“Agent”与页面“Agent 私聊”名字相近，含义不同。前者是用户与某个 Agent 的私聊，后者是 Agent 之间的私聊。
- 原设计要求 Rail 的房间图标显示未读总数，照 Cumora `Rail.tsx` 的 `totalUnread`。代码没有实现。未读总数只显示在工作台侧栏的协作入口上。
- 界面设计稿：https://claude.ai/artifact/MDGsQTdy7KuRLYPuvFGHeR（房间亮/暗、房间里点开卡片、Agent、看板）。设计稿与代码不一致时，以代码为准。
