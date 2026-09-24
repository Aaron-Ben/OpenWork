# macOS 协作桌面端

协作模式是 OpenWork Desktop 中与工作台平级的第二个 Shell。React 负责投影 Server canonical state；Tauri host 负责监督本机 Collaboration Runtime 并把 typed Desktop command 转发给 Server。

业务语义与存储约束见 [collaboration.md](collaboration.md)。界面设计稿：https://claude.ai/artifact/MDGsQTdy7KuRLYPuvFGHeR（房间亮/暗、房间里点开卡片、Agent、看板）。

## 1. 产品边界

| Desktop 负责 | Desktop 不负责 |
|---|---|
| 启动、监督和停止 Server/Computer 子进程 | 不执行 Agent loop |
| 管理 Agent profile/runtime config | 不选择 triage 结果或 Agenda 候选 |
| 创建 Room、管理 Group audience、发送与引用消息 | 不直接写 PostgreSQL/Redis |
| 管理 Board/Column 结构、Column 类型和删除 | 不向 Agent 暴露结构删除命令 |
| 上报用户看到了哪些消息（`user_viewed_seq`） | 不持有 Agent JWT 或 Engine session |
| 展示 Runtime、Engine、Agent 当前状态、Message、Card 状态 | 不提供运行记录，不展示 Run、triage 或 Engine 的历史 |
| 把 Desktop SSE invalidation 转成 Tauri event | 不把 SSE 当成业务事实 |

**协作模式没有运行记录。** 界面只回答“现在怎样”：谁在工作、谁暂停了以及原因、哪张卡片在等谁；以及房间里“为什么有人没回复”的说明行（§7.3）。它不展示某次运行做了什么、花了多少 token、调用过哪些命令。

只支持当前 Mac。界面没有远程机器、Computer 选择器或后台 Runtime 开关；Desktop 正常退出即停止 Collaboration Runtime。

## 2. Shell 与导航

`desktop/src/App.tsx` 根据 `modeStore` 选择工作台或 `CollabShell`：

```text
App
├── workbench → AppShell
└── collab    → CollabShell
```

mode 写入 `localStorage` 的 `openwork-mode`。切换 mode 只替换 React 组件树，不重启 Tauri host 或 Collaboration Runtime。

`CollabRail` 宽 64px，有三个顶层目的地，底部是返回工作台：

1. 房间；
2. Agent；
3. 看板。

macOS Rail 顶部保留 36px 窗口拖拽区，避免内容压在窗口控制按钮下。协作 feature 不 import 工作台 chat feature；两者只共享 UI primitive、主题、i18n 和通用错误处理。

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

`CollabDaemonClient` 的 interface 只有三类能力：`call(DesktopCommand)`、`subscribe_invalidations()`、`shutdown()`。它内部持有 `runtime.lock` 文件句柄、当前 HTTP connection 和 Desktop secret、supervisor command channel、Server/Computer child handles、Desktop SSE task 与当前 runtime 目录。

首次启动失败会让 Tauri setup 失败，不注册半可用 managed state。运行期任一子进程退出时，connection 暂时变为 unavailable；成组重启成功后，新 command 自动使用新 RuntimeSession connection。

应用事件循环返回后，`lib.rs` 先取出 setup 已注册的 `CollabDaemonClient`，再等待 `shutdown()` 完成，最后以原 exit code 结束 Desktop。shutdown 总等待上限为 30 秒；Computer 有 20 秒外层窗口，Server 有 5 秒窗口。

## 4. Tauri command seam

React bridge 位于 `desktop/src/bridge/collab.ts`；Rust adapter 位于 `desktop/src-tauri/src/commands/collab.rs`。两边只传 `openwork-collab::protocol::desktop` 定义的 typed command/result；前端类型只在 bridge 定义（[.claude/rules/contracts.md](../.claude/rules/contracts.md)）。

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

`AgentView` 在 profile/config 之外带当前状态 `activity`：

| `activity.kind` | 附带 | 来源 |
|---|---|---|
| `working` | 正在处理的房间或卡片标题、开始时间 | 该 Agent 的 running Run |
| `queued` | 待处理卡片数与第一张的标题 | 未结算的 `collab_card_wakes` |
| `paused` | 原因、恢复时间 | Runner heartbeat 的 `paused`（collaboration.md §5） |
| `error` | 最后一次错误 | Runner heartbeat 的 `error` 或 Engine inventory |
| `idle` | 最近一次发言的房间与时间 | 该 Agent 最近一条消息 |
| `archived` | — | profile |

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

- `RoomView` 增加 `unreadCount`（sequence 大于 `user_viewed_seq`、作者不是 `local-user` 的 normal 消息数）、`lastMessage`（作者显示名与正文前 80 字）、`workingAgentIds`（在该房间有 running Run 的 Agent）、`userIsMember`。
- `collab_room_open` 返回一个房间快照：消息、成员及其 `activity`、说明行（§7.3）。`MessageView` 增加 `authorName`、`authorKind`、`authorRole` 与可空的 `quoted { id, authorName, body }`（原文前 180 字）。
- `collab_message_send` 增加可选的 `quotedMessageId`。
- `collab_room_viewed { roomId, upToSeq }`：用户看到了这个房间到 `upToSeq` 为止的消息。Server 只增不减地写入 `collab_rooms.user_viewed_seq`（collaboration.md §13.3.4）。
- Desktop 用户可以创建 Group 并改变 Group audience。Direct Room 创建是“创建或返回已有 Room”。成员操作只接受 Agent ID；固定用户始终由 Server 管理。

当前 `collab_room_open` 没有分页参数；如果长期 Room 的数据量要求分页，先扩展 Server 的 typed read command，不在前端截断 canonical list。

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
collab_card_assign
collab_card_delete
```

- `BoardColumnView` 用 `kind`（`todo` / `doing` / `done` / `null`）替换 `isTerminal`。
- `CardView` 增加当前状态 `agentState`：`working`（负责人正在处理这张卡片）、`queued`（有未结算的卡片唤醒，负责人在忙）、`notified`（有未结算的卡片唤醒，负责人暂停中）或 `null`。

Agent 创建、领取、更新、移动 Card 走 Agent command，不经过这些 Desktop command。

### 4.4 删除的 command

`collab_run_list` 与 `collab_run_trace` 及其 protocol 类型、Server 的 `observability` 模块、`collab_run_events` 表和 Runner 的事件上报一起删除。房间里原来依据 Run 列表推断的“思考中/重试/限流/失败”改由成员的 `activity` 给出。

## 5. SSE 到 WebView

Tauri host 独占 `/desktop/events`。收到 `InvalidationEvent` 后，通过固定事件名发给 WebView：

```text
openwork://collaboration-invalidation
```

payload：

```ts
interface CollabInvalidation {
  id: string
  kind: 'runtime_ready' | 'agent_config' | 'message' |
        'engine_inventory' | 'runner_status'
  subjectId: string | null
  revision: number | null
  publishedAt: number
}
```

event 只表示“某类 canonical view 可能变化”。前端不能把它直接拼进 store；收到后重新调用 Tauri command 获取完整 view：

- Runtime/Engine/Runner invalidation → 重新取 `status` 与 Agent 列表；
- runtime-ready / agent-config → 同时重新取 Agent 列表；
- message invalidation → 重新取房间列表；当前打开的房间由它自己的 poll 刷新。

打开的房间每 2 秒调用一次 `collab_room_open`，看板页面每 5 秒读取一次 Board。这两个 poll 是 durable fallback，也避免 WebView 必须理解 Agent SSE 或 Redis wake。Desktop SSE 断线后由 Tauri host 独立指数退避重连；WebView 不参与 credential 或 connection 管理。

## 6. Store 所有权

| store | 拥有 |
|---|---|
| `modeStore` | workbench/collab mode |
| `collabNavigationStore` | Rail view、当前房间、房间右侧栏显示房间信息还是卡片预览（及预览的卡片 id）、当前看板与选中的卡片 |
| `roomStore` | 房间列表与创建流程 |
| `messageStore` | 当前房间快照、草稿与引用目标、已上报的 `user_viewed_seq` |
| `agentStore` | Agent 列表和 profile/config 操作 |
| `boardStore` | Board tree 与结构操作 |
| `runtimeStore` | RuntimeStatus snapshot |

Store 只保存 UI snapshot 和 request 状态。权限、幂等、顺序、领取、HELD、triage、路由与 Agenda 决策都由 Server 裁决。状态转换写成纯函数 reducer（[frontend.md](../.claude/rules/frontend.md)），说明行、未读、`@` 补全候选等派生数据放在同目录的视图模型模块里单独测试。

## 7. 房间页面

```text
┌──────┬──────────────┬──────────────────────────────────┬──────────────┐
│ Rail │ 房间列表      │ 消息区                            │ 房间信息      │
│ 64   │ 272          │ 标题栏 / 消息流 / 工作条 / 输入框   │ 300，可收起   │
└──────┴──────────────┴──────────────────────────────────┴──────────────┘
```

### 7.1 房间列表

- 顶部：标题、新建房间按钮、搜索框（按房间名和成员名过滤）。
- 分三组：**群组**、**私聊**（用户与某个 Agent）、**Agent 之间**（用户不是成员的 Direct Room，组标题注明“只读”）。
- 每行：房间头像（群组取标题首字，私聊用对方的 Agent 头像，Agent 之间用两个重叠头像）、标题、最近一条消息的作者与摘要、时间或未读数。房间里有 Agent 在工作时，摘要换成“<名字> 正在处理…”，并在前面显示该 Agent 识别色的小圆点。
- 当前房间用 `paper` 底色加轻阴影标出。

### 7.2 消息流

- 所有消息左对齐。每条显示头像、显示名、Agent 的 role、时间。用户显示为“你”，头像用墨色；Agent 的头像和名字用各自的识别色（§10）。
- 引用：正文上方显示被引用消息的作者与原文摘要（单行截断）。
- 正文中的 `@<id>` 渲染成该成员识别色的提及标签；Markdown 与行内代码沿用 `MarkdownRenderer`。
- **卡片链接**：正文中（代码块与行内代码之外）匹配 `card-[0-9a-f]{32}` 的 id 渲染成胶囊，显示看板图标与卡片标题；标题从已加载的 Board 快照中查找，没加载时先读取一次 Board 列表，卡片已删除时只显示 id 且不可点击。点击后房间右侧栏切换为这张卡片的预览（§7.5），胶囊描边变为 clay。对照 Cumora `src/components/CardLink.tsx` 与 `src/desktop/BoardPeekPane.tsx`。
- 悬停消息时显示浮动工具条：**引用回复**、复制。
- 打开房间、以及房间可见时有新消息到达并滚动到底部，都上报 `collab_room_viewed`，`upToSeq` 为视口中最新的 sequence。窗口不在前台时不上报。
- Agent 之间的房间只读：没有输入框，底部显示“这是 Agent 之间的私聊，你只能查看”。

### 7.3 说明行

说明行不是消息，由 `collab_room_open` 根据本房间的 `collab_triages` 生成，插在触发它的消息之后。它们解释“为什么有人没回复”，不展示任何运行细节：

| 说明行 | 条件 | 文案 |
|---|---|---|
| 路由 | 同一条人类消息有 Agent 的 triage 为 `source = routing`、`response_mode = me` | “Ada、Cy、Dee 判断这条是给 **Bo** 的，没有参与” |
| 一轮上限 | 自最近一次人类消息后，本房间第一次出现 `source = lap_floor` 的 triage | “讨论已满一轮，**Cy** 开始第二次发言，其余 Agent 不再被叫醒。你发言后会继续。” 附“为什么？”按钮，点开是一句话解释 lap floor |
| 硬上限 | 同上，`source = loop_cap` | “Agent 之间已连续 20 条消息，暂停到你下次发言。” |

每类说明行在同一段对话（两条人类消息之间）最多出现一次。

### 7.4 工作条与输入框

- 输入框上方的工作条：本房间有 Agent 在工作时，显示头像、“<名字> 正在处理<房间或卡片> · 已用时间”。没有人工作时不占位。
- 输入框由三部分组成：
  - 引用条：正在引用时显示“回复 <名字>：<原文>”与取消按钮；
  - 文本框：Enter 发送，Shift+Enter 换行；输入 `@` 弹出补全，候选依次为 `@all`（注明“全员，不收窄”）和房间内的 Agent（显示名 · role），上下键选择、Enter 确认；
  - 工具行：`@` 按钮、提示“只 @ 某人时，其他 Agent 会先判断是否与自己有关”、发送按钮。

### 7.5 房间右侧栏

右侧栏有两种内容，默认显示房间信息，点击消息里的卡片链接后显示卡片预览；关闭预览回到房间信息。

**房间信息**：

- 成员列表：头像（工作中的 Agent 加识别色外圈）、显示名与 `@id`、一句当前状态、状态标签。状态标签与 `activity.kind` 对应：工作中（success）、已唤醒（clay）、空闲（中性）、暂停（warning，附原因与恢复时间）、出错（danger）。
- 群组显示“管理”入口：添加、移除成员。
- 底部一句提示：“你在看这个房间时，Agent 之间的讨论不会一轮就停；离开后，它们说完一轮就会暂停。”

**卡片预览**：顶部“卡片 · <看板名>”与关闭按钮；卡片标题；所在列与类型标记；负责人；当前状态（与看板页卡片的 `agentState` 一致）；描述；底部“打开看板”，跳到看板页并选中这张卡片。预览只读，改负责人或列要去看板页。

## 8. Agent 页面

- 标题栏：标题、“N 位活跃 · N 位工作中 · N 位暂停”、Engine 状态标签（例如“OpenCode 1.18.18 · 沙箱已启用”，异常时为 danger 并显示原因）、新建 Agent。
- 标签页：活跃 / 已归档。
- 两列卡片，每张：
  - 头部：识别色头像（工作中加外圈）、显示名与 `@id`、role、状态标签；
  - 当前状态行：工作中“处理卡片「…」· 用时”，排队中“待处理：你指派的「…」”，空闲“上次在「房间」回复 · 时间”；
  - 暂停或出错时，状态行换成 warning/danger 提示框，写明原因、会不会自动恢复、何时恢复、用户要做什么（例如“在终端运行 `opencode auth login`”）；
  - persona 摘要（最多 3 行）；
  - 主模型与判断模型；
  - 底部：“主动巡检（Agenda）”复选框、私聊、编辑。
- 编辑对话框可修改显示名、role、persona、Engine、主模型和 triage 模型。Engine 下拉目前只有 OpenCode。Persona 编辑器只编辑用户人格部分；Computer 写入 `AGENTS.md` 时追加代码拥有的协作契约，persona 不能移除它。
- create 使用 Server 返回的 slug ID；archive 立即停止 Runner 但保留记录；restore 触发 config revision 变化和 reconcile。

## 9. 看板页面

```text
┌──────┬────────────┬──────────────────────────────────────┬──────────────┐
│ Rail │ 看板列表    │ 列                                    │ 卡片详情      │
│ 64   │ 200        │ 每列一个圆角容器，横向排列              │ 340，选中时    │
└──────┴────────────┴──────────────────────────────────────┴──────────────┘
```

- 看板列表：看板名与卡片数，新建看板。
- 标题栏：看板名、“N 张卡片 · N 位 Agent 在做”、编辑列。
- 列头：列名、卡片数、类型标记：`TODO`（描边）、`DOING`（success 底）、`DONE`（墨色底）、`未分类`（虚线描边）。编辑列对话框里类型是下拉框。
- 卡片：标题、描述摘要（`@id` 用识别色）、负责人头像，以及状态：
  - `working`：“<名字> 处理中 · 用时”，头像加外圈；
  - `queued`：clay 标签“已唤醒 · 排队”；
  - `notified`：clay 标签“已通知 <名字>”，旁注暂停原因（例如“等限流结束”）；
  - 无负责人：“未分配”；其他情况显示最近更新时间；
  - `done` 列的卡片降低不透明度。
- 卡片详情：标题、描述、所在列与负责人下拉框、一段接手规则说明（“<负责人> 正在处理或排队时别的 Agent 领不走；超过 20 分钟没有更新且 <负责人> 没有在运行，才允许别人接手”）、当前状态提示、删除卡片、“在房间中讨论”（打开与负责人的私聊，输入框预填“关于卡片「标题」（card-id）：”）。
- UI 提交语义位置：Column move 发送 `before_column_id` 或 append；UI 不自行保存 position，Server 返回重排后的完整 Board。删除冲突、非空 Column/Board 等错误通过统一 `CommandError` 显示，不由前端预判代替 Server 校验。

## 10. 视觉

- 沿用工作台的设计令牌（`app/theme/globals.css`）：`paper` / `paper-hover` / `surface` 三层面、`ink` 系文字、`line` 边框、`clay` 唯一强调色、`status-*` 状态色；标题用衬线字体。亮色与暗色都由现有令牌驱动。
- 新增一组 **Agent 识别色**令牌，只用于头像、名字、提及标签和工作中外圈：亮暗各 6 档低饱和色，从 clay 同一色系向外扩展，彼此在亮度上也有差别。Agent 按 ID 的稳定哈希取色，不随列表顺序变化。
- 小号说明文字在米色底上需要满足 4.5:1 对比度：设计稿用的次级文字比当前 `--ink-faint`（`#8a8780`）更深。协作界面的 11–12px 说明文字使用 `ink-soft`，不使用 `ink-faint`。
- 图标沿用 lucide；不使用 emoji。

## 11. 错误与恢复

| 情况 | Desktop 表现 |
|---|---|
| 初始 Runtime 启动失败 | setup 失败，应用不进入半可用协作状态 |
| Runtime 正在成组替换 | command 返回 unavailable；store 保留旧 snapshot 并显示错误 |
| Server 业务拒绝 | 显示稳定 error code/message |
| SSE 断线 | Tauri host 重连；页面 poll 继续 |
| Agent 暂停或出错 | Agent 卡片与房间成员列表显示原因，其他 Agent 不受影响 |
| Engine missing/error | Agent 页标题栏的 Engine 标签显示原因 |
| Desktop 正常退出 | 等待协作进程组停止后退出 |

## 12. 验收

1. setup 完成后再读取 managed collaboration state；
2. 指定无效 child ready metadata 时启动失败且 runtime 目录为空；
3. 忽略 SIGTERM 的 child 会在 deadline 后被强制结束；
4. Desktop SSE 有限响应流关闭后能重新连接并继续投影 invalidation；
5. 真实 supervisor 测试分别杀死 Server 和 Computer，两个 PID 与 RuntimeSession 都整体更换；
6. 每次替换后 fake OpenCode 仍能通过真实 shim 发布 durable reply；
7. 正常 shutdown 后没有协作 child，当前 runtime 目录为空；
8. PostgreSQL/Redis 在 Desktop shutdown 后仍可连接；
9. React bridge 参数与 Rust command DTO 一致，契约测试覆盖新增字段；`collab_run_list`、`collab_run_trace` 与运行记录页不存在；
10. 未读数与 `collab_room_viewed`：打开房间后未读归零，窗口不在前台时不上报，`user_viewed_seq` 不回退；
11. 说明行：路由、一轮上限、硬上限各按 §7.3 的条件出现，同一段对话每类最多一次；
12. 引用回复：发送带 `quotedMessageId`，消息与输入框正确显示引用；`@` 补全候选为 `@all` 与房间内 Agent；卡片链接：代码中的 id 不渲染，已删除的卡片不可点击，点击后右侧栏显示该卡片预览，“打开看板”选中这张卡片；
13. Agent `activity` 的六种状态与卡片 `agentState` 的三种状态各有渲染测试；
14. Column 类型下拉与列头标记；
15. Agent 之间的房间只读；
16. Room/Agent/Board store 的 loading/error 不承载业务真相；三种语言文案键结构一致。
