# 桌面端

Tauri 2 + React + TypeScript。React 只通过 Tauri Command/Event 与 Core 通信，**不访问 SQL、Provider Adapter 或 Tool Executor**。

## 1. 目录

```text
desktop/
├── src/
│   ├── app/          Bootstrap、导航、唯一的 Event Bridge
│   ├── bridge/       commands.ts / compat.ts（Host DTO）
│   ├── features/
│   │   ├── sessions/ canonical Session 与 Message
│   │   ├── chat/     per-session runtime view + reducer
│   │   ├── models/
│   │   ├── traces/
│   │   └── projects/
│   ├── components/   共享 UI
│   ├── lib/          dateTime 等工具
│   └── i18n/
└── src-tauri/src/
    ├── lib.rs        进程入口与 Command 注册
    ├── commands/     短生命周期 Command 适配
    └── error.rs      Core 错误 → CommandError
```

**只有拥有 API、状态或独立业务语言的 Feature 才建目录**，不为每个小组件建目录。

## 2. Tauri Bridge 只做适配

**允许**：把 Command DTO 转成 Core API 调用；把 Core 结构化错误转成 `CommandError`；进程启动时订阅 Core Update 并 emit 给前端；调用目录选择、外部链接等 Host 能力。

**禁止**：

- 创建 Agent / Provider / Tool Executor；
- 保存 Turn 运行时状态；
- 拼接 Message；
- 根据 Trace 判断 Turn 状态；
- 为每次 `turn_start` 注册新的全局 listener；
- **在 Tauri Command 里等待完整 Turn 结束**。

Bridge DTO 目前是**手写**在 `src/bridge/compat.ts`，必须与 Rust 侧手动保持同步；更新形状时同步提升 `RUNTIME_SESSION_UPDATE_VERSION`。契约代码生成尚未落地。

## 3. 状态三层

三层不能互相污染，这是前端最重要的边界。

### canonical server state

```ts
interface SessionStoreState {
  summaries: Record<string, SessionSummary>
  orderedSessionIds: string[]
  messagesBySession: Record<string, Message[]>
  loadStateBySession: Record<string, LoadState>
}
```

这里的 Message **全部来自 Core 的持久化结果**，不放 `isStreaming`、临时 Request ID 或 Permission。

### per-session runtime view

```ts
interface SessionRuntimeView {
  lastSequence: number
  turnId: string | null
  clientRequestId: string | null
  phase: SessionPhase
  pendingUserMessage: PendingUserMessage | null
  assistantDraft: AssistantDraft | null
  toolCalls: Record<string, LiveToolCall>
  orderedToolCallIds: string[]
  pendingPermission: PermissionRequest | null
  terminal: TurnOutcome | null
  syncState: 'current' | 'stale' | 'resyncing'
}
```

**每个 Session 有独立状态**，没有全局唯一的 `activeStream`。

### local UI state

当前导航、Sidebar 展开、Composer 草稿、Trace 面板选中项、主题、语言、Modal/Tab。**它们不能写回 runtime view。**

## 4. Runtime Reducer

```ts
function reduceSessionUpdate(
  state: SessionRuntimeView,
  envelope: SessionUpdateEnvelope,
): SessionRuntimeView
```

必须是**纯函数**：不调用 Tauri、不访问其他 Store、不执行 reload、不弹窗、不修改参数、对所有 union variant 穷尽处理。

Sequence 规则：

| 情况 | 处理 |
|---|---|
| `sequence <= lastSequence` | 重复或旧事件，忽略 |
| `sequence = lastSequence + 1` | 正常应用 |
| `sequence > lastSequence + 1` | 标记 `stale`，由 Controller 请求 snapshot |
| snapshot 到达 | 替换整个 runtime view 并设置新的 `lastSequence` |

**没有持久 Update Replay，不能假装补回缺失的 Delta。**

高频 Text/Reasoning Delta 可以由 Event Bridge 在一个 animation frame 内合并后再进 Store，但必须保持 block 与 sequence 顺序。

## 5. Event Controller

App 根部**只挂载一次** `useCoreEventBridge()`：

1. 订阅 session update 事件；
2. 校验 envelope version 与最小字段；
3. 调用 `runtimeStore.apply(envelope)`；
4. 检测到 sequence gap 时请求 snapshot；
5. Turn terminal 时使 canonical messages / session summary / trace summary 失效并刷新；
6. 卸载时解除 listener。

它**不**直接操作 Permission Store，也**不**把错误文本拼进 Assistant Message。

刷新动作放在独立的 Effect Controller，Reducer 只返回状态和可选的 effect 描述：

```ts
type RuntimeEffect =
  | { type: 'refresh_messages'; sessionId: string }
  | { type: 'refresh_session_summary'; sessionId: string }
  | { type: 'refresh_trace'; turnId: string }
  | { type: 'request_snapshot'; sessionId: string }
```

## 6. Turn 发送与乐观 UI

```text
生成 clientRequestId
  → runtime store 创建 pending user message
  → 调用 turn_start
  → Accepted 或 turn_started 事件（任一先到）都把 clientRequestId 绑定到真实 turnId
  → 事件更新 draft / tool / permission
  → terminal 后拉取 canonical message
  → canonical 到达后删除 pending 与 draft
```

**Accepted 与 `turn_started` 事件的到达顺序不作保证**，两者都携带 `clientRequestId` 和 `turnId`，Controller 必须支持任一先到，并把同一条 pending 消息对账到同一个 Turn。

若 `turn_start` 在 Accepted 前失败：pending 消息标记失败、允许重试、**不创建 Assistant Draft、不伪造持久 Message**。

## 7. Trace UI

消费 `model_call` / `tool_call` / `compaction` 三类 Span，**不复制数据库 Record，也不通过 Trace 更新 Chat Runtime**。

```text
Turn Trace Summary（不是 Span）
├── Compaction（threshold/overflow，有 Turn 时才在这里）
│   └── Model Call（摘要采样，子 Span）
├── Model Call #1
│   ├── Tool Call
│   └── Tool Call
├── Model Call #2
└── Orphan Tool Calls（父 Model Span 缺失时）
```

Span 之间按 `startedAt` 排序，**不依赖后端序号**——后端不提供 `sequence`，理由见 [trace.md](trace.md)。

**手动压缩与 rewind 没有 Turn，不出现在这棵树里**，但它们出现在运行记录列表中。

运行记录列表按 `traceId` 而不是 Turn 组织：`listTraces` 返回两路来源的并集——有 Turn 支撑的 Trace，以及无 Turn 的独立 Trace（手动压缩、rewind）。后者的 `turnId` / `turnSequence` 为 `null`，调用计数为 0。

因此列表行有两种形态：

| | 有 Turn | 无 Turn |
|---|---|---|
| 模型/工具调用计数 | 显示 | **不显示**（没有可言，不能渲染成两个 0） |
| 压缩标记 | 无 | 显示 |
| 详情入口 | `getTrace(turnId)` | `getTraceById(traceId)` |

**不要为了让它可点击而伪造一个 Turn。** 两种形态走各自的查询，共用同一个详情抽屉。

压缩的完整历史（触发类型、耗时、前后 Token 与回收量）另在聊天页上下文抽屉的「压缩历史」区块展示，走 Session scope 查询。

三类 Span 共用同一个详情面板，差异只有图标、标题取值、时间条颜色和折叠区标签。

Trace 页面读取失败**不能**影响 Session 页面和普通聊天历史。

### 属性分主次

**详情面板默认只显示主字段，其余进「详细」折叠区。**

| kind | 主字段 |
|---|---|
| `model_call` | 模型、耗时、token、`finishReason`、`temperature` |
| `tool_call` | 工具名、`permissionDecision`、`executionMs`、状态 |
| `compaction` | `trigger`、前后 token、回收量、尝试次数 |

一个平铺 20 多行属性的面板等于没有面板。**这条比减少属性数量更重要**——删减解决"字段太多"，分主次解决"不知道哪几个重要"，后者才是实际用起来的障碍。

新增属性时**必须决定它属于主还是次**，默认进折叠区。属性白名单加了 key 却没归类，就会又平铺回去。

### 正文按需加载

Span 详情面板有一个正文区，展示 `request` / `system_context` / `tool_definitions` / `response` 四个槽位。

**它必须在展开时才调 `getSpanPayload(spanId, slot)`，不能随 `getTrace` 一起拉。** 一次 Turn 的全部请求正文可以有几 MB，而用户多数时候只想看时间线。列表和时间线靠 `requestMessageCount` 与 `byteSize` 显示规模。

三条渲染规则：

- **`truncated` 必须显示成"已截断，原始 N KB"**，不能只显示一个省略号——用户要能判断被切掉的部分重不重要；
- **正文缺失只能陈述事实，不能声称原因**：显示"无正文记录"。若**当前**设置不是 `full`，可以补一句"内容记录当前已关闭"，措辞必须指向当下而不是那条 Span 记录时——策略不随 Span 持久化，声称历史原因就是猜测。理由见 [trace.md](trace.md) §12；
- **成功的 Model Call 没有 `response` 槽位**，它的响应在聊天记录里——面板应给一个跳到那条 Message 的入口，而不是显示"无内容"。

正文格式化后的默认 DOM 上限为 **64K 个 JavaScript 字符或 200 行（先到者为准）**。超过上限只渲染头部预览和“展开全部”入口；完整内容只有在用户明确展开后才进入 DOM。这个上限不改变数据库的 1 MiB 槽位上限。

内容策略由前端本地持久化。应用启动时先把该策略推给 Core，成功后才渲染可发起 Turn 的界面；运行中保存设置会原子更新 Recorder，后续写入立即生效，无需重启。

### 标注

Trace 详情顶部有 好 / 不确定 / 差 三档加一个可选备注，Span 行上也可以单独标。

改评价是覆盖，不是追加。已标注的 Trace 在列表上有标记——它们不会被自动清理，用户应该看得出来哪些被留住了。

## 8. 时间显示

后端返回的时间字符串是带 `+08:00` 的 RFC 3339，前端**只有两种合法操作**：

```ts
Date.parse(value)                    // 拿到瞬间
formatBeijingDateTime(value)         // 显示
```

**禁止对时间字符串做 slice / split / replace / 正则。** 新增时间显示一律走 `src/lib/dateTime.ts`，不在组件里各自 `new Intl.DateTimeFormat`。

## 9. i18n

三种语言（zh-CN / zh-TW / en-US）**结构必须完全一致**，有测试强制。Trace 属性白名单里的每个 key 都必须有三语标签——新增属性时漏掉会直接测试失败。

## 10. 验收

1. App 根部只有一个 Event listener，切换 Session 不重复注册；
2. Reducer 是纯函数，对所有 update variant 穷尽处理；
3. 重复 sequence 被忽略，gap 触发 snapshot 而不是伪造 Delta；
4. Accepted 与 `turn_started` 任一先到都能正确对账；
5. `turn_start` 失败时 pending 消息可重试，且不产生 Assistant Draft；
6. canonical Message 到达后 pending 与 draft 被清除，不留重影；
7. 多个 Session 各自维护 runtime view，互不干扰；
8. UI 本地状态不写回 runtime view；
9. Trace 页面读取失败不影响聊天页；
10. 手动压缩能在界面上看到**并能打开详情看到摘要正文**；
11. 正文只在展开槽位时请求，打开 Trace 详情不触发正文查询；
12. 截断的正文显示原始大小；正文缺失的文案只陈述事实，不声称历史原因；
13. 成功 Model Call 的响应区给出跳转到聊天记录的入口，而不是"无内容"；
14. 标注是覆盖式的，已标注的 Trace 在列表上可辨认；
15. Span 详情默认只显示主字段，其余在折叠区内；属性白名单里的每个 key 都有归类；
16. 时间显示全部走 `dateTime.ts`，无字符串手术；
17. 三语资源结构一致，无缺失 key。
