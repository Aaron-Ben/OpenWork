# 桌面端

Tauri 2 + React + TypeScript。React 只通过 Tauri Command/Event 与 Core 通信，**不访问 SQL、Provider Adapter 或 Tool Executor**。

本文描述当前已经落地的 Desktop 实现。具体协议仍以 [permissions.md](permissions.md)、[multi-agent.md](multi-agent.md)、[skills.md](skills.md) 与 [trace.md](trace.md) 为准。

## 1. 目录与职责

```text
desktop/
├── src/
│   ├── app/          AppShell、导航、主题、唯一的 Event Bridge
│   ├── bridge/       commands.ts / events.ts / compat.ts（Host DTO）
│   ├── features/
│   │   ├── sessions/ canonical Session、Message、Plan 与左栏项目树
│   │   ├── chat/     per-session runtime view、对话投影、工具卡与多智能体视图
│   │   ├── models/   Provider / Model 设置
│   │   ├── settings/ 通用设置与 Skill 设置
│   │   ├── traces/   运行记录列表、时间线与正文查看
│   │   └── projects/ 工作目录
│   ├── components/   共享 UI
│   ├── lib/          dateTime、错误转换等通用工具
│   └── i18n/         zh-CN / zh-TW / en-US
└── src-tauri/src/
    ├── lib.rs        进程入口与 Command 注册
    ├── commands/     短生命周期 Command 适配
    └── error.rs      Core 错误 → CommandError
```

只有拥有 API、状态或独立业务语言的 Feature 才建目录，不为每个小组件建 Feature。

## 2. 应用外壳与导航

### 2.1 三种聊天形态

```text
形态 A｜两栏（没有子智能体，或窗口不足以容纳右栏）
┌──────────┬────────────────────────────────┐
│ Sidebar  │ MainHeader                     │
│ 240px    ├────────────────────────────────┤
│          │ 对话流                          │
│          ├────────────────────────────────┤
│          │ ChatInput                       │
└──────────┴────────────────────────────────┘

形态 B｜三栏（会话有 ≥1 个子智能体）
┌──────────┬──────────────────────┬──────────┐
│ Sidebar  │ MainHeader           │ AgentRail│
│ 240px    ├──────────────────────┤ 300px    │
│          │ 对话流                │ 智能体卡片│
│          ├──────────────────────┤          │
│          │ ChatInput            │ 合计 tok │
└──────────┴──────────────────────┴──────────┘

形态 C｜智能体详情（三栏，中栏被只读视图替换）
┌──────────┬──────────────────────┬──────────┐
│ Sidebar  │ ← 返回主控对话        │ AgentRail│
│ 240px    ├──────────────────────┤ 当前项高亮│
│          │ 落库后的只读记录       │          │
│          │ （没有输入框）         │          │
└──────────┴──────────────────────┴──────────┘
```

切换规则：

- `listSubAgents(activeSessionId)` 非空、当前是聊天页且右栏没有因窗口宽度收起时，显示形态 B；否则是 A。
- 点击右栏卡片进入 C；点击「返回主控对话」或再次点击同一张卡片退出详情。
- 切换父会话时清空 `agentFocus`，不能把上一棵 Agent 树带到新会话。
- 窗口宽度 `< 1100px` 先收右栏，`< 720px` 再收左栏。窗口重新变宽时右栏恢复；左栏由用户入口重新展开。

`MainHeader` 只回答“当前在哪里、运行到什么程度”，不放导出、暂停等动作。聊天页标题是 `项目名 / 会话标题`，副标题由以下片段组成：

```text
单智能体
主控 + 3 个子智能体 · 12 步 · 01:48
```

“步”是当前 Agent 树内各 Session **最近最多 50 条 Trace** 的模型调用数与工具调用数之和；耗时取树中 runtime 记录最长的一项。还没有数据的片段直接省略，不显示伪精度的 `0 步` 或 `--:--`。

### 2.2 左栏

- 普通聊天页展示品牌、新建会话、项目、项目下的会话和设置入口。
- 只有活动项目展开会话列表；每个会话行保持单行标题。
- 当前选中的会话或正在运行的会话在标题前显示状态点；等待审批用 warning 色，其余情况用 accent 色。
- 设置与运行记录使用另一套导航，提供返回聊天、模型、Skills、通用设置和运行记录入口。
- macOS 展开左栏时为原生红绿灯保留顶部空间；左栏收起后由中栏的 `SidebarReveal` 提供展开入口。

### 2.3 外壳文件

| 文件 | 职责 |
|---|---|
| `src/app/AppShell.tsx` | 两栏/三栏装配、页面切换、Agent 详情切换 |
| `src/app/MainHeader.tsx` | 面包屑与运行概况；不承载动作按钮 |
| `src/app/Sidebar.tsx` / `SidebarReveal.tsx` | 左栏与收起后的恢复入口 |
| `src/app/navigationStore.ts` | 页面、左右栏、Agent 焦点与消息跳转焦点 |
| `src/features/sessions/components/ProjectSection.tsx` | 新建会话、项目展开与打开目录 |
| `src/features/sessions/components/SessionItem.tsx` | 会话标题、活动状态、重命名与删除 |

## 3. Tauri Bridge 只做适配

**允许**：把 Command DTO 转成 Core API 调用；把 Core 结构化错误转成 `CommandError`；进程启动时订阅 Core Update 并 emit 给前端；调用目录选择、外部链接等 Host 能力。

**禁止**：

- 创建 Agent / Provider / Tool Executor；
- 保存 Turn 运行时状态；
- 拼接 Message；
- 根据 Trace 判断 Turn 状态；
- 为每次 `turn_start` 注册新的全局 listener；
- **在 Tauri Command 里等待完整 Turn 结束**。

Bridge DTO 目前手写在 `src/bridge/compat.ts`，必须与 Rust 侧同步维护；更新 Session Update 形状时同步提升 `RUNTIME_SESSION_UPDATE_VERSION`。契约代码生成尚未落地。

## 4. 状态三层

三层不能互相污染，这是前端最重要的边界。

### 4.1 canonical server state

```ts
interface SessionStoreState {
  summaries: Record<string, RuntimeSessionRecord>
  orderedSessionIds: string[]
  activeSessionId: string | null
  messagesBySession: Record<string, RuntimeStoredMessage[]>
  plansBySession: Record<string, RuntimeTurnPlan[]>
  loadStateBySession: Record<string, LoadState>
  isLoading: boolean
  error: string | null
}
```

这里的 Message 与 Plan 全部来自 Core 持久化结果，不放 `isStreaming`、临时 Request ID 或 Permission。子 Session 可以被 `loadSession` 加进这些映射，但不会进入顶层 `orderedSessionIds`。

同一 Session 的并发 reload 使用请求序号对账：后发请求完成后，早发请求不能覆盖新结果。

### 4.2 per-session runtime view

```ts
interface SessionRuntimeView {
  lastSequence: number
  turnId: string | null
  clientRequestId: string | null
  phase: SessionRuntimePhase
  pendingUserMessage: PendingUserMessage | null
  assistantDraft: AssistantDraft | null
  toolCalls: Record<string, RuntimeLiveToolCall>
  orderedToolCallIds: string[]
  pendingPermission: RuntimePermissionRequest | null
  permissionMode: RuntimePermissionMode
  plan: RuntimeTurnPlanSnapshot | null
  terminal: RuntimeTurnOutcome | null
  startedAtMs: number | null
  endedAtMs: number | null
  syncState: 'current' | 'stale' | 'resyncing'
  error: string | null
}
```

每个 Session 有独立 runtime view，没有全局唯一的 `activeStream`。

子 Agent Update 也走这层。`subAgentParentBySession` 标记出子 Session 后，Reducer 仍推进 sequence、phase、开始/结束时间与 terminal，但丢弃 text/reasoning、工具详情、Permission 与 Plan 等高体积实时数据；详情页需要完整内容时直接加载落库 transcript。

### 4.3 local UI state

页面导航、Sidebar/AgentRail 展开、Agent 焦点、Composer 草稿、Trace 选中项、主题、语言、Modal/Drawer 都是本地 UI 状态，不能写回 runtime view。

## 5. Runtime Reducer 与 Event Controller

```ts
function reduceSessionUpdate(
  state: SessionRuntimeView,
  envelope: RuntimeSessionUpdateEnvelope,
  isSubAgent?: boolean,
): SessionRuntimeView
```

Reducer 必须是纯函数：不调用 Tauri、不访问其他 Store、不 reload、不弹窗、不修改参数，并对 union variant 穷尽处理。

Sequence 的当前恢复策略：

| 情况 | 处理 |
|---|---|
| `sequence <= lastSequence` | 重复或旧事件，忽略 |
| `sequence = lastSequence + 1` | 正常应用 |
| `sequence > lastSequence + 1` | Controller 先调用 `replayUpdates(sessionId, lastSequence)`；仍有缺口或 replay 失败时再用 snapshot 替换 |
| 切换活动 Session | 主动读取 snapshot，并 reload canonical state |

App 根部只挂载一次 `useCoreEventBridge()`。它按 Session 将 `text_delta`、`reasoning_delta`、`tool_call_progress` 在 **100ms** 窗口内批量应用；批次不连续时退回逐条 Controller 路径，不能跨 sequence 拼接。

`processSessionUpdate` 负责副作用：

1. 校验版本；不支持时 snapshot 并标记 stale；
2. 处理 replay / snapshot 恢复；
3. 应用 Update；
4. `draft_cleared` 或 terminal 后 reload canonical Message/Plan 并对账；
5. terminal 后刷新 Session 列表。

当前实现没有额外的 `RuntimeEffect` union；副作用边界由 `coreEventController.ts` 本身承担。Controller 不把错误文本伪造成 Assistant Message。

## 6. Turn 发送、对账与 Transcript

```text
生成 clientRequestId
  → 从草稿收集仍有效的 SkillMentionBinding，形成 UserInput::Skill[]
  → runtime store 创建只含用户可见内容的 pending user message
  → turn_start：Skill 输入在前，原始 Text 输入在后
  → Accepted 或 turn_started（任一先到）绑定真实 turnId
  → Accepted 后仅在活动 Session 与草稿 revision 都没变化时清空草稿
  → Update 推进 draft / tool / permission / plan
  → draft_cleared 或 terminal 后 reload canonical state
  → canonical 到达后删除 pending 与 live draft
```

Accepted 与 `turn_started` 的到达顺序不作保证。若 `turn_start` 在 Accepted 前失败，pending 标记失败并保留错误，不创建 Assistant Draft、不伪造持久 Message。

发送期间输入框仍可编辑。清理草稿必须同时比较提交 Session 与单调递增 revision，不能只比较字符串，否则跨 Session 或 ABA 改动会清掉新草稿。Session 切换会重建 `ChatInput`，本地 Skill 绑定不能泄漏到另一会话。

Skill 绑定只存在于输入组件本地。提交时编码为 `UserInput::Skill`；Core 解析后的正文以 `message_kind = 'skill_instruction'` 持久化，Transcript 过滤它，只展示用户可见消息。完整契约见 [skills.md §4.2](skills.md)。

`buildTranscript` 负责把 canonical Message、乐观用户消息、live assistant draft、live tool result 和 Plan 投影成 `ChatItem[]`。它不写回任何 Store。

## 7. 对话流中的状态卡片

### 7.1 计划卡

`update_plan` 是状态更新，不按普通工具调用展示：

- 原始 Tool Call/Result、输入 JSON 和 `Plan updated` 都从 Transcript 隐藏；
- 会话只投影最新一份非空计划，卡片固定在该计划首次 `update_plan` 的消息位置；
- 后续调用更新同一张卡，并用 distinct tool call id 计算“已更新 N 次”；
- 状态变化的步骤高亮 600ms；步骤增删用布局动画；
- 完成步骤、进行中步骤和未开始步骤使用不同图标与文字层级；
- 全部完成后自动折叠成一行，可手动展开；
- 超过 12 步时，已完成部分默认折叠；
- 隐藏纯 Plan 消息时保留 `sourceMessageIds`，保证 Trace 的“打开消息”仍能定位。

计划真相来自 runtime `plan_updated` 完整快照和持久化 `turn_plans`，组件不会自行推进步骤，也不会从 Tool JSON 重建历史计划。

### 7.2 工具活动

`ToolActivityList` 先按 tool call id 配对 Call/Result，再对相邻、同名、非失败的调用分组。中间出现正文、thinking 或其他工具会截断分组；失败调用单独展示。展开状态按 tool call id 记忆。

| 工具 | 组件与默认形态 |
|---|---|
| `read` / `list` / `glob` | `ReadonlyToolActivityRow`，默认折叠 |
| `grep` | `ReadonlyToolActivityRow`，默认展开 |
| `edit` / `write` | `FileChangeActivityRow`，默认展开，展示两列行号 Diff 与撤销/重做 |
| `bash` | `BashToolActivityRow`，默认展开；失败保持展开 |
| `spawn_agent` | `AgentToolActivityRow`，相邻派发合并并默认展开任务名 |
| `wait_agent` | `AgentToolActivityRow`，相邻等待合并，成功默认折叠；失败单独展开 |
| 未识别工具 | 最小兜底行：扳手图标、工具名、展开后的 input/output |

只读、写入、失败三档外壳由 `ToolActivityFrame` 统一；状态判定和 `ToolActivity` 类型在 `toolActivity.ts`，避免底层 Frame 反向依赖列表组件。

完成一个 Turn 后，分散的 `edit`/`write` artifact 还会聚合成 `FileChangeCard`：

- 按路径合并同文件的多次修改；
- 显示项目相对路径、文件数与增删行数；
- 默认显示 3 个文件，其余可展开；
- 支持整批撤销/重新应用；
- “查看”打开 `FileChangeReviewDrawer`，按文件聚合并复用 `FileDiffPanel`，第一项默认展开。

### 7.3 审批卡

`ApprovalDialog` 位于输入框上方，直接读取当前 Session 的 `pendingPermission`，而不是插入 Transcript。

- 标题按最高风险描述后果：写入、工作区外写入、永久删除、force push 等；
- 已自动放行的工作区内只读项折叠成一行，不和待决策写入争夺视觉权重；
- `edit`/`write` 能从严格可证的工具输入生成预览时，复用 `FileDiffContent` 展示两列 Diff；预览不冒充完整文件快照；
- 路径优先显示相对工作区值，完整路径保留在 `title`；
- 原始命令默认折叠；
- 普通审批提供“允许一次 / 本会话始终允许 / 拒绝”；危险操作只提供“取消 / 仍要允许”，并默认聚焦取消；
- `Enter` 执行默认安全选择，`Esc` 拒绝；危险操作下两者都不会直接批准。

Permission 的结构化 effect、规则与 session action 来自 Core，Desktop 只做人话化和布局，不重新推导授权结果。详细权限语义见 [permissions.md](permissions.md)。

## 8. 多智能体视图

多智能体协议见 [multi-agent.md](multi-agent.md)。Desktop 不新增 Session Update 类型：子 Session 的 Update 通过同一条全局事件流到达，并携带自己的 `sessionId`。

### 8.1 右栏数据

`subAgentStore` 维护父会话的子 Session 列表和 Trace 汇总。右栏与其他消费者通过引用计数共享一份轮询：

- 挂载与父会话运行状态翻转时立即刷新；
- 父 Turn 运行中，子 Agent 列表每 1s 刷新，Trace 汇总每 3s 刷新；
- 父 Turn 空闲后停止定时器，但保留最后一次数据；
- 后发请求用序号保护，早发晚到的响应不能覆盖新结果；
- Trace 汇总读取失败只保留旧值，不影响会话运行。

右栏第一张是主控卡，其后是子智能体：

| 字段 | 来源 |
|---|---|
| 主控标题 / 副标题 | 本地化“主控” / 会话标题 |
| 子卡标题 / 副标题 | `agentRole` / `taskName` |
| 状态 | runtime phase/terminal；主控 terminal 被 canonical 对账清理后用最新 Turn Trace 状态补回 |
| 耗时 | runtime `startedAtMs` 到 `endedAtMs` 或当前时间 |
| token | `listTraces(sessionId, 50)` 的 `totalTokens` 合计 |
| 子卡 token 条 | 相对子智能体中最大 token 的比例，只表达用量对比，不表达任务进度 |

状态是离散值，没有“完成 62%”这种数据。`idle` 与 `completed` 在卡片上都归为“待命”；缺少开始/结束时间时显示 `--:--`，不虚构耗时。

### 8.2 只读详情

点击卡片后，中栏用 `loadSession(sessionId)` 读取持久化 messages，并按 user/tool/assistant 角色复用现有展示组件。详情页：

- 只有返回主控对话和只读 transcript；
- 没有输入框、重跑、直接下发、重命名或独立管理；
- 不提供 Trace、撤销/重做等会改变会话或工作区的交互；
- 子 Session 不出现在左侧顶层会话列表。

### 8.3 协作工具的前端覆盖

Core 暴露五个协作工具：`spawn_agent`、`wait_agent`、`list_agents`、`followup_task`、`interrupt_agent`。

当前聊天流只为用户需要持续观察的 `spawn_agent` 与 `wait_agent` 提供专用聚合卡；其余三个仍走未知工具兜底。Trace 时间线则为五个协作工具全部提供独立图标，`update_plan` 也使用计划图标，不退化成统一扳手。

## 9. Trace UI

Trace 是观测面，不复制数据库 Record，也不通过 Trace 更新 Chat Runtime。页面读取失败不能影响 Session 页面和普通聊天历史。

### 9.1 运行记录页

`TracePage` 首次读取最近 100 条，可逐次增加到 500 条；有运行中记录时每 3s 刷新，否则每 30s 刷新。页面顺序是：

1. 搜索框与完整状态下拉：全部、运行中、已完成、失败、已取消、已中断；
2. 当前已加载记录中的今日运行数、成功率、中位耗时、今日 token 合计；
3. 按北京时间日期分组的运行卡片。

搜索覆盖会话标题、工作目录、模型名、Session ID、Trace ID 与 Turn ID。卡片展示状态、目录、模型/工具调用数、模型、耗时、token 与相对时间；耗时条和 token 条只用于当前结果集内比较。

顶部统计同样只基于当前已加载的 100～500 条记录；如果当天运行数超过加载上限，它不是服务端全量日报。

列表按 `traceId` 组织，包含有 Turn 的运行和无 Turn 的独立 Trace：

| | 有 Turn | 无 Turn（手动压缩、rewind） |
|---|---|---|
| 模型/工具调用计数 | 显示 | 不显示，不能伪造成两个 0 |
| 压缩标记 | 无 | 显示 |
| 详情入口 | `getTrace(turnId)` | `getTraceById(traceId)` |

### 9.2 详情抽屉

详情采用宽右侧抽屉：顶部显示状态、总耗时、模型调用、工具调用、token 与 Trace 完整度；主体左侧是瀑布时间线，右侧是选中 Span 的详情。

```text
Trace Summary
├── Compaction（threshold/overflow，有 Turn 时）
│   └── Model Call（摘要采样）
├── Model Call #1
│   ├── Tool Call
│   └── Tool Call
├── Model Call #2
└── Orphan Tool Calls
```

Span 按 `startedAt` 排序，不依赖后端序号。时间线显示相对 Trace 起点的刻度、模型/工具耗时条和 Permission 决策标记；有子节点的组可以单独折叠。

详情右栏包括：

- Span 状态、耗时与调用序号；
- token 构成；cached 是 input 子集、reasoning 是 output 子集，条形图按互斥部分计算；
- 正文槽位；
- 主属性与其余记录属性；默认只显示前 8 项，其余用“展开全部”查看；
- 结构化错误。

属性白名单新增 key 时必须同时决定主次归类，并补齐三语标签。

### 9.3 正文按需加载

Span 详情提供 `request` / `system_context` / `tool_definitions` / `response` 四个槽位。

- 只有用户打开槽位时才调用 `getSpanPayload(spanId, slot)`；`getTrace` 不携带大正文。
- 成功 Model Call 的 response 在聊天记录里时，槽位直接跳转到对应 Message，不重复拉正文。
- 工具调用正文与 Compaction 自身没有对应槽位时，界面陈述存储位置或事实，不伪造内容。
- `truncated` 显示原始大小。
- 格式化正文默认最多进入 DOM 64K JavaScript 字符或 200 行；用户明确展开后才渲染全部。
- request 是消息数组时可在人类可读的消息视图和原始 JSON 之间切换。

Desktop 不提供 Trace 正文记录档位；Core 始终完整记录其支持的正文槽位。

聊天页还提供 `ContextWindowDrawer`：展示当前模型可见投影、token 估算与 Session 级压缩历史；运行中的上下文用量只做观测，不参与 Turn 状态判断。

## 10. 通用设置

通用设置目前只有两组：

- 外观：亮色、暗色、跟随系统，以及 zh-CN / zh-TW / en-US；
- 上下文窗口：应用级 token 数值，校验为正整数后持久化，并随 `turn_start` 传给 Core。

这里没有 Trace 内容记录档位，也不展示自动压缩阈值说明。手动 `/compact` 仍属于聊天命令，不是设置项。

## 11. 主题、时间与 i18n

### 11.1 主题

组件的面、文字、边框、强调与状态颜色使用 `src/app/theme/globals.css` 的语义 token：`paper*`、`surface`、`ink*`、`line*`、`clay*`、`status-*` 和 Trace 专用条形色。`clay` 是唯一 accent，用于主动作与当前选中项；状态颜色不冒充 accent。详情抽屉目前仍有固定 alpha 的 `box-shadow`，它只表达层级，不作为业务颜色。

组件不新增硬编码的 UI 色板。需要新视觉语义时先在亮/暗主题中定义 token，再映射 Tailwind 类。

### 11.2 时间

- 后端 RFC 3339 时间先用 `Date.parse` 取得瞬间；完整北京时间显示使用 `formatBeijingDateTime`。
- Trace 日期分组和时钟明确固定为 `Asia/Shanghai`。
- runtime 耗时使用毫秒数计算，再交给对应 presentation helper 格式化。
- 禁止通过 `slice` / `split` / `replace` 修改带时区的时间字符串。

### 11.3 i18n 与动画

- zh-CN / zh-TW / en-US 结构必须完全一致，测试会校验 key 结构。
- Trace 属性白名单里的每个 key 必须有三语标签。
- `AnimatePresence` 的直接子元素必须带稳定 key；否则退出节点会冻结旧 props，出现多个项目同时展开或高亮。

## 12. 当前刻意不做的内容

| 不做 | 原因 |
|---|---|
| 顶栏导出 / 暂停 | 当前产品没有对应动作 |
| 子智能体重跑 / 直接下发 | 会绕过主控，使任务图与实际执行脱节；详情保持只读 |
| 右栏费用 | Core 没有定价数据源 |
| 智能体百分比进度 | runtime 只有 phase/terminal，没有完成度 |
| 智能体产物字段 | runtime 没有统一产物概念 |
| 侧栏用户卡 | 应用没有账号体系 |
| 侧栏搜索 | 当前没有实现，不放不可用入口 |
| Trace 前端标注 | Core 有相关存储能力，但当前 Desktop 没有标注入口，不能写成已实现功能 |

## 13. 验收

1. App 根部只有一个 Event listener，切换 Session 不重复注册；
2. Reducer 是纯函数，对所有 update variant 穷尽处理；
3. 重复 sequence 被忽略，gap 先 replay，仍有缺口才 snapshot；
4. Accepted 与 `turn_started` 任一先到都能正确对账；
5. `turn_start` 失败时 pending 标记失败，且不产生 Assistant Draft；
6. canonical Message 到达后 pending 与 draft 被清除，不留重影；
7. 多个 Session 各自维护 runtime view，互不干扰；
8. UI 本地状态不写回 runtime view；
9. 会话有子智能体且窗口足够宽时出现右栏，切换会话会退出旧 Agent 详情；
10. 子智能体详情只读，子 Session 不出现在顶层会话列表；
11. `update_plan` 在消息流中始终只有一张最新计划卡，不出现 JSON 或 `Plan updated`；
12. 相邻同类工具正确分组，失败项不参与分组；
13. 写入摘要能按文件聚合、撤销/重做并打开复用的 Diff 审阅抽屉；
14. 审批卡默认突出真正需要决策的写入，危险操作默认选择取消；
15. Trace 页面读取失败不影响聊天页；
16. 无 Turn Trace 不显示伪造的调用数，并能通过 `traceId` 打开详情；
17. 正文只在打开槽位时请求，成功响应可以跳回聊天 Message；
18. 截断正文显示原始大小，默认 DOM 有 64K 字符 / 200 行上限；
19. 属性白名单里的 key 有主次归类与三语标签；
20. 三语资源结构一致，主题在亮暗模式下都只使用语义 token。
