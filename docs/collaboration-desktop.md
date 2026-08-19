# 协作模式桌面端

协作模式的前端：两个 Shell 由一个 `mode` 字段切换，各自拥有导航、布局与事件通道。

**范围划分**：工作台的前端见 [desktop.md](desktop.md)；协作模式的后端、协议与数据模型见 [collaboration.md](collaboration.md)，**那篇是协作语义的唯一权威**，本篇只回答"这些语义在界面上长什么样、状态归谁、怎么验收"。

## 1. 定位与边界

| 是什么 | 不是什么 |
|---|---|
| 与 `AppShell` 平级的第二个 Shell | 不是 `AppView` 枚举里多几个取值 |
| 一个按钮双向切换 | 不是第二个窗口，也不是路由 |
| 自己的图标 Rail 导航 | 不复用工作台的侧栏导航范式 |
| 共享 UI 原语、主题与 i18n 基建 | 不共享 `ChatInput` / `ToolActivity` / `ApprovalDialog` |
| 数据经 `src-tauri` 中转 | WebView **连不了 Unix socket**，前端永远不直连 daemon |

**核心不变量：**

1. **两个 Shell 互不知道对方存在。** `CollabShell` 不 import 工作台的任何 feature，`AppShell` 也不 import 协作的。唯一的连接点是 `App.tsx` 里的一个三元表达式和侧栏底部那个按钮。
2. **两根事件管子都挂在 `App.tsx`，与 `mode` 无关。** 切模式不拆监听。
3. **协作事件不与 `openwork://session-update` 共用通道。** 前端不需要分流。
4. **前端不做协作语义判定。** HELD、去重、认领的裁决全在 daemon；前端只显示结果。
5. **不做移动端。** 现有的 `shouldCollapseSidebar` / `shouldCollapseAgentRail` 只服务窄窗口，不引入移动布局。

## 2. 两个 Shell 与 mode

```tsx
// App.tsx
const mode = useModeStore((s) => s.mode)
useCoreEventBridge()      // 工作台事件，常驻
useCollabEventBridge()    // 协作事件，常驻
return mode === 'collab' ? <CollabShell /> : <AppShell />
```

### 2.1 mode 的位置与持久化

`app/modeStore.ts`，与 `app/themeStore.ts` 同层同形态：zustand + 直接读写 `localStorage`（键 `openwork-mode`），**不引入 zustand persist 中间件**——仓库现有的 `themeStore` / `projectStore` / `i18n` 都是这个写法。

**重启回到上次所在的模式。** 协作模式的前提是常驻同事在你不看的时候也在工作，每次启动强制回工作台等于否认这个前提。

### 2.2 切换按钮

工作台侧栏底部，与「设置」并列，同一视觉约定。协作模式侧栏区域对称放「返回工作台」。

**按钮上带未读提示**（圆点 / 数字）。这是 §4.2 让协作事件桥常驻的**直接目的**：不做未读提示，用户就没有任何途径知道该切过去看，常驻 Agent 的价值也就看不见了。

侧栏折叠到 0 时按钮随之消失——现有的「设置」按钮已经是这样，沿用既有约定，不为此新增顶栏入口。

### 2.3 切换时什么被保留

zustand store 是模块级单例，**切模式不丢任何状态**。会被卸载的只有组件树：滚动位置、输入框草稿、动画状态。

真正需要小心的是**事件监听**：`useCoreEventBridge()` 现在写在 `AppShell` 内部，`AppShell` 一卸载 effect cleanup 就会 `unlisten()`。所以它必须上提到 `App.tsx`（见 §4.2）。上提之后，切走期间工作台的 Turn 照常收更新，回来无需人工刷新。

## 3. CollabShell 的骨架

```text
┌──────┬──────────────┬──────────────────────┬─────────────┐
│      │ 房间列表      │ MainHeader           │ 同事名册     │
│ Rail │ （可拖宽）    ├──────────────────────┤ 与实时状态   │
│      │              │ 消息流                │             │
│      │              │                      │ （常驻，     │
│      │              ├──────────────────────┤  不抢槽）    │
│      │              │ @ 输入                │             │
└──────┴──────────────┴──────────────────────┴─────────────┘
```

### 3.1 Rail

四个目的地：房间 / Agent / 看板 / 日志。底部放「返回工作台」。

这与工作台的侧栏导航**范式不同**——工作台用可折叠的 240px 侧栏，协作用固定窄 Rail。这是刻意的差异：切过去要能立刻感到"换了地方"。代价是新增入口时两边各有一套加法，需要靠纪律维持。

### 3.2 macOS 红绿灯与拖拽区

一条从窗口顶通到底的 Rail 会**长在红绿灯底下**：mac 上前几个图标点不到，而且窗口失去可拖拽区域。

工作台不存在这个问题，因为它的最左是 `Sidebar`，其头部标了 `data-tauri-drag-region="deep"`；侧栏收起时由 `headerInsetClass(sidebarExpanded)` 给顶栏补 `pl-20`。那个函数的注释要求"每一种占据顶栏的视图都要用它，不要各写各的"——但 Rail 占据的是**左栏**不是顶栏，它管不到。

**做法**：Rail 顶部**仅 macOS** 预留约 28px 并标 `data-tauri-drag-region`，那块放模式标识 / 品牌图标，图标从它下面开始。

**不加 TitleBar 行。** cumora 能让 Rail 贴左边，是因为它上面压了一整行 44px 的 `TitleBar`（`DesktopApp.tsx` 的 `grid-rows-[44px_1fr]`）。OpenWork 加这一行会让两个模式的窗口结构不同，切换时内容整体跳一次。

预留是平台分支，Windows / Linux 不留——**要确保不留时图标不错位、留时不出现空洞**。

### 3.3 房间视图三栏

| 栏 | 内容 | 与工作台的对应 |
|---|---|---|
| 左 | 房间列表，可拖宽 | 项目 / 会话树 |
| 中 | 消息流 + `@` 输入 | 对话 |
| 右 | **同事名册与实时状态，常驻** | 子智能体运行状态（`AgentRail`） |

**右栏不抢槽。** cumora 的右栏是 thread / agent info / doc / board / calendar 五种 peek 竞争同一位置，打开一个隐式关掉另一个——它必须这样，因为看板和文档在它那里没有顶层入口。**我们有 Rail，看板和日志各有自己的 view**，右栏因此可以专职。

"谁在线、谁在忙、谁正在跑 `$ cargo test`"是常驻同事这个产品最核心的感知，它不能被卡片预览顶掉。

## 4. 数据通道

### 4.1 src-tauri 的 daemon 客户端

WebView 连不了 Unix socket，所以 `src-tauri` 新增一层 daemon 客户端：

- 依赖 `openwork-collab` 取 IPC 线协议类型（见 [architecture.md §1](architecture.md)）；**不得绕过 daemon 直连 `collab_*` 表**；
- 负责 daemon 的发现与拉起（`collaboration.md §12`：启动时发现 socket，未运行则拉起，Desktop 关闭时**不停止** daemon）；
- 读写暴露为短生命周期 Tauri Command，与 `commands/` 现有形态一致；
- 把 daemon 推来的事件 emit 成 `openwork://collab-event`。

### 4.2 两根管子都常驻 App.tsx

```text
openwork://session-update       + batch   工作台   ← useCoreEventBridge
openwork://collab-event         + batch   协作     ← useCollabEventBridge
```

**两根完全独立**，前端不需要按类型分流；`compat.ts` 那套版本协商与 `sequence` 补洞逻辑也各自一份，不互相污染。

两个 hook 都在 `App.tsx` 调用，与 `mode` 无关。这一条同时解决两件事：切走时工作台不失联，以及工作台模式下能收到协作未读。

### 4.3 批处理

现有 `LIVE_UPDATE_BATCH_MS = 100`，只对 `text_delta` / `reasoning_delta` / `tool_call_progress` 三类做窗口合并，其余立即 flush 以保序。协作侧沿用同一形状，但**阈值必须实测重定**：现有值是为单会话 delta 调的，协作是 N 个 Agent 并发推 `commandExecution`，是另一个量级。

## 5. 状态与 store

### 5.1 划分

| store | 拥有 |
|---|---|
| `app/modeStore` | `mode` |
| `features/collab/collabNavigationStore` | Rail 选中项、当前房间、右栏折叠 |
| `features/collab/rooms/roomStore` | 房间列表、成员、未读 |
| `features/collab/rooms/messageStore` | 按房间归一化的消息窗口与游标 |
| `features/collab/agents/agentStore` | Agent 定义、在线与实时状态 |
| `features/collab/boards/boardStore` | 看板、列、卡片 |

### 5.2 消息历史：游标分页与双向加载

工作台的会话是任务级的，`get_session_snapshot` 一次全量拿回来即可。**协作房间是长期的**：几个常驻同事互相说话加上主动性唤醒，跑几个月就是几万条。

- 按 `sequence` 做游标，双向加载；
- 默认从 `collab_room_members.last_read_seq` **附近**打开，而不是从最新一条；
- 内存里只保留一个窗口，向两端按需扩展；
- 窗口拼接、跳转定位、滚动锚定是真实工作量，不要低估。

### 5.3 未读

未读来自 `last_read_seq` 与房间最高 `sequence` 的差。房间列表逐行显示，Rail 的房间图标显示总数，工作台侧栏按钮显示总数——**同一个数字三处呈现，只有一个来源**。

被静音的房间不计入总数（逐行仍显示），否则静音就没有意义。

## 6. 复用边界

| 共享 | 各写各的 |
|---|---|
| `components/ui/*`（button / select / dropdown-menu / CopyButton） | 房间消息气泡 |
| `components/markdown/MarkdownRenderer` | `@` 提及输入框 |
| 主题 token（`app/theme/globals.css`） | 同事卡片与名册 |
| i18n 基建（`i18n/`） | 看板与卡片 |
| `ErrorBoundary` | 日志与事件流抽屉 |
| `lib/`（dateTime、错误转换） | |

**`ChatInput` 不复用。** 那 564 行绑死了工作台概念：Skill 提及绑定、`RuntimePermissionMode`、上下文用量指示器、模型选择器。协作的输入框要的是「文本 + `@`同事 + 发送」。

**`@` 提及另写一套与 `skillMentions` 同构但独立的绑定机制**，不改造后者——两者的可见 token、候选来源、绑定对象完全不同，合并只会让两边都长出条件分支。

`components/ui` 目前只有 4 个原语，协作要用的输入框、气泡、头像、看板卡一个都没有，**新增的通用原语放 `components/ui`，业务组件留在 `features/collab`**。

## 7. 实时状态与待审批

### 7.1 实时状态

由 OpenCode 的事件流驱动，不轮询。daemon 订阅 `GET /event`（**instance 范围，按 `properties.sessionID` 过滤**），把与协作 Agent 相关的事件转成 `openwork://collab-event`：

| 事件类别 | 名册上的表现 |
|---|---|
| 工具/命令开始执行 | 正在执行 `…`（原文截断） |
| 助手消息产出 | 正在回复 |
| 上下文压缩 | 正在整理上下文 |
| 会话状态转 idle / busy | 空闲 / 在忙 |
| 超时无事件 | 无响应 |

具体事件名以实测为准（P0 尖刺产出），**前端不硬编码事件名**——daemon 负责归一化成上面五种表现，前端只消费归一化后的状态。这样 OpenCode 换事件名时改动只落在 daemon。

### 7.2 待审批角标

协作模式的审批**永久挂起、不超时**（`collaboration.md §6`）。一个凌晨触发的 `ask` 会把那个 Agent 占到用户回来为止，期间它对房间里的一切都不响应。因此"有人在等你"必须是**永远可见**的，而不是一条会滚走的消息：

- Rail 与工作台侧栏按钮上都显示**待审批数**，由 **daemon 汇总后给出**——`GET /permission` 是 instance 范围的，跨 Agent 的汇总怎么做见 `collaboration.md §6`，**前端不该知道 instance 这个概念**；
- 未读与待审批**分开计数**：未读可以慢慢看，待审批是有 Agent 卡住了；
- 点开是审批卡片：谁、要做什么、涉及哪些路径，可回 `once` / `always` / `reject`（`reject` 可填理由，会回给模型）；
- 卡片上有 `abort` 逃生口，直接掐掉那一轮。

名册上被卡住的 Agent 显示为**等待审批**，与"在忙"区分——它不是在干活，是在等你。

**不做第二套 Trace UI。** [trace.md](trace.md) 的 Span 树与完整度派生建立在"OpenWork 组装了这次请求"之上；协作的请求由 OpenCode 自己组装，口径对不上。日志抽屉是 `collab_runs` / `collab_triages` / `collab_events` 三张平表的时间序视图。

## 8. i18n、主题与时间

- **三语全跟**（zh-CN / en-US / zh-TW）。现有三份合计 1047 行，协作预计再加 150~250 键 × 3。漏键会回退成 key 名显示，比多写两遍更糟。
- **主题完全共享**，不引入第二套配色。`--clay` 仍是唯一 accent。"换了模式"的感知由 Rail 与布局承担，不由配色承担。
- 时间显示一律走 `lib/dateTime.ts`，**禁止对时间字符串做切片、拼接或正则**（见 [.claude/rules/database.md](../.claude/rules/database.md)）。

## 9. 目录

```text
desktop/src/
├── app/
│   ├── App.tsx                  按 mode 分发；两个事件桥都在这里
│   ├── AppShell.tsx             工作台（现有）
│   ├── CollabShell.tsx          协作模式
│   ├── modeStore.ts             mode + localStorage
│   └── useCollabEventBridge.ts
├── bridge/
│   └── collab.ts                协作的 Command 与 Event 封装
└── features/collab/
    ├── collabNavigationStore.ts
    ├── components/              Rail、三栏骨架
    ├── rooms/
    ├── agents/
    ├── boards/
    └── logs/
```

Shell 归 `app/` 是既有约定（`AppShell` 就在那里），不因为多了一个而改。

## 10. 明确不做

| 不做 | 理由 |
|---|---|
| 移动端布局 | 明确排除 |
| react-router | Tauri 应用没有地址栏，为一个开关引入路由层是投机抽象 |
| 第二个 Tauri 窗口 | 诉求是"一个按钮切换"，不是"多开一个窗" |
| 两个 Shell 同时挂载（CSS 隐藏） | 隐藏那棵树的定时器与动画照跑，且不变量 #1 在 DOM 层不再成立 |
| mock 数据层 | 前端跟随后端竖切；视觉迭代用现有的组件 fixture |
| 空状态与前置引导 | daemon 未起、`opencode` 未装 / 版本不符 / 未登录、无 Agent 五种情况不做分级引导。**见 §11 R-F2** |
| 第二套主题 / 视觉语言 | 见 §8 |
| 第二套 Trace UI | 见 §7 |
| 复用 `ChatInput` / `ToolActivity` / `ApprovalDialog` | 见 §6 |

## 11. 已知风险

| | 项 | 说明 |
|---|---|---|
| **R-F1** | 两套导航范式并存 | 侧栏 vs Rail。新增入口时两边各要加一次，靠纪律维持 |
| **R-F2** | 不做空状态 | P0/P1 期最常见的状态恰恰是"连不上"或"版本不符"，届时只能看终端日志。补救是一条"daemon + opencode 状态 + 错误文本"横条，几十行 |
| **R-F3** | Rail 顶部预留是平台分支 | 非 macOS 不留，要确保不出现空洞或图标错位 |
| **R-F4** | 协作事件密度未知 | 100ms 批窗口是为单会话 delta 调的；N 个 Agent 并发是另一个量级，需实测重定 |
| **R-F5** | "换了模式"的感知只靠 Rail 与布局 | 主题与原语全共享，差异可能不够明显 |
| **R-F6** | 审批永久挂起 | 用户没看见角标就等于一个 Agent 无限期停摆。角标与"等待审批"状态是**必做项**，不是装饰（§7.2） |
| **R-F7** | **原生窗口拖拽无法自动验收** | macOS 对自动化进程报 `sharingState=0` 与 `postEventAccess=false`，既截不到 WebView 也合成不了拖拽。渲染测试只能守住「28px 区域存在、带 `data-tauri-drag-region`、非 macOS 为 `hidden`」；**「这块区域真的能拖动窗口」只能人工拖一次**。改动 Rail 顶部时不要再花时间尝试自动化 |

## 12. 分期与验收

前端不独立排期，嵌在 [collaboration.md §16](collaboration.md) 的同一条竖切里，**期号与后端一致**（后端 P0 是纯尖刺，前端从 P2 开始）。

### P2 — 切得过去，说得上话

`modeStore` + 切换按钮 + `CollabShell` + Rail + 房间三栏 + 两个事件桥常驻 + 消息游标分页 + Agent 管理 + 未读 + 待审批角标。

1. 点侧栏底部按钮进入协作模式，再点一次回到工作台；重启应用回到**上次所在**的模式；
2. `useCoreEventBridge` 已上提到 `App.tsx`：工作台跑着 Turn 时切到协作模式再切回来，**无需人工刷新**，transcript 与运行状态连续；
3. 协作房间来新消息时，即使当前在工作台模式，侧栏按钮出现未读提示；
4. macOS 上 Rail 首个图标不被红绿灯遮挡，且 Rail 顶部区域可拖动窗口；Windows / Linux 上无空洞；
5. 在房间里发言，被 `@` 的 Agent 回话后出现在同一流里；
6. 在 Desktop 创建、编辑、停用 Agent；停用后不再出现在名册与唤醒候选中，房间历史里它的消息与身份完好；
7. Agent 触发 `ask` 时出现待审批角标与卡片；回 `once` 后它继续，回 `reject` 并填理由后它换路；
8. 待审批数与未读数**分开计数、互不吞并**；被卡住的 Agent 在名册上显示"等待审批"而非"在忙"；
9. 卡片上的 `abort` 能掐掉那一轮，Agent 回到空闲；
10. `CollabShell` 的 import 图中不出现 `features/{chat,sessions,traces,models}` 的任何模块。

### P3 — 看得见在干什么

OpenCode 事件驱动的名册 + HELD 反馈 + 分页的长房间表现。

11. Agent 执行工具时，右栏名册显示"正在执行 `…`"，状态来自事件而非轮询；
12. **前端不硬编码 OpenCode 的事件名**，也不感知 instance / directory——只消费 daemon 归一化后的五种状态（§7.1）；
13. 一个 Agent 被 HELD 后重发，界面上不出现重复内容，且能看出发生过一次让位；
14. 一轮在进行中被新消息打断、其结论未产出时，界面**不显示为已完成**——未读仍在原地（见 `collaboration.md §8.1`）；
15. 构造一个上万条消息的房间：打开耗时与最新几条的房间无明显差异；
16. 离开三天后打开房间，**从未读处开始**而不是从最新一条，向上向下都能继续加载；
17. 静音房间不计入 Rail 与侧栏按钮的未读总数，但房间列表里仍显示其行内数字。

### P4 — 看板

18. 用户建卡并指派给某个 Agent，卡片出现在看板且房间收到系统消息；
19. Agent 认领卡片后，看板上该卡显示认领人，且用户端无需刷新。

### P5 — 主动性可见

20. 由 agenda 唤醒的一轮，在房间或日志里能看出"这是主动发起的，因为什么"；
21. 停滞推动产生的发言有可辨识的标识，不与普通回复混淆。

### P6 — 补齐

日志与事件流抽屉 + i18n 补齐。

22. 日志抽屉能还原一次唤醒的完整经过：triage 裁决 → prompt 起止 → 工具与命令执行 → 发言 → usage；
23. 三份 locale 无缺键（无任何界面文案回退成 key 名）。

### 贯穿全程

24. 前端不出现任何协作语义判定（HELD、去重、认领的裁决）——界面只显示 daemon 给的结果；
25. 新增的纯函数、reducer 与 store 有与现有同强度的 vitest 覆盖；
26. 未读总数在房间列表、Rail、侧栏按钮三处一致，且只有一个来源；待审批数同理。
