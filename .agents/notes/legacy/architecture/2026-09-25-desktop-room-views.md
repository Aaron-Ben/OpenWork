# Agent Note: Desktop 房间页使用专用视图

Status: legacy

## 问题

Desktop 重做房间页。新界面需要很多 Agent 用不到的数据：未读数、最近一条消息、成员头像、置顶、作者显示名与 role、时间，以及说明行。

原来 Desktop 与 Agent 命令共用 `RoomView` 与 `MessageView`，Desktop 用 `collab_message_list` 读消息。Agent 的 `openwork rooms` 直接把这些视图输出为 JSON。在共用视图上加字段，模型每次读到的内容也会变。

另外，在此之前 `AgentView.activity` 已经带上 Agent 正在哪个房间工作。房间页如果再从别处取“谁在工作”，同一事实就有两个来源。

## 决策

Desktop 房间页使用自己的视图，Agent 命令的视图不变（用户确认，2026-09-25）。

- 形状在 `crates/openwork-collab/src/protocol/room_views.rs`：
  - `RoomSummaryView`：`RoomView` 的字段，加 `unreadCount`、`lastMessage`、`lastMessageAt`、`userIsMember`、`memberIds` 与 `pinned`；
  - `RoomSnapshotView`：只含 `messages` 与 `notes`；
  - `RoomMessageView`：用 `#[serde(flatten)]` 包含 `MessageView`，另加 `authorName`、`authorKind`、`authorRole` 与 `createdAt`；
  - `RoomNoteView`：路由、一轮上限、硬上限三种说明行。
- Server：`server/room_summaries.rs` 负责房间列表与 `PinRoom`，最近一条消息取前 80 字（`LAST_MESSAGE_MAX_CHARS`）。`server/room_snapshot.rs` 负责 `OpenRoom` 快照，`server/room_notes.rs` 生成说明行。迁移 `202609250001_room_pin.sql` 加入 `collab_rooms.user_pinned_at`。
- Tauri：`collab_room_list`、`collab_room_open`、`collab_room_pin`（`desktop/src-tauri/src/commands/collab.rs`）。`collab_message_list`、`ListMessages`、`Messages::list` 与 `Rooms::list` 已删除。
- 谁在工作，前端用 `desktop/src/features/collab/rooms/roomWorking.ts` 的 `roomWorkers` 从 Agent 列表的 `activity` 得出。成员的当前状态也从 Agent 列表得出。
- Agent 命令的 `RoomView` 与 `MessageView`（`protocol/desktop.rs`）不变，模型看到的输出也不变。

设计见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md) §4.2、§4.5 与 §7。

## 考虑过的方案

**单独下发 `workingAgentIds`。** 房间列表或快照直接带上正在工作的 Agent。没有采用：`AgentView.activity` 已经给出 Agent 在哪个房间工作，前端可以直接算出。

**快照带上 Agent 成员状态。** 原文档要求房间快照包含 Agent 成员状态。2026-09-25 用户确认改为快照只含消息与说明行，成员状态从 Agent 列表得出。理由与上一条相同：状态只保留一个来源。

## 后果

- Desktop 界面的改动不改变 Agent 命令的协议形状，也不改变模型读到的输出。
- 代价：两套相近的形状要分别维护。`RoomMessageView` 平铺 `MessageView`，所以 `MessageView` 加字段时，两边一起变。
- 房间页要同时持有房间列表与 Agent 列表。`agent_activity` invalidation 刷新 Agent 列表后，“正在处理”行才更新。
- 快照没有分页。长期房间需要分页时，先扩展 Server 的读取命令（collaboration-desktop.md §4.2）。
- 打开的房间靠 invalidation 刷新。collaboration-desktop.md §5 写的“每 2 秒调用一次 `collab_room_open`”在代码中不存在。原进度文件的待定项记录了这一点。
- 测试：`room_page::acc_15_room_list_carries_unread_last_message_members_and_pin`、`room_page::acc_12_room_snapshot_carries_authors_quotes_and_notes`、`protocol::room_views::tests`、`roomWorking.test.ts`。
- 消息里的引用与卡片链接见 [引用回复](../feature/2026-09-24-message-quotes.md) 与 [卡片链接](2026-09-24-card-links-instead-of-board-events.md)。
