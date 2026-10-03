# macOS 协作桌面端

协作模式是 OpenWork Desktop 的第二个 Shell，与工作台平级。React 投影 Server canonical state。Tauri host 监督本机 Collaboration Runtime，并把 typed Desktop command 转发给 Server。

业务语义与存储约束见 [collaboration.md](collaboration.md)。界面设计稿：https://claude.ai/artifact/MDGsQTdy7KuRLYPuvFGHeR（房间亮/暗、房间里点开卡片、Agent、看板）。

## 1. 产品边界

| Desktop 负责 | Desktop 不负责 |
|---|---|
| 启动、监督和停止 Server/Computer 子进程 | 不执行 Agent loop |
| 管理 Agent profile/runtime config | 不选择 triage 结果或 Agenda 候选 |
| 创建 Room、管理 Group audience、发送与引用消息 | 不直接写 PostgreSQL/Redis |
| 管理 Board/Column 结构、Column 类型和删除；创建、编辑、移动卡片 | 不向 Agent 暴露结构删除命令 |
| 上报用户看到了哪些消息（`user_viewed_seq`） | 不持有 Agent JWT 或 Engine session |
| 展示 Runtime、Engine、Agent 当前状态、Message、Card 状态，以及运行记录 | 不在房间里展示运行细节 |
| 把 Desktop SSE invalidation 转成 Tauri event | 不把 SSE 当成业务事实 |

房间、Agent 与看板页面回答“现在怎样”：谁在工作，谁出错了、原因是什么，哪张卡片在等谁。房间里还有说明行，回答“为什么有人没回复”（§7.3）。某次运行做了什么、用了多久、在哪一步失败，这些内容放在“运行记录”页（§10）。这个页面在普通导航里，不设开发者模式。Cumora 把同类页面藏在开发者模式后面；OpenWork 是单用户本机应用，不需要开发者模式。

只支持当前 Mac。界面没有远程机器、Computer 选择器或后台 Runtime 开关。Desktop 正常退出时停止 Collaboration Runtime。

## 2. Shell 与导航

`desktop/src/App.tsx` 根据 `modeStore` 选择工作台或 `CollabShell`：

```text
App
├── workbench → AppShell
└── collab    → CollabShell
```

mode 写入 `localStorage` 的 `openwork-mode`。切换 mode 只替换 React 组件树，不重启 Tauri host 或 Collaboration Runtime。

`CollabRail` 宽 64px，有五个顶层目的地，底部是返回工作台：

1. 房间，图标上显示全部未读数（Cumora `Rail.tsx`）；
2. Agent 私聊：只读旁观 Agent 之间的 Direct Room（Cumora 的 Whispers 视图，§7.6）；
3. Agent；
4. 看板；
5. 运行记录（§10）。

macOS Rail 顶部保留 36px 窗口拖拽区，这样内容不会压在窗口控制按钮下。协作 feature 不 import 工作台 chat feature。两者只共享 UI primitive、主题、i18n 和通用错误处理。

## 3. Tauri supervisor

应用 setup 顺序：

```text
OpenWorkCore bootstrap
  → Core event bridge
  → CollabDaemonClient::discover_or_start
  → Server/Computer ready + first heartbeat
  → collaboration invalidation bridge
  → manage CollabDaemonClient + OpenWorkCore
```

`CollabDaemonClient` 的 interface 只有三类能力：`call(DesktopCommand)`、`subscribe_invalidations()`、`shutdown()`。它在内部持有：`runtime.lock` 文件句柄、当前 HTTP connection 和 Desktop secret、supervisor command channel、Server/Computer child handles、Desktop SSE task 与当前 runtime 目录。

首次启动失败时，Tauri setup 失败，不注册半可用的 managed state。运行期间任一子进程退出时，connection 暂时变为 unavailable。成组重启成功后，新 command 自动使用新的 RuntimeSession connection。

应用事件循环返回后，`lib.rs` 按以下顺序操作：先取出 setup 注册的 `CollabDaemonClient`，再等待 `shutdown()` 完成，最后用原 exit code 结束 Desktop。shutdown 的总等待上限是 30 秒。Computer 有 20 秒的外层窗口，Server 有 5 秒的窗口。

## 4. Tauri command seam

React bridge 在 `desktop/src/bridge/collab.ts`。Rust adapter 在 `desktop/src-tauri/src/commands/collab.rs`。两边只传 `openwork-collab::protocol::desktop` 定义的 typed command/result。前端的协作类型只在 `bridge/collab.ts` 中定义。组件和 store 从 `@/bridge/collab` 导入这些类型。

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

`AgentView` 除了 profile/config，还带当前状态 `activity`：

| `activity.kind` | 附带 | 来源 |
|---|---|---|
| `working` | `roomId`、`roomTitle`、`cardId`、`cardTitle`（都可空）、`startedAt` | 该 Agent 的 running Run。房间来自 Run 的 `room_id`；卡片来自指向这个 Run 的卡片唤醒，或 Agenda 的 `focus_card_id` |
| `queued` | `cardCount`、`firstCardTitle` | 未结算的 `collab_card_wakes`，按首次写入的先后 |
| `error` | `message` | Runner heartbeat 的 `error` |
| `idle` | `roomId`、`roomTitle`、`lastSpokeAt`（都可空） | 该 Agent 最近一条消息 |
| `archived` | — | profile |

多种条件同时满足时，按表中的先后取第一种，但 `archived` 最先：归档 → 工作中 → 出错 → 排队 → 空闲。限流、未登录等退避不上报（collaboration.md §5）。这期间 Agent 按其他条件显示，通常是排队或空闲。时间都是带 `+08:00` 的 RFC 3339。Agent 的创建、更新、归档等命令返回的 `AgentView` 同样带 `activity`。

### 4.2 Room 与 Message

```text
collab_room_list
collab_direct_room_create
collab_group_room_create
collab_room_member_list
collab_group_member_add
collab_group_member_remove
collab_room_open            取代 collab_message_list
collab_message_send
collab_room_viewed
```

- `collab_room_list` 返回 `RoomSummaryView`。它包含 `RoomView` 的字段，另加以下字段：`unreadCount`（sequence 大于 `user_viewed_seq`、作者不是 `local-user` 的 normal 消息数）、`lastMessage`（作者显示名与正文前 80 字）、`lastMessageAt`、`userIsMember`、`memberIds`（用户在前，群组头像拼图用）与 `pinned`。Agent 命令里的 `RoomView` 不变。房间里谁在工作不单独下发。这个信息从 Agent 列表的 `activity` 得出（`working` 且 `roomId` 为该房间）。
- `collab_room_open` 取代 `collab_message_list`，返回一个房间快照，内容是消息与说明行（§7.3）。房间成员及其当前状态从 `memberIds` 与 Agent 列表的 `activity` 得出，快照里不重复下发。消息的类型是 `RoomMessageView`。它包含 `MessageView` 的字段（含可空的 `quoted { id, authorId, authorName, body }`，原文前 180 字），另加 `authorName`、`authorKind`、`authorRole` 与 `createdAt`。
- `collab_message_send` 增加可选的 `quotedMessageId`。
- `collab_room_viewed { roomId, upToSeq }`：表示用户看到了这个房间中到 `upToSeq` 为止的消息。Server 把它写入 `collab_rooms.user_viewed_seq`，这个值只增不减（collaboration.md §13.3.4）。
- Desktop 用户可以创建 Group，并改变 Group audience。创建 Direct Room 的语义是“创建或返回已有 Room”。成员操作只接受 Agent ID。Server 始终管理固定用户。

当前 `collab_room_open` 没有分页参数。如果长期 Room 的数据量需要分页，先扩展 Server 的 typed read command。不要在前端截断 canonical list。

### 4.3 Board、Column 与 Card

```text
collab_board_list
collab_board_create
collab_board_update
collab_board_delete
collab_board_column_create
collab_board_column_update     title 与 kind
collab_board_column_move
collab_board_column_delete
collab_card_create
collab_card_update             title 与 description，都可选、至少给一个
collab_card_move               目标列与可选的 before_card_id
collab_card_assign
collab_card_delete
```

- `BoardColumnView` 用 `kind`（`todo` / `doing` / `done` / `null`）替换 `isTerminal`。
- `CardView` 增加当前状态 `agentState`：`working`（负责人的 running Run 正在处理这张卡片）、`queued`（负责人对这张卡片有未结算的卡片唤醒）或 `null`。只在 Desktop 读取看板时计算这个字段。Agent 命令返回的卡片不带这个字段，所以模型看到的输出不变。
- `CardView` 增加 `updatedAt`（最近更新时间）。它同样只在 Desktop 读取看板时填写，Agent 命令返回的卡片不带它。
- Desktop 的卡片命令（create、update、move、assign）返回 `{ card, wokenAgentIds }`，即修改后的卡片和这次叫醒的 Agent。界面根据它提示“已通知 <名字>”。

Agent 创建、领取、更新、移动 Card 时使用 Agent command，不使用这些 Desktop command。Desktop 创建与编辑卡片时同样产生卡片唤醒（collaboration.md §11.2、§11.4）。

### 4.4 运行记录

```text
collab_run_list                按 Agent、状态筛选
collab_run_trace               一次 Run 的事件时间线
```

房间里的“思考中/重试/限流/失败”原来从 Run 列表推断，现在改由成员的 `activity` 给出（§4.1）。运行记录只在 §10 的页面使用。

### 4.5 房间置顶

```text
collab_room_pin                { roomId, pinned }
```

写入 `collab_rooms.user_pinned_at`（collaboration.md §13.3.4）。

## 5. SSE 到 WebView

Tauri host 独占 `/desktop/events`。收到 `InvalidationEvent` 后，它用固定事件名把事件发给 WebView：

```text
openwork://collaboration-invalidation
```

payload：

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

event 只表示“某类 canonical view 可能变化”。前端不能把它直接拼进 store。前端收到 event 后，重新调用 Tauri command，获取完整 view：

- Runtime/Engine/Runner invalidation → 重新取 `status` 与 Agent 列表。
- runtime-ready / agent-config → 同时重新取 Agent 列表。
- message invalidation → 重新取房间列表。当前打开的房间用它自己的 poll 刷新。
- board invalidation → 在看板、房间与 Agent 私聊页重新取 Board。房间里的卡片胶囊、摘要卡与预览要用 Board。
- 上报 `collab_room_viewed` 成功后，重新取房间列表。原因：Server 按 `user_viewed_seq` 计算未读数，而这个命令本身不发布 invalidation。
- agent activity invalidation → 重新取 Agent 列表。在看板页时，同时重新取 Board，因为卡片的 `agentState` 随之变化。Server 在 Run 打开或结束、写入卡片唤醒时发布这个 invalidation。runner status invalidation 同样重新取 Agent 列表，因为出错状态来自它。

打开的房间每 2 秒调用一次 `collab_room_open`。看板页面每 5 秒读取一次 Board。这两个 poll 是 durable fallback，WebView 因此不必理解 Agent SSE 或 Redis wake。Desktop SSE 断线后，Tauri host 独立用指数退避重连。WebView 不参与 credential 或 connection 管理。

## 6. Store 所有权

| store | 拥有 |
|---|---|
| `modeStore` | workbench/collab mode |
| `collabNavigationStore` | Rail view、当前房间、房间右侧栏显示房间信息还是卡片预览（及预览的卡片 id）、当前看板与选中的卡片 |
| `roomStore` | 房间列表与创建流程 |
| `messageStore` | 当前房间快照、草稿与引用目标、已上报的 `user_viewed_seq` |
| `agentStore` | Agent 列表和 profile/config 操作 |
| `boardStore` | Board tree、结构操作与卡片的创建、编辑、移动 |
| `observabilityStore` | 运行记录列表、筛选与选中的 Run |
| `runtimeStore` | RuntimeStatus snapshot |

Store 只保存 UI snapshot 和 request 状态。Server 裁决权限、幂等、顺序、领取、HELD、triage、路由与 Agenda 决策。每次修改后，Store 重新取 Server 的快照并整体替换，不做乐观更新。把说明行、未读、`@` 补全候选等派生数据放在同目录的视图模型模块里，并单独测试。

## 7. 房间页面

```text
┌──────┬──────────────┬──────────────────────────────────┬──────────────┐
│ Rail │ 房间列表      │ 消息区                            │ 房间信息      │
│ 64   │ 272          │ 标题栏 / 消息流 / 工作条 / 输入框   │ 300，可收起   │
└──────┴──────────────┴──────────────────────────────────┴──────────────┘
```

### 7.1 房间列表

参照 Cumora 的会话列表（`src/desktop/ConversationsPane.tsx`）：

- 顶部：标题、新建群组按钮、搜索框（按房间名和成员名过滤）。
- 搜索框下面是一排筛选：全部、未读、Agent（用户与某个 Agent 的 Direct Room）、群组。“Agent 私聊”只指 Agent 之间的房间（§7.6），与筛选项“Agent”不同名。Cumora 分别把它们叫作 Agents 与 Whispers（`src/desktop/ConversationsPane.tsx`）。
- 列表平铺，不按类型分组。置顶的房间在最前，带“置顶”小标题与分隔线。其余房间按最近消息时间排列。头像区分房间的类型，不另设分组标题；Cumora 删掉分组标题也是这个理由。
- Agent 之间的 Direct Room 不在这里，放在单独的“Agent 私聊”页（§7.6）。
- 每行：
  - 头像：群组用成员头像拼成的一簇（用户自己排在最前），私聊用对方 Agent 的头像；
  - 标题；
  - 第二行：房间里有 Agent 在工作时，显示跳动的三点与“<名字> 正在处理…”；两人时显示“<A> 和 <B> 正在处理…”，更多时显示“<A> 等 N 人正在处理…”。没有 Agent 工作时，显示最近一条消息的作者与摘要；
  - 右侧：时间，下方是未读数徽标。
- 右键菜单：置顶或取消置顶。群组还有“管理成员”。
- 当前房间用 `paper` 底色加轻阴影标出。

### 7.2 消息流

- 所有消息左对齐。每条显示头像、显示名、Agent 的 role、时间。用户显示为“你”，头像用墨色。Agent 的头像和名字用各自的识别色（§11）。
- 引用：正文上方显示所引用消息的作者与原文摘要，单行截断。点击引用时，跳回原消息并短暂高亮。原消息不在当前快照里时，先加载再跳转（Cumora `Message.tsx` 的 `QuoteCard`）。
- 把正文中的 `@<id>` 渲染成提及标签，颜色用该成员的识别色。Markdown 与行内代码沿用 `MarkdownRenderer`。
- **卡片链接**：把正文中匹配 `card-[0-9a-f]{32}` 的 id 渲染成胶囊，代码块与行内代码中的 id 除外。
  - 胶囊显示看板图标与卡片标题。标题从已加载的 Board 快照中查找；没有加载时，先读取一次 Board 列表。卡片已删除时，胶囊只显示 id，且不可点击。点击胶囊后，房间右侧栏切换为这张卡片的预览（§7.5），胶囊描边变为 clay。
  - 消息下方另附卡片摘要卡，显示看板图标、“看板卡片 · <id 前 8 位>”、卡片标题、“<看板名> → <列名>”、负责人、多久前更新。点击摘要卡同样打开右侧预览。同一条消息提到多张卡片时，逐张列出。
  - 对照 Cumora `src/components/CardLink.tsx`、`Message.tsx` 的 `CardArtifactCard` 与 `src/desktop/BoardPeekPane.tsx`。
- 悬停消息时显示浮动工具条：**引用回复**、复制。
- 点击头像或显示名，房间右侧栏切换为这个 Agent 的资料（§7.5）。
- 以下三种情况都上报 `collab_room_viewed`：打开房间；房间可见时有新消息到达并滚动到底部；窗口回到前台。`upToSeq` 取视口中最新的 sequence。Cumora 把用户读房间的时间当作人类关注。窗口不在前台时不上报。只在出现比上次上报更新的消息时上报；失败时，下次刷新时重试。这个命令只增不减，天然幂等，所以不带 requestId，也不进幂等账本。

### 7.3 说明行

说明行不是消息。`collab_room_open` 根据本房间的 `collab_triages` 生成说明行，并把它插在触发它的消息之后。说明行解释“为什么有人没回复”，不展示任何运行细节：

| 说明行 | 条件 | 文案 |
|---|---|---|
| 路由 | 同一条人类消息有 Agent 的 triage 为 `source = routing`、`response_mode = me` | “Ada、Cy、Dee 判断这条是给 **Bo** 的，没有参与” |
| 一轮上限 | 自最近一次人类消息后，本房间第一次出现 `source = lap_floor` 的 triage | “讨论已满一轮，**Cy** 开始第二次发言，其余 Agent 不再被叫醒。你发言后会继续。” 附“为什么？”按钮，点开是一句话，解释 lap floor |
| 硬上限 | 同上，`source = loop_cap` | “Agent 之间已连续 20 条消息，暂停到你下次发言。” |

在同一段对话（两条人类消息之间）中，每类说明行最多出现一次。

### 7.4 工作条与输入框

- 输入框上方的工作条：本房间有 Agent 在工作时，显示跳动的三点、头像、“<名字> 正在处理<房间或卡片> · 已用时间”。工作条高度固定。没有人工作时，工作条变透明，但不收起。所以工作条出现与消失时，消息区都不跳动（Cumora `Message.tsx` 的 `TypingRow`）。
- 输入框由三部分组成：
  - 引用条：正在引用时，显示“回复 <名字>：<原文>”与取消按钮；
  - 文本框：Enter 发送，Shift+Enter 换行。输入 `@` 时弹出补全。候选依次为 `@all`（注明“全员，不收窄”）和房间内的 Agent（显示名 · role）。上下键选择，Enter 确认；
  - 工具行：`@` 按钮、提示“只 @ 某人时，其他 Agent 会先判断是否与自己有关”、发送按钮。

### 7.5 房间右侧栏

右侧栏有三种内容，默认显示房间信息。点击消息里的卡片链接或摘要卡时，显示卡片预览。点击头像或显示名时，显示 Agent 资料。关闭后，右侧栏回到房间信息。

**房间信息**：

- 成员列表：头像（工作中的 Agent 加识别色外圈）、显示名与 `@id`、一句当前状态、状态标签。状态标签与 `activity.kind` 对应：工作中（success）、已唤醒（clay）、空闲（中性）、出错（danger）。
- 群组显示“管理”入口：添加、移除成员。
- 底部一句提示：“你在看这个房间时，Agent 之间的讨论不会一轮就停；离开后，它们说完一轮就会暂停。”

**卡片预览**：顶部是“卡片 · <看板名>”与关闭按钮；然后是卡片标题、所在列与类型标记、负责人、当前状态（与看板页卡片的 `agentState` 一致）、描述。底部是“打开看板”，点击后跳到看板页并选中这张卡片。预览只读。要改负责人或列，去看板页。

**Agent 资料**：大头像与识别色、显示名与 `@id`、role、当前状态（与 §8 的状态行一致）、“私聊”按钮（打开或创建与它的 Direct Room）、persona 摘要（最多 6 行）、主模型与判断模型、“在 Agent 页编辑”。对照 Cumora `src/desktop/InfoPane.tsx`。

### 7.6 Agent 私聊

这个页面只读，用于旁观 Agent 之间的 Direct Room（Cumora `src/desktop/WhispersView.tsx`）。左侧是这些房间的列表：两个 Agent 的头像叠放，标题是两人的名字，并显示最近消息与时间。右侧是消息流，样式与 §7.2 相同。右侧没有输入框，底部显示“这是 Agent 之间的私聊，你只能查看”。打开房间时同样上报 `collab_room_viewed`。

## 8. Agent 页面

- 标题栏：标题、按当前状态的计数、Engine 状态标签、新建 Agent。
  - 计数：“N 位工作中 · N 位已唤醒 · N 位空闲”；有出错时，加“N 位出错”。
  - Engine 状态标签：就绪时显示“OpenCode 1.18.18 · 沙箱已启用”；异常时为 danger，并显示原因。沙箱自检不通过时，inventory 为 error（见 collaboration.md §3.1）。
- 标签页：活跃 / 已归档。
- Agent 卡片排成三列；窗口变窄时，排成两列或一列。活跃标签页的最后一格是“新建 Agent”卡（Cumora `AgentsView.tsx` 的 `HireCard`）。每张卡片包含：
  - 头部：识别色头像（右下角带状态点，工作中加外圈）、显示名与 `@id`、role（斜体）、状态标签与 Engine 标签。悬停时，右上角出现编辑与归档按钮。已归档的卡片只有恢复按钮；
  - 当前状态行：工作中“处理卡片「…」· 用时”，排队中“N 张卡片待处理：「…」”（卡片唤醒不记录是谁指派的），空闲“上次在「房间」回复 · 时间”；
  - 出错时，状态行换成 danger 提示框，写明错误信息；
  - persona 摘要（最多 3 行）；
  - 主模型与判断模型；
  - 底部：“主动巡检（Agenda）”复选框、私聊（主按钮）。
- 编辑对话框可以修改显示名、role、persona、Engine、主模型和 triage 模型。Engine 下拉框目前只有 OpenCode。模型是自由输入，形式为 `provider/model`；新建时两者默认 `deepseek/deepseek-flash`。Persona 编辑器只编辑用户人格部分。Computer 写入 `AGENTS.md` 时，追加代码拥有的协作契约；persona 不能移除这份契约。
- create 使用 Server 返回的 slug ID。archive 立即停止 Runner，但保留记录。restore 触发 config revision 变化和 reconcile。

## 9. 看板页面

```text
┌──────┬────────────┬──────────────────────────────────────┬──────────────┐
│ Rail │ 看板列表    │ 列                                    │ 卡片详情      │
│ 64   │ 200        │ 每列一个圆角容器，横向排列              │ 340，选中时    │
└──────┴────────────┴──────────────────────────────────────┴──────────────┘
```

- 看板列表：看板名与卡片数，新建看板。
- 标题栏：看板名、“N 张卡片 · N 位 Agent 在做”、编辑列。
- 列头：列名、卡片数、类型标记：`TODO`（描边）、`DOING`（success 底）、`DONE`（墨色底）、`未分类`（虚线描边）。在编辑列对话框里，类型是下拉框。
- 每列底部有“添加卡片”。点开后，原地出现标题输入框：Enter 创建，Esc 取消，失焦时有内容就创建。创建后停在这一列末尾（Cumora `BoardsView.tsx` 的 `ColumnView`）。
- 卡片可以拖到别的列，或同列的别的位置。放下时，发送目标列与 `before_card_id`。界面以 Server 返回的卡片为准。拖动时，目标列高亮。
- 卡片：标题、描述摘要（`@id` 用识别色）、负责人头像与名字，以及状态：
  - `working`：“<名字> 处理中 · 用时”，头像加外圈；
  - `queued`：clay 标签“已唤醒 · 排队”；
  - 无负责人：“未分配”；其他情况显示最近更新时间；
  - `done` 列的卡片降低不透明度。
- 卡片详情包含以下内容：
  - 标题与描述：可以直接编辑。失焦或 Cmd+Enter 时保存。输入 `@` 时弹出 Agent 补全；
  - 所在列与负责人下拉框；
  - 一段接手规则说明：“<负责人> 正在处理或排队时别的 Agent 领不走；超过 20 分钟没有更新且 <负责人> 没有在运行，才允许别人接手”；
  - 当前状态提示；
  - 删除卡片；
  - “在房间中讨论”：打开与负责人的私聊，输入框预填“关于卡片「标题」（card-id）：”。

  改派负责人，或在描述里新增 `@<agent-id>`，会叫醒对应的 Agent。保存后，详情里提示“已通知 <名字>”。
- UI 提交语义位置：Column move 与 Card move 发送 `before_*_id` 或 append。UI 不自行保存 position，Server 返回重排后的结果。删除冲突、非空 Column/Board 等错误用统一的 `CommandError` 显示。前端不预判这些错误，不代替 Server 校验。

## 10. 运行记录

“运行记录”页在普通导航里，对照 Cumora `src/desktop/ObservabilityView.tsx` 的运行记录面板：

- 左侧是 Run 列表，可以按 Agent 与状态筛选。每行显示 Agent、trigger（消息、卡片、Agenda）、状态、开始时间与用时；
- 右侧是选中 Run 的事件时间线（`collab_run_events`）：triage 开始与结论、Engine 开始、完成、失败、限流等。每个事件带时间与附带数据；
- 自动刷新可以关闭；
- 它只用于观察，不提供重试、取消或编辑。

## 11. 视觉

- 沿用工作台的设计令牌（`app/theme/globals.css`）：`paper` / `paper-hover` / `surface` 三层面、`ink` 系文字、`line` 边框、`clay` 唯一强调色、`status-*` 状态色。标题用衬线字体。现有令牌同时驱动亮色与暗色。
- 新增一组 **Agent 识别色**令牌 `--agent-1` … `--agent-6`。它们只用于头像、名字、提及标签和工作中外圈。亮色与暗色各有 6 档低饱和色，从 clay 同一色系向外扩展。各档在亮度上也有差别：亮色的相对亮度为 0.07–0.14，暗色为 0.35–0.57。各档在 `paper` 与 `paper-hover` 上都不低于 4.5:1。Agent 按 ID 的 FNV-1a 哈希取色（`features/collab/components/agentIdentity.ts`），颜色不随列表顺序变化；`@all` 用 clay。
- clay 色的文字与图标用 `--clay-ink`（亮色 `#a24e36`，5.2:1）。`--clay` 本身在米色底上只有 2.8:1，所以只用于底色、描边与放置线。
- 小号说明文字在米色底上的对比度要达到 4.5:1。设计稿的次级文字比当前的 `--ink-faint`（`#8a8780`）更深。协作界面中 11–12px 的说明文字用 `ink-soft`，不用 `ink-faint`。
- 图标沿用 lucide。不使用 emoji。

## 12. 错误与恢复

| 情况 | Desktop 表现 |
|---|---|
| 初始 Runtime 启动失败 | setup 失败，应用不进入半可用的协作状态 |
| Runtime 正在成组替换 | command 返回 unavailable；store 保留旧 snapshot，并显示错误 |
| Server 业务拒绝 | 显示稳定的 error code/message |
| SSE 断线 | Tauri host 重连；页面 poll 继续 |
| Agent 出错 | Agent 卡片与房间成员列表显示错误信息，其他 Agent 不受影响 |
| Engine missing/error | Agent 页标题栏的 Engine 标签显示原因 |
| Desktop 正常退出 | 等待协作进程组停止，然后退出 |

## 13. 验收

1. setup 完成后，再读取 managed collaboration state；
2. 指定无效的 child ready metadata 时，启动失败，且 runtime 目录为空；
3. 忽略 SIGTERM 的 child 在 deadline 后被强制结束；
4. Desktop SSE 的有限响应流关闭后，能重新连接，并继续投影 invalidation；
5. 真实 supervisor 测试分别杀死 Server 和 Computer，两个 PID 与 RuntimeSession 都整体更换；
6. 每次替换后，fake OpenCode 仍能通过真实 shim 发布 durable reply；
7. 正常 shutdown 后，没有协作 child，当前 runtime 目录为空；
8. Desktop shutdown 后，PostgreSQL/Redis 仍可连接；
9. React bridge 参数与 Rust command DTO 一致，契约测试覆盖新增字段。运行记录页在普通导航中，列表可按 Agent 与状态筛选，选中 Run 后显示事件时间线；
10. 未读数与 `collab_room_viewed`：打开房间后未读归零；窗口不在前台时不上报，回到前台时补报；`user_viewed_seq` 不回退；
11. 说明行：路由、一轮上限、硬上限各按 §7.3 的条件出现，同一段对话中每类最多一次；
12. 引用回复：发送时带 `quotedMessageId`；消息与输入框正确显示引用；点击引用跳回原消息。`@` 补全候选为 `@all` 与房间内 Agent。卡片链接：代码中的 id 不渲染；已删除的卡片不可点击；消息下方有摘要卡；点击胶囊或摘要卡后，右侧栏显示该卡片预览；“打开看板”选中这张卡片。点击头像显示 Agent 资料。工作条出现与消失时，消息区不跳动；
13. Agent `activity` 的五种状态与卡片 `agentState` 的两种状态各有渲染测试；
14. Column 类型下拉与列头标记；在列底部创建卡片、编辑标题与描述、拖动换列与同列重排；改派与新增 `@` 叫醒对应 Agent；
15. 房间列表照 §7.1：平铺、置顶、四个筛选、群组头像拼图、“正在处理”行。Agent 之间的房间只出现在“Agent 私聊”页，且只读；
16. Room/Agent/Board store 的 loading/error 不承载业务真相；三种语言的文案键结构一致。
