# macOS 协作桌面端

本页描述协作模式的桌面界面：Shell 与导航、Tauri 监督进程、命令与事件、各页面的结构与行为。React 代码在 `desktop/src/features/collab/` 与 `desktop/src/bridge/`。Tauri host 的协作部分在 `desktop/src-tauri/src/collab_client.rs`、`collab_event_bridge.rs` 与 `commands/collab.rs`。命令与视图类型由 `openwork-collab::protocol` 定义。

业务语义与存储见 [collaboration.md](collaboration.md)。界面结构以 Cumora 桌面端为参照，对照表与差异见 [Agent Note：协作界面参照 Cumora](../../.agents/notes/implemented/feature/2026-09-25-collab-desktop-follows-cumora.md)。

## 1. 边界

| Desktop 负责 | Desktop 不负责 |
|---|---|
| 启动、监督和停止 Server/Computer 子进程 | 不执行 Agent loop |
| 管理 Agent profile 与运行配置 | 不选择 triage 结果或 Agenda 候选 |
| 创建 Room、管理 Group 成员、发送与引用消息 | 不直接读写 PostgreSQL/Redis |
| 管理 Board/Column 结构、Column 类型和删除；创建、编辑、移动、改派卡片 | 不向 Agent 暴露结构删除命令 |
| 上报用户看到了哪些消息（`user_viewed_seq`） | 不持有 Agent JWT 或 Engine session |
| 显示 Runtime、Engine、Agent 当前状态、Message、Card 状态与运行记录 | 不在房间里显示运行细节 |
| 把 Desktop SSE invalidation 转成 Tauri event | 不把 SSE 当成业务事实 |

房间、Agent 与看板页面显示当前状态：谁在工作，谁出错了，哪张卡片在等谁。房间里的说明行解释为什么有人没回复（§7.3）。一次 Run 的过程只在运行记录页显示（§10）。理由见 [Agent Note：运行记录页](../../.agents/notes/implemented/feature/2026-09-24-run-records-page.md)。

只支持当前 Mac。界面没有远程机器、Computer 选择器或后台 Runtime 开关。Desktop 正常退出时停止 Collaboration Runtime（§3.3）。

## 2. Shell 与导航

`desktop/src/App.tsx` 按 `modeStore` 的 mode 选择 `AppShell`（工作台）或 `CollabShell`（协作）。mode 写入 `localStorage` 的 `openwork-mode`，无法识别的值按 `workbench` 处理。切换 mode 只替换 React 组件树，不重启 Tauri host 或 Collaboration Runtime。

`CollabShell` 挂载时读取房间列表、Agent 列表与 Runtime 状态，并订阅 invalidation（§5）。没有选中的房间时，它选中列表中第一个用户所在的房间。

`CollabRail` 宽 80px（`w-20`），有五个目的地，底部是返回工作台：

| view | 标签（zh-CN） | 页面 |
|---|---|---|
| `rooms` | 房间 | §7 |
| `whispers` | Agent 私聊 | §7.6 |
| `agents` | 同事 | §8 |
| `boards` | 看板 | §9 |
| `observability` | 观测 | §10 |

Rail 上没有未读数。未读总数显示在工作台侧栏的“协作”入口上（`app/Sidebar.tsx` 的 `WorkbenchFooter`）。它是用户所在房间的 `unreadCount` 之和，取自 `roomStore`。`roomStore` 只在协作模式中读取与刷新。

macOS 上，Rail 顶部有 28px（`h-7`）的窗口拖拽区。协作 feature 不 import 工作台的 chat、sessions 与 traces feature。两者只共享 UI primitive、主题、i18n 与通用错误处理。

## 3. Tauri 监督进程

### 3.1 启动

应用 setup 的顺序（`desktop/src-tauri/src/lib.rs`）：

```text
OpenWorkCore::bootstrap
  → Core 事件桥
  → CollabDaemonClient::discover_or_start
      → 启动 Server，校验 ready
      → 启动 Computer，校验 ready
      → 等待 Computer 的第一次 heartbeat
      → 启动 Desktop SSE 任务
  → 协作 invalidation 桥
  → manage(CollabDaemonClient)，manage(OpenWorkCore)
```

- 状态目录取 `OPENWORK_COLLAB_HOME`，没有时取 `~/.openwork`，权限 0700。
- Desktop 对 `runtime.lock` 加排他 `flock`。另一个 Desktop 已持有它时，启动失败（`AlreadyRunning`）。
- 每个 RuntimeSession 生成新的凭据，并建立 `runtime/<runtimeSessionId>/`。建立前删除 `runtime/` 下的所有旧条目。
- 两个子进程都是 Desktop 自己的可执行文件，参数是 `--openwork-collab-server` 或 `--openwork-collab-computer`。
- Desktop 向子进程 stdin 写一行 bootstrap JSON，从 stdout 读一行 ready JSON，上限 64 KiB。每个子进程必须在 30 秒内 ready。
- Server 的 ready 必须带相同的 `runtimeSessionId`，`baseUrl` 必须是端口非零的 loopback `http` 地址。Computer 的 ready 必须带相同的 `runtimeSessionId`。
- 两者 ready 后，Desktop 轮询 `Status`，30 秒内必须看到 Computer 的 heartbeat。
- 任一步失败时，Desktop 停止已启动的子进程，并删除本次的 runtime 目录。
- Computer 的环境中去掉 `DATABASE_URL`、`REDIS_URL`、`TEST_DATABASE_URL`、`TEST_REDIS_URL` 与 `PG*` 连接变量。Engine 可执行文件取 `OPENCODE_BIN`，默认 `opencode`。

首次启动失败时，Tauri setup 失败，不注册半可用的 managed state。

### 3.2 运行与成组替换

`CollabDaemonClient` 对外只有三个能力：`call(DesktopCommand)`、`subscribe_invalidations()`、`shutdown()`。它持有 `runtime.lock` 句柄、当前 connection（Server 地址与 Desktop secret）、supervisor channel，以及容量 128 的 invalidation 广播。子进程句柄、SSE 任务与 runtime 目录归 supervisor 任务所有。

supervisor 每 250ms 检查一次两个子进程。任一个退出时，它按这个顺序操作：

1. 把 connection 置空。此后 `call` 返回 `Unavailable`。
2. 停止整组子进程与 SSE 任务，删除 runtime 目录。
3. 用新凭据启动新的一组。成功后，新命令使用新 RuntimeSession 的 connection。

重启失败时，supervisor 每 2 秒重试一次。

`call` 用 Desktop secret 发送 `POST /desktop/commands`。修改类命令带新的 `requestId`；读取命令与 `RoomViewed` 不带（`DesktopCommand::is_mutating`）。每次请求超时 20 秒。网络错误与 5xx 最多尝试 3 次，间隔从 100ms 起翻倍，上限 1 秒。其他非 2xx 响应返回 `Rejected`，带 Server 的 code 与 message。

### 3.3 退出

应用事件循环返回后，`lib.rs` 取出 setup 注册的 `CollabDaemonClient`，等待 `shutdown()` 完成，再用原 exit code 退出 Desktop。

- shutdown 的总等待上限是 30 秒。
- 停止一组时，先取消 SSE 任务，再停 Computer（20 秒窗口），最后停 Server（5 秒窗口）。
- 每个子进程先收到 SIGTERM。窗口结束时仍在运行的子进程被强制结束。

## 4. Tauri 命令

React bridge 是 `desktop/src/bridge/collab.ts` 的 `collabCommands`。Rust adapter 是 `desktop/src-tauri/src/commands/collab.rs`，每个命令转成一个 `DesktopCommand`。结果类型在 `openwork-collab` 的 `protocol/desktop.rs` 与 `protocol/room_views.rs`。前端的协作类型只在 `bridge/collab.ts` 与 `bridge/collabEvents.ts` 中定义，组件与 store 从这两个模块导入。

### 4.1 Runtime 与 Agent

```text
collab_status
collab_agent_list
collab_agent_create
collab_agent_update
collab_agent_agenda_set
collab_agent_archive
collab_agent_restore
```

`collab_status` 返回 `RuntimeStatusView`：`runtimeSessionId`、`startedAt`、`lastComputerHeartbeat`、`engines`、`engineReadiness` 与 `runners`。

Agent 命令都返回 `AgentView`：profile 与配置字段，另加当前状态 `activity`。`activity` 在 `server/activity.rs` 中计算：

| `activity.kind` | 附带 | 来源 |
|---|---|---|
| `working` | `roomId`、`roomTitle`、`cardId`、`cardTitle`（都可空）、`startedAt` | 该 Agent 最近开始的 running Run。房间取 Run 的 `room_id`。卡片取 Run 的 `focus_card_id`，没有时取绑定到这个 Run 的第一条卡片唤醒 |
| `queued` | `cardCount`、`firstCardTitle` | 未结算的 `collab_card_wakes`，按写入先后取第一张 |
| `error` | `message` | Runner heartbeat 的 `error` 与 `lastError` |
| `idle` | `roomId`、`roomTitle`、`lastSpokeAt`（都可空） | 该 Agent 最近一条 normal 消息 |
| `archived` | — | profile 的 `archivedAt` |

多种条件同时满足时，按这个顺序取第一种：归档 → 工作中 → 出错 → 排队 → 空闲。时间是带 `+08:00` 的 RFC 3339。私聊房间没有 `title`，所以 `roomTitle` 对私聊房间为空。

限流、未登录等 Runner 退避不上报（collaboration.md §5）。退避期间，Agent 按其他条件显示，通常是排队或空闲。理由见 [Agent Note：不上报 Runner 暂停](../../.agents/notes/implemented/simplification/2026-09-25-no-runner-pause-reporting.md)。

### 4.2 Room 与 Message

```text
collab_room_list
collab_direct_room_create
collab_group_room_create
collab_room_member_list
collab_group_member_add
collab_group_member_remove
collab_room_open
collab_room_pin               { roomId, pinned }
collab_message_send
collab_room_viewed            { roomId, upToSeq }
```

- `collab_room_list` 返回全部房间的 `RoomSummaryView`，按最近消息时间从新到旧排列，没有消息的房间按创建时间。字段如下：
  - `id`、`kind`、`title`：私聊的 `title` 是对方的显示名；
  - `unreadCount`：sequence 大于 `user_viewed_seq`、作者不是 `local-user` 的 normal 消息数；
  - `lastMessage`：最近一条 normal 消息的作者显示名与正文前 80 字；`lastMessageAt`；
  - `userIsMember`；`memberIds`：用户在最前，其余按 ID 排序；`pinned`。
- `collab_room_open` 返回 `RoomSnapshotView { roomId, messages, notes }`。`messages` 是房间的全部消息，按 sequence 排列，没有分页。成员变动写入的系统消息也在其中，作者是 `local-user`。
- `RoomMessageView` 平铺 `MessageView` 的字段，另加 `authorName`、`authorKind`、`authorRole` 与 `createdAt`。`quoted` 可空，形如 `{ id, authorId, authorName, body }`，`body` 是原文前 180 字。
- `notes` 是说明行（§7.3）。房间成员与其当前状态不在快照里，前端从 `memberIds` 与 Agent 列表的 `activity` 得出。
- `collab_message_send` 带可选的 `quotedMessageId`，返回 `MessageView`。
- `collab_room_viewed` 把 `user_viewed_seq` 更新为 `GREATEST(原值, LEAST(upToSeq, next_seq))`，并返回记录后的值。这个值只增不减。负数被拒绝。
- `collab_room_pin` 写入 `collab_rooms.user_pinned_at`。已经置顶时，不改置顶时间。房间不存在时返回 `NOT_FOUND`。
- 创建私聊的语义是“创建或返回已有房间”。创建群组至少要两个 Agent。成员命令只接受 Agent ID；Server 始终管理 `local-user`。每次成员变动写入一条系统消息。

房间页使用专用视图，Agent 命令的 `RoomView` 与 `MessageView` 不变。理由见 [Agent Note：Desktop 房间页使用专用视图](../../.agents/notes/implemented/architecture/2026-09-25-desktop-room-views.md)。引用见 [Agent Note：引用回复](../../.agents/notes/implemented/feature/2026-09-24-message-quotes.md)。

### 4.3 Board、Column 与 Card

```text
collab_board_list                     → BoardView[]
collab_board_create                   → BoardView
collab_board_update                   → BoardView
collab_board_delete                   → 被删除的 id
collab_board_column_create            → BoardView    title 与 kind
collab_board_column_update            → BoardView    title 与 kind
collab_board_column_move              → BoardView    before_column_id 或末尾
collab_board_column_delete            → BoardView
collab_card_create                    → CardChangeView
collab_card_update                    → CardChangeView   title 与 description，都可选、至少给一个
collab_card_move                      → CardChangeView   目标列与可选的 before_card_id
collab_card_assign                    → CardChangeView
collab_card_delete                    → 被删除的 id
```

- 新建的看板有三列：`Todo`、`Doing`、`Done`，类型分别是 `todo`、`doing`、`done`。
- `BoardColumnView.kind` 取 `todo`、`doing`、`done` 或 `null`（未分类）。理由见 [Agent Note：Column 类型与领取](../../.agents/notes/implemented/architecture/2026-09-24-column-kind-and-card-claim.md)。
- `collab_board_list` 给每张卡片加上两个字段，Agent 命令返回的卡片不带它们：
  - `agentState`：负责人的 running Run 正在处理这张卡片时为 `working`；负责人对它有未结算的卡片唤醒时为 `queued`；否则不发这个字段；
  - `updatedAt`：最近更新时间，带 `+08:00`。
- `CardChangeView` 是 `{ card, wokenAgentIds }`：修改后的卡片与这次叫醒的 Agent。
- `collab_card_update` 的 `description` 写空字符串时清空描述。两者都不给时返回 `INVALID_ARGUMENT`。
- Desktop 的卡片命令以 `local-user` 的身份写入卡片唤醒，不受每分钟限额约束（collaboration.md §11.2、§11.4）。

Agent 创建、领取、更新、移动 Card 时使用 Agent command，不使用这些 Desktop command。

### 4.4 运行记录

```text
collab_run_list     { agentId, status, limit }  → RunSummaryView[]
collab_run_trace    { runId }                   → RunTraceView { run, events }
```

- `collab_run_list` 按开始时间从新到旧排列。Server 把 `limit` 限制在 1–200，前端发 100。
- 时间线合并四个来源：Run 的开始与结束、`collab_run_events`、`collab_triages` 与 `collab_command_requests`。事件按时间排序。

| 事件 `kind` | 来源 |
|---|---|
| `run.opened` | Run 开始，带 trigger、房间、焦点卡片与收件数 |
| `triage.started` | Runner 上报 |
| `triage.completed` | `collab_triages`，带 `actionable`、`source`、原因、模型与 token |
| `engine.started`、`engine.completed`、`engine.failed`、`engine.cancelled` | Engine 上报 |
| `command.completed` | `collab_command_requests` |
| `run.completed`、`run.failed`、`run.cancelled`、`run.interrupted` | Run 结束，带结果、错误与用量 |

没有单独的限流事件类型。

## 5. 事件与刷新

Tauri host 用 Desktop secret 独占 `GET /desktop/events`。收到 `InvalidationEvent` 后，它用固定事件名发给 WebView（`collab_event_bridge.rs`）：

```text
openwork://collaboration-invalidation
```

```ts
interface CollabInvalidation {
  id: string
  kind: 'runtime_ready' | 'agent_config' | 'room' | 'message' | 'board' |
        'engine_inventory' | 'runner_status' | 'agent_activity'
  subjectId: string | null
  revision: number | null
  publishedAt: number
}
```

event 只表示某类视图可能变了。**前端不把 event 拼进 store，只重新调用命令取完整视图。**`features/collab/invalidationCoordinator.ts` 持有 WebView 中唯一的订阅，按下表刷新：

| kind | 重新读取 |
|---|---|
| `runtime_ready` | Runtime 状态、Agent 列表、房间列表、当前房间的快照；在看板页时另读 Board |
| `agent_config` | Runtime 状态、Agent 列表 |
| `room` | 房间列表；当前房间是群组，且 `subjectId` 为空或等于它时，另读成员 |
| `message` | 房间列表；`subjectId` 为空或等于当前房间时，另读当前房间的快照 |
| `board` | 在房间、Agent 私聊与看板页时读 Board |
| `engine_inventory` | Runtime 状态 |
| `runner_status` | Runtime 状态、Agent 列表 |
| `agent_activity` | Agent 列表；在看板页时另读 Board |

“当前房间”在房间页是选中的房间，在 Agent 私聊页是选中的私聊。Server 在 Run 打开或结束、写入卡片唤醒时发布 `agent_activity`。

其他读取：

- 上报 `collab_room_viewed` 成功后，重新读房间列表。这个命令不发布 invalidation。
- 房间页、Agent 私聊页与看板页挂载时各读一次 Board。
- 运行记录页开着自动刷新时，每 3 秒读一次（§10）。

除运行记录页外，没有定时重读。广播积压时，Tauri host 只记一条警告。

Desktop SSE 断开后，Tauri host 自己重连。间隔从 1 秒起翻倍，上限 30 秒；连接保持 60 秒以上后，间隔重置为 1 秒。WebView 不参与凭据或连接管理。

## 6. Store

| store | 文件 | 拥有 |
|---|---|---|
| `modeStore` | `app/modeStore.ts` | workbench/collab mode |
| `collabNavigationStore` | `features/collab/collabNavigationStore.ts` | Rail view、当前房间、当前私聊、要预填到输入框的文字 |
| `roomViewStore` | `rooms/roomViewStore.ts` | 房间右侧栏的内容与开关、正在引用的消息、高亮的消息、成员管理对话框、滚到最新的信号 |
| `roomStore` | `rooms/roomStore.ts` | 房间列表、各房间成员、建群、打开私聊、置顶与成员操作 |
| `messageStore` | `rooms/messageStore.ts` | 每个房间的快照、loading 与 error；上报已看到的位置 |
| `agentStore` | `agents/agentStore.ts` | Agent 列表与 profile/配置操作 |
| `boardStore` | `boards/boardStore.ts` | Board tree、选中的看板与卡片、结构操作与卡片操作 |
| `observabilityStore` | `observability/observabilityStore.ts` | Run 列表、筛选、选中的 Run 与它的时间线 |
| `runtimeStore` | `features/collab/runtimeStore.ts` | `RuntimeStatusView` |

- Store 只保存 Server 快照与请求状态。权限、幂等、顺序、领取、HELD、triage、路由与 Agenda 由 Server 裁决。
- 每次修改后，store 重新读取 Server 的快照并整体替换，不做乐观更新。读取失败时，store 保留旧快照，并写入 `error`。
- 输入框草稿是 `Composer` 的组件状态。已上报的位置记在 `messageStore.ts` 的模块变量里。
- 派生数据放在视图模型模块里，并单独测试：`roomListModel`、`roomTimeline`、`roomWorking`、`roomViewed`、`mentionCompletion`、`messageText`、`agentStatus`、`agentPageModel` 与 `boardModel`。

## 7. 房间页

```text
┌──────┬──────────────┬──────────────────────────────────┬──────────────┐
│ Rail │ 房间列表      │ 消息区                            │ 房间右侧栏    │
│ 80   │ 272          │ 标题栏 / 消息流 / 工作条 / 输入框   │ 300，可收起   │
│      │ 可拖动 224–416 │                                  │              │
└──────┴──────────────┴──────────────────────────────────┴──────────────┘
```

标题栏包含房间名与一行摘要。群组的摘要是“群组 · 你和 N 位 Agent”，私聊是“私聊”；有 Agent 在工作时，加“N 位工作中”。右侧叠放最多 4 个成员头像，工作中的成员带外圈。最右是收起或展开右侧栏的按钮。

### 7.1 房间列表

- 顶部是标题、新建群组按钮与搜索框。活跃 Agent 少于两个时，新建群组按钮不可用。
- 搜索不区分大小写，匹配房间名、成员 ID 与成员显示名。
- 搜索框下面是四个筛选：全部、未读、Agent（用户与某个 Agent 的私聊）、群组。“未读”旁边显示有未读消息的房间数。
- 列表只含用户所在的房间，不按类型分组。置顶的房间在最前，带“置顶”小标题与分隔线。其余房间保持 Server 的顺序。
- 每行包含：
  - 头像：群组是成员头像拼成的一簇，用户排在最前；私聊是对方 Agent 的头像；
  - 标题：有未读时加粗；
  - 第二行：房间里有 Agent 在工作时，显示跳动的三点与“<名字> 正在处理…”。两人时显示“<A> 和 <B> 正在处理…”，更多时显示“<A> 等 N 人正在处理…”，按开始时间先后命名。没有人工作时，显示“作者：摘要”；
  - 右侧：最近消息时间（今天显示时刻，昨天显示“昨天”，更早显示“M月D日”），下方是未读数徽标。
- 右键菜单：置顶或取消置顶。群组还有“成员管理”。
- 当前房间用 `paper` 底色加轻阴影标出。

### 7.2 消息流

- 所有消息左对齐。每条显示头像、显示名、Agent 的 role 与时刻。用户显示为“你”。Agent 的名字用它的识别色（§11），点击头像或名字时，右侧栏显示这个 Agent 的资料（§7.5）。
- 引用：正文上方显示被引用消息的作者与原文，单行截断，带“跳到原文”。点击时，消息流滚到原消息，并高亮 1.6 秒。
- 正文用 `MarkdownRenderer` 渲染。代码块与行内代码之外的 `@<id>` 渲染成提及标签。只有 Agent 列表中的 ID 与 `all` 算提及，边界规则与 Server 的 `routing::mentions` 相同。提及标签用该 Agent 的识别色，`@all` 用 clay。
- 卡片链接：代码块与行内代码之外匹配 `card-[0-9a-f]{32}` 的 id 渲染成胶囊。
  - 胶囊显示看板图标与卡片标题，标题从已加载的 Board 中查找。卡片已删除时，胶囊只显示 id，且不可点击。
  - 消息下方为每张找得到的卡片附一张摘要卡：看板图标、“看板卡片 · `card-` 加 id 前 8 位”、标题、“<看板名> → <列名>”、负责人、多久前更新。
  - 点击胶囊或摘要卡时，右侧栏显示这张卡片的预览（§7.5），对应的胶囊与摘要卡描边变为 clay。
  - 理由见 [Agent Note：卡片链接](../../.agents/notes/implemented/architecture/2026-09-24-card-links-instead-of-board-events.md)。
- 悬停消息时显示浮动工具条：引用回复、复制。只读房间没有引用回复。
- 视口离底部不到 72px 时，消息流跟随新消息滚到底部。否则显示“回到最新消息”按钮。发送消息后，消息流滚到最新。

`collab_room_viewed` 的上报规则（`rooms/messageStore.ts`、`rooms/roomViewed.ts`）：

- 每次读到房间快照后上报。窗口获得焦点或可见性变化时，也上报当前快照。
- 只在窗口在前台时上报：`document.visibilityState` 为 `visible`，且 `document.hasFocus()`。
- `upToSeq` 取快照中最新一条消息的 sequence。只有它大于上次成功上报的值时，才上报。
- 失败时不记录，下次读到快照或回到前台时重试。

用户查看房间算作人类关注。理由见 [Agent Note：lap floor](../../.agents/notes/implemented/architecture/2026-09-24-lap-floor.md)。

### 7.3 说明行

说明行不是消息。`collab_room_open` 根据本房间的 `collab_triages` 生成说明行（`server/room_notes.rs`），前端把它插在 `afterSequence` 那条消息之后。说明行解释为什么有人没回复，不显示运行细节。

“一段对话”是一条人类消息到下一条人类消息之间的消息。成员变动的系统消息不切分对话。

| 说明行 | 条件 | 位置 | 文案 |
|---|---|---|---|
| 路由 | 有 Agent 的 triage 为 `source = routing`、`response_mode = me`，且对应的人类消息 `@` 了房间里的 Agent | 那条人类消息之后 | “Ada、Cy 判断这条是给 Bo 的，没有参与” |
| 一轮上限 | 一段对话里第一次出现 `source = lap_floor` 的 triage | triage 看到的最后一条消息之后；发言人取这条消息的作者 | “讨论已满一轮，Cy 开始第二次发言，其余 Agent 不再被叫醒。你发言后会继续。”附“为什么？”按钮，点开后显示一段解释 |
| 硬上限 | 一段对话里第一次出现 `source = loop_cap` 的 triage | 同上 | “Agent 之间已连续 20 条消息，暂停到你下次发言。” |

在同一段对话中，每类说明行最多出现一次。

### 7.4 工作条与输入框

工作条在输入框上方，高 22px。本房间有 Agent 在工作时，它显示跳动的三点、头像与一句话；没有人工作时，它不可见但不收起。所以工作条出现与消失时，消息区不跳动。文案按开始时间最早的 Agent 写：

- 一人且有卡片：“<名字> 正在处理「<卡片>」 · <用时>”；
- 一人且没有卡片：“<名字> 正在处理 · <用时>”；
- 多人：“<名字> 等 N 人正在处理 · <用时>”。

输入框由三部分组成：

- 引用条：正在引用时，显示“回复 <名字>：<原文>”与取消按钮。
- 文本框：Enter 发送，Shift+Enter 换行，输入法组字时 Enter 不发送。输入 `@` 时弹出补全。
  - 候选依次为 `@all`（注明“全员，不收窄”）和房间里未归档的 Agent（显示名 · role），按 ID 或显示名的前缀匹配。
  - 上下键选择，Enter 或 Tab 确认，Esc 关闭。
- 工具行：`@` 按钮（在光标处插入 `@`）、提示“只 @ 某人时，其他 Agent 会先判断是否与自己有关”、发送按钮。正文为空时，发送按钮不可用。

发送失败时，输入框恢复草稿与引用，并显示错误。从卡片详情“在房间中讨论”进入房间时，输入框预填文字（§9）。

### 7.5 房间右侧栏

右侧栏宽 300px，有三种内容，默认显示房间信息。点击卡片链接或摘要卡时，显示卡片预览。点击 Agent 的头像或名字时，显示 Agent 资料。关闭预览或资料后，右侧栏回到房间信息。切换房间时，右侧栏回到房间信息，引用与高亮被清除。

房间信息：

- 标题“成员 · N”，N 计入用户。群组另有“管理”入口，用来添加与移除成员。
- Agent 成员列表：头像（工作中加外圈，点击打开资料）、显示名与 `@id`、一句当前状态、状态标签。
- 状态标签与 `activity.kind` 对应：工作中（success）、已唤醒（clay）、空闲（中性）、出错（danger）、已归档（中性）。
- 底部提示：“你在看这个房间时，Agent 之间的讨论不会一轮就停；离开后，它们说完一轮就会暂停。”

卡片预览（只读）：

- 顶部是“卡片 · <看板名>”与关闭按钮。
- 内容是卡片标题、所在列与类型标记、负责人、当前状态（与看板页卡片的 `agentState` 一致）、Markdown 描述。
- 底部是“打开看板”。点击后跳到看板页，选中这个看板与这张卡片。
- 卡片已删除时，只显示“卡片已删除”。

Agent 资料：64px 头像与识别色、显示名与 `@id`、role、状态标签与一句当前状态、“私聊”按钮、“在 Agent 页编辑”按钮、persona（最多 6 行）、主模型与判断模型。“私聊”打开或创建与这个 Agent 的私聊；Agent 已归档时不可用。“在 Agent 页编辑”跳到 Agent 页。

### 7.6 Agent 私聊

这个页面只读，用于旁观 Agent 之间的私聊。它列出用户不是成员的 `direct` 房间。

- 左侧列表宽 272px。每行显示两个 Agent 的头像叠放、标题“<A> 与 <B>”、最近一条消息；最近消息在今天时，显示时刻。
- 中间是消息流，样式与 §7.2 相同，但没有引用回复。底部显示“这是 Agent 之间的私聊，你只能查看”，没有输入框。
- 右侧是与 §7.5 相同的右侧栏。
- 打开私聊时，同样上报 `collab_room_viewed`。

## 8. Agent 页

页面标题是“同事管理”。标题栏包含标题、按当前状态的计数、Engine 状态标签与“创建同事”按钮。

- 计数只算活跃 Agent。“N 位工作中”总是显示；“已唤醒”“空闲”“出错”只在数量大于 0 时显示。
- Engine 状态标签取 `engines` 中 OpenCode 的条目，见下表。沙箱自检不通过时，inventory 为 `error`（collaboration.md §3.1）。

| inventory 状态 | 标签 | 色调 |
|---|---|---|
| `ready` | “OpenCode <version> · 沙箱已启用” | success |
| `missing` | “未安装 OpenCode” | danger |
| `error` | “OpenCode 不可用：<lastError>” | danger |
| `unknown` 或没有条目 | “正在检查 OpenCode” | 中性 |

- 标签页：活跃 N / 已归档 N。
- Agent 卡片在宽窗口排三列，窄一些时排两列或一列。活跃标签页的最后一格是“创建同事”卡。每张卡片包含：
  - 头部：识别色头像（右下角带状态点，工作中加外圈）、显示名与 `@id`、role（斜体）、状态标签与“OpenCode”标签。悬停时，右上角出现编辑与归档按钮。已归档的卡片只有恢复按钮；
  - 当前状态行：工作中“处理卡片「…」· 用时”，没有卡片时“在「房间」处理 · 用时”；排队中“N 张卡片待处理：「…」”；空闲“上次在「房间」回复 · 时刻”，从未发言时“还没有发言”；
  - 出错时，状态行换成 danger 提示框，写明错误信息；
  - persona（最多 3 行）；
  - 主模型与判断模型；
  - 底部：“主动巡检（Agenda）”复选框与“私聊”按钮。已归档时，两者不可用。
- 编辑对话框可以修改显示名、role、Engine、主模型、判断模型与 persona。
  - Engine 下拉框只有 OpenCode。
  - 模型是自由输入，形式为 `provider/model`。新建时两者默认 `deepseek/deepseek-flash`。
  - 显示名、persona 与两个模型都不为空时，才能保存。
  - persona 只是用户人格部分。Computer 写入 `AGENTS.md` 时，追加代码拥有的协作契约；persona 不能移除这份契约。
- 创建时，Server 按显示名生成 slug ID。前端随后创建与这个 Agent 的私聊。
- 修改配置、切换 Agenda、归档与恢复都使 `configRevision` 加一。
- 已归档的 Agent 不在 Computer 的分配列表中（`Agents::assignments`），所以它的 Runner 停止。记录保留。

## 9. 看板页

```text
┌──────┬────────────┬──────────────────────────────────────┬──────────────┐
│ Rail │ 看板列表    │ 列                                    │ 卡片详情      │
│ 80   │ 272        │ 每列 288，圆角容器，横向排列            │ 340，选中时    │
│      │ 可拖动 220–440 │                                    │              │
└──────┴────────────┴──────────────────────────────────────┴──────────────┘
```

- 页面顶栏：标题“看板”与“创建看板”按钮。
- 看板列表：每个看板显示名称、卡片数；`done` 列有卡片时，另显示完成数。
- 看板标题栏：看板名、“N 张卡片 · N 位 Agent 在做”、“添加列”、编辑看板、删除看板。“N 位 Agent 在做”是有 `working` 卡片的不同负责人数。
- 列头：列名、卡片数、类型标记：`TODO`（描边）、`DOING`（success 底）、`DONE`（墨色底）、`未分类`（虚线描边）。悬停时显示左移、右移、编辑与删除按钮。编辑列对话框里，类型是下拉框。
- 每列底部有“添加卡片”。点开后，原地出现标题输入框：Enter 创建，Esc 取消，失焦时有内容就创建。新卡片没有描述与负责人，排在这一列末尾。创建失败时，输入框保留文字。
- 卡片可以拖到别的列，或同列的别的位置。放下时只发送目标列与 `before_card_id`；位置不变时不发命令。拖动时，目标列高亮，放置位置显示 clay 色的线。
- 卡片：标题、描述摘要（最多 2 行，`@id` 用识别色）、负责人头像与名字，以及状态：
  - `working`：“<名字> 处理中 · 用时”，头像加外圈；
  - `queued`：clay 标签“已唤醒 · 排队”；
  - 无负责人：“未分配”；其他情况显示负责人与“多久前更新”；
  - `done` 列的卡片降低不透明度。
- 卡片详情：
  - 标题与描述可以直接编辑。标题在失焦或 Enter 时保存。描述在失焦或 Cmd/Ctrl+Enter 时保存，输入 `@` 时弹出 Agent 补全；
  - 所在列与负责人下拉框。负责人可选“未分配”“你（local-user）”与活跃 Agent。从下拉框换列时，卡片放到目标列末尾；
  - 当前状态提示（`agentState`）；
  - 接手规则说明：有负责人时为“<负责人> 正在处理或排队时别的 Agent 领不走；超过 20 分钟没有更新且 <负责人> 没有在运行，才允许别人接手。”，没有时为“没有负责人时，任何 Agent 都可以领取这张卡片。”；
  - 删除卡片；
  - “在房间中讨论”：打开与负责人的私聊，输入框预填“关于卡片「标题」（card-id）：”。负责人不是 Agent 时不可用。
- 改派负责人，或在描述里新增 `@<agent-id>`，会叫醒对应的 Agent。保存后，详情里提示“已通知 <名字>”。
- Column move 与 Card move 只发送 `before_*_id` 或追加到末尾。UI 不自行保存 position，每次修改后重新读取 Board。删除冲突、非空 Column/Board 等错误由 Server 判断，显示在页面顶部。

接手规则的理由见 [Agent Note：Column 类型与领取](../../.agents/notes/implemented/architecture/2026-09-24-column-kind-and-card-claim.md)。

## 10. 运行记录页

页面在 Rail 的第五项，标题“运行观测”（`features/collab/observability/`）。它只用于观察，不提供重试、取消或编辑。理由见 [Agent Note：运行记录页](../../.agents/notes/implemented/feature/2026-09-24-run-records-page.md)。

- 左侧是 Run 列表，默认宽 360px，可拖动 280–560。顶部有两个筛选：Agent，以及状态（`running`、`completed`、`failed`、`cancelled`、`interrupted`）。
- 每行显示 Agent 名、状态、`stage`、开始时间、用时；失败时另显示错误信息。
- 列表刷新后，如果之前选中的 Run 不在列表里，就选中最新的 Run。
- 右侧是选中 Run 的详情：
  - Agent、状态、结果、Run id 与 `triggerReason`；
  - 用时、token 总数、命令数、收件数、事件数与 trigger；
  - Engine、配置模型、实际模型与开始时间；有错误时显示错误码与信息；
  - 事件时间线（§4.4）。每个事件带名称、来源、时间、摘要与结构化数据。
- 自动刷新默认开启，每 3 秒在窗口可见时读一次，可以关闭。标题栏另有手动刷新按钮。

## 11. 视觉

- 沿用工作台的设计令牌（`desktop/src/app/theme/globals.css`）：`paper` / `paper-hover` / `surface` 三层面、`ink` 系文字、`line` 边框、`clay` 唯一强调色、`status-*` 状态色。标题用衬线字体。亮色与暗色由同一组令牌驱动。
- Agent 识别色令牌 `--agent-1` … `--agent-6` 只用于头像、名字、提及标签和工作中外圈。
  - 亮色与暗色各 6 档。亮色的相对亮度为 0.07–0.14，暗色为 0.35–0.57。
  - 各档在 `paper` 与 `paper-hover` 上的对比度都不低于 4.5:1。
  - Agent 按 ID 的 FNV-1a 32 位哈希取色（`features/collab/components/agentIdentity.ts`），颜色不随列表顺序变化。`@all` 用 clay。
- clay 色的文字与图标用 `--clay-ink`（亮色 `#a24e36`，在 `paper` 上 5.2:1）。`--clay` 在 `paper` 上只有 2.8:1，只用于底色、描边与放置线。
- 协作界面中 11–12px 的说明文字用 `ink-soft`，不用 `ink-faint`。`ink-faint` 只用于占位文字、边框与装饰点。
- 图标用 lucide，不使用 emoji。

## 12. 错误与恢复

| 情况 | Desktop 表现 |
|---|---|
| 初始 Runtime 启动失败 | setup 失败，应用不进入半可用的协作状态 |
| 另一个 Desktop 已持有 Runtime | 启动失败（`AlreadyRunning`） |
| Runtime 正在成组替换 | 命令返回 `collaboration_unavailable`，message 为 “collaboration Runtime is restarting”；store 保留旧快照，并显示错误 |
| Server 业务拒绝 | 命令返回 `collaboration_unavailable`；message 带 Server 的 code 与原因 |
| SSE 断线 | Tauri host 按 §5 重连 |
| Agent 出错 | Agent 卡片与房间成员列表显示错误信息，其他 Agent 不受影响 |
| Engine missing/error | Agent 页标题栏的 Engine 标签显示原因 |
| Desktop 正常退出 | 等待协作进程组停止，然后退出（§3.3） |

所有协作命令的错误都映射为 `CommandError { code: 'collaboration_unavailable', message }`（`desktop/src-tauri/src/error.rs`）。前端只显示 message。

## 13. 验收

编号沿用原设计文档的验收清单，测试注释按这些编号引用。Rust 测试路径相对 `crates/`，Tauri 与前端测试写出仓库路径，前端另写用例名。

`openwork-collab/tests/` 中的测试需要 `TEST_DATABASE_URL`，没有时直接返回。`collab_supervisor.rs` 另外只在 macOS 上编译，并需要 Redis（`TEST_REDIS_URL`，默认 `redis://127.0.0.1:6379/15`）。

1. setup 完成后，再读取 managed collaboration state。
   - 测试：`desktop/src-tauri/src/lib.rs::tests::managed_state_is_read_after_setup_runs`
2. child 的 ready metadata 无效时，启动失败，且 runtime 目录为空。
   - 测试：`desktop/src-tauri/src/collab_client.rs::tests::invalid_server_ready_fails_startup_and_removes_the_runtime_root`；`desktop/src-tauri/src/collab_client.rs::tests::invalid_computer_ready_stops_server_and_removes_the_runtime_root`
   - 缺口：两者直接调用 `start_group`，不经过 Tauri setup。
3. 忽略 SIGTERM 的 child 在窗口结束后被强制结束。
   - 测试：`desktop/src-tauri/src/collab_client.rs::tests::bounded_stop_force_kills_a_child_that_ignores_sigterm`
4. Desktop SSE 的有限响应流关闭后，能重新连接，并继续投影 invalidation。
   - 测试：`desktop/src-tauri/src/collab_client.rs::tests::desktop_sse_reconnects_after_the_stream_closes`
   - 缺口：测试只断言事件进入广播，没有覆盖 `collab_event_bridge.rs` 发出 Tauri event。
5. 真实 supervisor 测试分别杀死 Server 和 Computer，两个 PID 与 RuntimeSession 都整体更换。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`
6. 每次替换后，fake OpenCode 仍能通过真实 shim 发布 durable reply。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`
7. 正常 shutdown 后，没有协作 child，当前 runtime 目录为空。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`
8. Desktop shutdown 后，PostgreSQL/Redis 仍可连接。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`
9. React bridge 参数与 Rust command DTO 一致，契约测试覆盖新增字段。运行记录页在普通导航中，列表可按 Agent 与状态筛选，选中 Run 后显示事件时间线。
   - 测试：`desktop/src/bridge/collab.test.ts` 的全部用例，例如 › "maps run observability filters and trace reads to dedicated commands"；`openwork-collab/src/protocol/desktop.rs::tests::agent_activity_serializes_as_a_kind_tagged_camel_case_object`；`openwork-collab/src/protocol/desktop.rs::tests::card_agent_state_is_omitted_unless_the_desktop_computed_it`；`openwork-collab/src/protocol/room_views.rs::tests::room_messages_flatten_and_notes_are_kind_tagged`；`openwork-collab/src/protocol/room_views.rs::tests::room_summaries_use_camel_case_fields`；`desktop/src/features/collab/observability/observabilityStore.test.ts` › "selects the newest run and loads its complete trace"、› "passes filters to the Server-owned run projection"
   - 缺口：前端测试只断言 `invoke` 的参数，Rust 测试只断言 serde 形状，没有测试把两边连起来。`CollabRail` 与 `ObservabilityPage` 没有渲染测试。
10. 未读数与 `collab_room_viewed`：打开房间后未读归零；窗口不在前台时不上报，回到前台时补报；`user_viewed_seq` 不回退。
    - 测试：`openwork-collab/tests/room_page.rs::acc_15_room_list_carries_unread_last_message_members_and_pin`（上报后未读为 0）；`openwork-collab/tests/messaging.rs::acc_10_a_second_lap_is_skipped_until_the_user_looks_again`（上报 99 记为 5，再报 2 仍为 5）；`desktop/src/features/collab/rooms/messageStore.test.ts` › "reports what the room already shows once the window comes back to the foreground"、› "keeps retrying when the report fails"；`desktop/src/features/collab/rooms/roomViewed.test.ts` 的三个用例
11. 说明行：路由、一轮上限、硬上限各按 §7.3 的条件出现，同一段对话中每类最多一次。
    - 测试：`openwork-collab/src/server/room_notes.rs::tests::acc_11_routing_names_who_stepped_aside_and_who_was_addressed`；`openwork-collab/src/server/room_notes.rs::tests::routing_without_an_addressed_agent_has_no_note`；`openwork-collab/src/server/room_notes.rs::tests::acc_11_caps_appear_once_per_conversation_segment`；`openwork-collab/tests/room_page.rs::acc_12_room_snapshot_carries_authors_quotes_and_notes`（系统消息不切分对话）；`desktop/src/features/collab/rooms/roomTimeline.test.ts` › "places each note right after the message it explains"；`desktop/src/features/collab/rooms/roomPage.render.test.tsx` › "explains who stepped aside for a routed message"
    - 缺口：一轮上限与硬上限的渲染、“为什么？”按钮没有测试。
12. 引用回复：发送时带 `quotedMessageId`；消息与输入框正确显示引用；点击引用跳回原消息。`@` 补全候选为 `@all` 与房间内 Agent。卡片链接：代码中的 id 不渲染；已删除的卡片不可点击；消息下方有摘要卡；点击胶囊或摘要卡后，右侧栏显示该卡片预览；“打开看板”选中这张卡片。点击头像显示 Agent 资料。工作条出现与消失时，消息区不跳动。
    - 测试：`desktop/src/bridge/collab.test.ts` › "sends the quoted message id with a quote reply"；`desktop/src/features/collab/rooms/roomPage.render.test.tsx` › "shows the author, role, clock, a jumpable quote, the card summary and a quote action"、› "shows who and what is being quoted, with a cancel button"、› "renders mentions and card chips only outside code, and deleted cards as plain ids"、› "keeps its height when nobody works and names the card when someone does"；`desktop/src/features/collab/rooms/mentionCompletion.test.ts` › "offers @all first, then active room Agents matching id or name"；`desktop/src/features/collab/rooms/messageText.test.ts` › "lists card ids once in order and skips code"；`desktop/src/features/collab/rooms/roomViewStore.test.ts` › "switches the side panel between room info, a card and an Agent, and closes back to info"；`openwork-collab/tests/room_page.rs::acc_12_room_snapshot_carries_authors_quotes_and_notes`
    - 缺口：点击引用后的滚动与高亮、“打开看板”调用 `focusCard`、点击胶囊与头像的事件接线都没有测试；右侧栏切换只在 store 层测试。
13. Agent `activity` 的五种状态与卡片 `agentState` 的两种状态各有渲染测试。
    - 测试：`desktop/src/features/collab/agents/agentCard.render.test.tsx` › "shows the card being worked on with elapsed time"、› "shows the queued cards"、› "shows where an idle Agent last spoke"、› "shows an error in a danger alert"、› "offers restore instead of edit for an archived Agent and disables chat and Agenda"；`desktop/src/features/collab/boards/boardPage.render.test.tsx` › "shows who is working on it and for how long"、› "shows a queued wake"；`desktop/src/features/collab/components/agentStatus.test.ts` 的全部用例；`openwork-collab/src/server/activity.rs::tests::activity_takes_the_first_matching_state_in_the_documented_order`；`openwork-collab/src/server/activity.rs::tests::card_state_prefers_working_over_a_pending_card_wake`；`openwork-collab/tests/agent_activity.rs::agent_activity_follows_runs_card_wakes_and_runner_heartbeats`
14. Column 类型下拉与列头标记；在列底部创建卡片、编辑标题与描述、拖动换列与同列重排；改派与新增 `@` 叫醒对应 Agent。
    - 测试：`desktop/src/features/collab/boards/boardPage.render.test.tsx` › "marks todo, doing, done and unsorted columns"、› "starts as an add button at the bottom of the column"；`desktop/src/features/collab/boards/boardModel.test.ts` › "names the card the dragged one lands in front of, skipping the dragged card itself"、› "detects drops that leave the card where it is"、› "counts the cards whose middle is above the pointer"；`desktop/src/bridge/collab.test.ts` › "maps Desktop card edits to explicit card commands"、› "maps R6 Board structure and Card ownership to Desktop-only commands"；`openwork-collab/tests/desktop_cards.rs::acc_14_desktop_creates_edits_and_moves_cards_and_wakes_who_it_names`；`openwork-collab/tests/card_wakes.rs::acc_14_real_changes_wake_once_and_merge_until_a_successful_run_settles_them`（Desktop 改派写入 `assigned` 唤醒）
    - 缺口：编辑列对话框的类型下拉、`AddCardInline` 的 Enter/Esc/失焦、卡片详情的保存与“已通知”提示都没有测试。
15. 房间列表照 §7.1：平铺、置顶、四个筛选、群组头像拼图、“正在处理”行。Agent 之间的房间只出现在“Agent 私聊”页，且只读。
    - 测试：`desktop/src/features/collab/rooms/roomListModel.test.ts` 的全部用例；`desktop/src/features/collab/rooms/roomPage.render.test.tsx` › "shows who is working instead of the last message, with time and unread count"、› "offers no quote reply in a read-only room"；`desktop/src/features/collab/rooms/roomWorking.test.ts` 的全部用例；`openwork-collab/tests/room_page.rs::acc_15_room_list_carries_unread_last_message_members_and_pin`
    - 缺口：`WhispersPage` 没有输入框这一点没有渲染测试。
16. Room/Agent/Board store 的 loading/error 不承载业务真相；三种语言的文案键结构一致。
    - 测试：`desktop/src/i18n/i18n.test.ts` › "keeps all locale resources on the same key structure"；`desktop/src/features/collab/runtimeStore.test.ts` › "replaces the runtime projection with the canonical Server snapshot"；`desktop/src/features/collab/rooms/messageStore.test.ts` › "keeps the room snapshot as returned by the Server"
    - 缺口：`roomStore`、`agentStore`、`boardStore` 的失败路径没有测试。手动：读这三个 store，确认 `catch` 只写 `error`，不改快照。
