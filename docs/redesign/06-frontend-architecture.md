# OpenWork Desktop 前端重构设计

> 状态：V1 已实施。Desktop 已使用 Runtime Session Store、Update Sequence、Snapshot/Replay 和 V2 Trace；模型选择按 Session 固定模型只读展示。
>
> 范围：`apps/desktop/src` React 前端与 `apps/desktop/src-tauri` Host Bridge。
>
> 原则：前端是 Core Session Runtime 的协议客户端，不复制 Runtime 状态机，也不根据 Trace 推进 Turn。

## 1. 结论

后端重构完成后，前端不能继续复制旧的 `Turn → Step → ToolRun → Approval` 生命周期树。目标聚合仍叫 `Turn`，但含义收敛为一次用户输入触发的完整 Agent Loop，不再包含可恢复的 Step/ToolRun 子领域。

```text
Turn
Step
ToolRun
Approval
Recovery
```

目标前端只认识：

```text
Session
Turn
Message
SessionUpdate
ToolCall
PermissionRequest
TurnTrace
```

最重要的边界：

1. Tauri Command 用于提交命令和读取快照；
2. Tauri Event 用于接收有序 `SessionUpdate`；
3. 已持久化 Message 与流式草稿分开保存；
4. 一个纯 Reducer 把 SessionUpdate 应用到每 Session 的 Runtime View；
5. Permission 是 Runtime View 的一部分，不建立全局 Approval Recovery Store；
6. Trace 是独立查询页面，不进入 Chat reducer；
7. TypeScript DTO 从 Rust Host Contract 生成，禁止继续手写两套镜像类型。

## 2. 当前前端结构与问题

### 2.1 当前目录事实

技术栈：

- React 19；
- TypeScript 5.8；
- Vite 7；
- Tauri 2；
- Zustand 5；
- Tailwind CSS 4；
- Radix UI；
- Vitest；
- i18next；
- Motion。

当前关键路径：

```text
src/App.tsx
  -> AppShell
      -> useChatStreamListener
      -> Sidebar
      -> ChatView
          -> sessionStore
          -> approvalStore
          -> streamAccumulator
          -> TurnTracePanel

src/api/sessions.ts / providers.ts
  -> Tauri invoke/listen

src-tauri/commands/chat.rs
  -> OpenWorkApplication.turns().generate_stream(...)
  -> app.emit("chat-stream-event", payload)
```

### 2.2 ChatView 职责过多

`src/views/ChatView.tsx` 同时负责：

- Composer Draft；
- 当前模型选择；
- 乐观 User Message；
- 创建临时 Assistant Message；
- 启动/取消请求；
- Trace Summary 拉取；
- Trace Panel 选择；
- Conversation 导航；
- Tool Activity 组合；
- Permission UI 挂载。

它既是页面，又是 Runtime Controller 和 Query Controller。

### 2.3 sessionStore 混合两类状态

`src/stores/sessionStore.ts` 同时保存：

- 服务端 Session Summary；
- 服务端持久 Message；
- 乐观 User Message；
- 流式 Assistant Draft；
- 全局唯一 `activeStream`；
- Async Command；
- Stream Reducer 入口；
- Durable Approval 恢复。

持久数据与临时数据混在 `messagesBySession` 后，只能在 `done` 事件中重新加载整个 Session 来收口。

### 2.4 listener 直接跨 Store 编排

`useChatStreamListener` 直接操作：

```text
approvalStore.push/remove
sessionStore.applyStreamEvent
sessionStore.reload
sessionStore.setActiveStream
```

因此 Event Handler 不是纯数据转换，而是隐藏的应用服务。事件顺序、重复、缺口和 Session 切换很难单测。

### 2.5 TypeScript 复制了 Rust Legacy

当前 `src/type/session.ts` 和 `src/type/trace.ts` 包含：

```text
TurnLifecycleSnapshot
StepLifecycleSnapshot
ToolRunLifecycleSnapshot
PendingApprovalSnapshot
turn/step/model_attempt/transport_attempt/tool_run/approval/recovery Span
```

这些类型来自后端当前实现，而不是 UI 真正需要的数据。Rust 字段变化必须手工同步 TypeScript，编译器无法跨语言发现漂移。

### 2.6 Tauri Command 生命周期过长

`chat_generate_stream` 把回调传入 Runtime，并等待整个 Turn 完成后才返回。前端一次 `invoke` 从发送持续到最终回答，同时又靠全局 Event 获取中间状态。

目标应拆成：

```text
turn_start -> 快速返回 TurnAccepted
session-update event -> 持续接收运行状态
turn terminal event -> 结束
```

## 3. 从 grok-build 前端边界借什么

### 3.1 Pager 通过 ACP 消费 Runtime

`xai-grok-pager` 不直接调用 `SessionActor::process_conversation_turn`，而是通过 ACP：

- `session/new` / `session/load`；
- `session/prompt`；
- `session/update`；
- Permission Reverse Request；
- Prompt Response。

UI 和 Shell Runtime 可以分别演进。

### 3.2 AcpUpdateTracker 是独立转换器

`xai-grok-pager/src/acp/tracker.rs` 对 `AcpUpdateTracker` 的描述是：

```text
SessionUpdate -> scrollback mutation
```

Tracker 管理当前文本块、思考块、Pending Tool 和乱序 Tool Update，但不负责网络和 UI Render。这正是 OpenWork 当前 `streamAccumulator` 应扩展成的边界。

### 3.3 每个 Session 有自己的运行视图

Pager 的 Agent View/Tracker 绑定到一个 Session，而不是使用一个全局 `activeStream`。导航到其他 Session 不会改变原 Session 的 Runtime 所有权。

### 3.4 不照搬 Pager 的复杂度

OpenWork 不复制：

- Ratatui Event Loop；
- Scrollback Block 大量特化；
- ACP Leader/Reconnect；
- Subagent/Task/Compaction UI；
- Pager 的全量键盘/鼠标状态；
- `updates.jsonl` Replay。

只借协议隔离、per-session tracker、纯 Update 应用和权限往返。

## 4. 目标前后端数据流

```mermaid
sequenceDiagram
    participant UI as React
    participant Bridge as Tauri Bridge
    participant Core as OpenWorkCore
    participant Actor as SessionActor

    UI->>Bridge: turn_start(clientRequestId, sessionId, input)
    Bridge->>Core: start_turn(...)
    Core->>Actor: SessionCommand::StartTurn
    Actor-->>Core: TurnAccepted(turnId)
    Core-->>Bridge: TurnAccepted
    Bridge-->>UI: invoke result

    loop Turn Runtime
        Actor-->>Core: SessionUpdateEnvelope
        Core-->>Bridge: Core event subscription
        Bridge-->>UI: openwork://session-update
        UI->>UI: reduceSessionUpdate
    end

    opt Permission Required
        UI->>Bridge: permission_resolve(sessionId, turnId, toolCallId, decision)
        Bridge->>Core: typed command
        Core->>Actor: ResolvePermission
    end

    Actor-->>UI: turn_completed/failed/cancelled update
    UI->>Bridge: session_messages(sessionId, afterSequence?)
    Bridge-->>UI: canonical messages
```

关键变化：

- Tauri Command 不再持有一个长时间运行的回调；
- Core 在启动时建立一次 Event Subscription；
- Event 与发起页面生命周期无关；
- 切换 Session 不会取消或丢弃后台 Session 的更新。

## 5. 目标目录

```text
apps/desktop/
├── src/
│   ├── app/
│   │   ├── App.tsx
│   │   ├── AppShell.tsx
│   │   ├── bootstrap.ts
│   │   ├── navigationStore.ts
│   │   └── useCoreEventBridge.ts
│   ├── bridge/
│   │   ├── commands.ts
│   │   ├── events.ts
│   │   ├── errors.ts
│   │   └── generated.ts              # Rust DTO 生成，禁止手改
│   ├── features/
│   │   ├── sessions/
│   │   │   ├── api.ts
│   │   │   ├── sessionStore.ts       # 只保存 canonical server state
│   │   │   ├── selectors.ts
│   │   │   └── components/
│   │   ├── chat/
│   │   │   ├── runtimeStore.ts       # per-session ephemeral runtime
│   │   │   ├── runtimeReducer.ts     # 纯 SessionUpdate reducer
│   │   │   ├── transcript.ts         # canonical + draft 派生模型
│   │   │   ├── useTurn.ts
│   │   │   └── components/
│   │   ├── models/
│   │   │   ├── api.ts
│   │   │   ├── modelStore.ts
│   │   │   └── components/
│   │   ├── traces/
│   │   │   ├── api.ts
│   │   │   ├── traceViewModel.ts
│   │   │   └── components/
│   │   └── projects/
│   │       ├── projectStore.ts       # working_directory 的本地导航书签
│   │       └── components/
│   ├── shared/
│   │   ├── ui/
│   │   ├── hooks/
│   │   ├── i18n/
│   │   ├── theme/
│   │   └── lib/
│   └── main.tsx
└── src-tauri/src/
    ├── lib.rs
    ├── state.rs                      # Arc<OpenWorkCore>
    ├── event_bridge.rs               # Core Event -> Tauri Event
    ├── contracts.rs                  # Host DTO source of truth
    └── commands/
        ├── sessions.rs
        ├── turns.rs
        ├── models.rs
        ├── permissions.rs
        └── traces.rs
```

不需要为每个小组件建目录。只有拥有 API、状态或独立业务语言的 Feature 才建立边界。

## 6. Tauri Bridge

### 6.1 Bridge 只做适配

`src-tauri` 允许：

- 将 Tauri Command DTO 转成 Core public API；
- 把 Core 结构化错误转成 `CommandError`；
- 在进程启动时订阅 Core Event；
- 将 Event 序列化并 `emit` 给前端；
- 调用目录选择、外部链接等 Host 能力。

禁止：

- 创建 Agent/Provider/Tool Executor；
- 保存 Turn Runtime State；
- 拼接 Message；
- 根据 Trace 判断 Turn 状态；
- 为每次 `turn_start` 注册一个新全局 listener；
- 在 Tauri Command 中等待完整 Turn 结束。

### 6.2 目标 Command

```text
session_list
session_create
session_load
session_messages
session_rename
session_delete
session_snapshot

turn_start
turn_cancel
permission_resolve

model_list
model_create
model_update
model_delete
model_test

trace_list
trace_turn
```

删除：

```text
chat_generate_stream
chat_abort(request_id)
resolve_approval(turn_id, approval_id)
trace_span_detail（删除独立命令，V1 可以随 trace_turn 一次返回小型详情）
provider_activate（Session/Turn 直接引用 model_id）
```

### 6.3 turn_start

请求：

```ts
interface TurnStartInput {
  clientRequestId: string
  sessionId: string
  modelId?: string
  content: ContentBlock[]
}
```

响应：

```ts
interface TurnAccepted {
  clientRequestId: string
  sessionId: string
  turnId: string
  acceptedAt: string
}
```

只有 Core 已提交 Turn 和 User Message 后才返回 Accepted。之后的运行终态只通过 Event/Snapshot 获取。

`turn_started` Event 与 Tauri `invoke` 响应在前端的可见顺序不作保证。两者都携带 `clientRequestId/turnId`，Controller 必须支持 Event 先到或 Accepted 先到，并把同一个 Pending User Message 对账到同一个 Turn。

## 7. Host Contract 与类型生成

Rust `src-tauri/contracts.rs` 是 Host Contract 的唯一来源。至少生成：

```text
IDs
SessionSummary/SessionDetail
TurnSummary/TurnAccepted/TurnOutcome
Message/ContentBlock
SessionSnapshot/SessionPhase
SessionUpdateEnvelope/SessionUpdate
PermissionRequest/PermissionDecision
ModelSummary/ModelInput
TurnTraceSummary/TurnTrace/TraceSpan
CommandError
```

生成结果写入：

```text
src/bridge/generated.ts
```

规则：

- 文件头标记 generated，禁止手改；
- CI/本地 Build 运行绑定生成并校验生成内容与 Rust Contract 一致；
- Event 使用 tagged union；
- TypeScript reducer 使用穷尽 `switch`；
- Contract 变化必须同时更新 Rust 序列化测试和前端 reducer 测试；
- 页面和 Store 不直接 import Rust 内部 Persistence/Trace Record 形状。

在生成器落地前，临时手写类型只能位于 `bridge/compat.ts`，不得继续散落到 `type/providers.ts`、`type/session.ts` 和 `type/trace.ts`。

## 8. SessionUpdate 协议

统一 Event Channel：

```text
openwork://session-update
```

Envelope：

```ts
interface SessionUpdateEnvelope {
  version: 1
  sessionId: string
  turnId: string
  sequence: number
  occurredAt: string
  update: SessionUpdate
}
```

最小 Update Union：

```ts
type SessionUpdate =
  | { type: 'turn_started'; clientRequestId: string }
  | { type: 'phase_changed'; phase: SessionPhase }
  | { type: 'text_delta'; blockId: string; delta: string }
  | { type: 'reasoning_delta'; blockId: string; delta: string }
  | {
      type: 'tool_call_started'
      toolCallId: string
      providerCallId: string
      requestedName: string
    }
  | { type: 'tool_call_input_delta'; toolCallId: string; delta: string }
  | {
      type: 'tool_call_finished'
      toolCallId: string
      status: 'succeeded' | 'failed' | 'denied' | 'cancelled' | 'outcome_unknown'
    }
  | { type: 'tool_result'; toolCallId: string; result: ToolResultPreview }
  | { type: 'permission_requested'; request: PermissionRequest }
  | { type: 'permission_resolved'; toolCallId: string; decision: PermissionDecision }
  | { type: 'turn_completed'; outcome: TurnOutcome }
  | { type: 'turn_failed'; error: CommandError }
  | { type: 'turn_cancelled' }
```

不再发送：

```text
step
llm_step_start/finish
approval_id
tool_run_id
recovery
done + finished 双终态
```

Model Call Index 属于 Trace/诊断，不需要驱动聊天 UI。

## 9. 前端状态分层

### 9.1 Canonical Server State

`sessionStore` 只保存：

```ts
interface SessionStoreState {
  summaries: Record<string, SessionSummary>
  orderedSessionIds: string[]
  messagesBySession: Record<string, Message[]>
  loadStateBySession: Record<string, LoadState>
}
```

这里的 Message 全部来自 Core 持久化结果，不放 `isStreaming`、临时 Request ID 或 Permission。

### 9.2 Per-session Runtime State

```ts
interface RuntimeStoreState {
  bySession: Record<string, SessionRuntimeView>
}

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

每个 Session 都有独立状态。删除全局唯一 `activeStream`。

### 9.3 Local UI State

以下只属于 UI：

- 当前导航 View；
- Sidebar 展开；
- Composer Draft（按 Session 保存可选）；
- 当前 Trace Panel/Span；
- Theme/Language；
- Project Bookmark 展开状态；
- Modal/Tab/Waterfall Zoom。

它们不能写回 Session Runtime。

## 10. Runtime Reducer

```ts
function reduceSessionUpdate(
  state: SessionRuntimeView,
  envelope: SessionUpdateEnvelope,
): SessionRuntimeView
```

必须是纯函数：

- 不调用 Tauri；
- 不访问其他 Store；
- 不执行 reload；
- 不弹窗；
- 不修改参数；
- 对所有 Union Variant 穷尽处理。

Sequence 规则：

1. `sequence <= lastSequence`：重复或旧事件，忽略；
2. `sequence = lastSequence + 1`：正常应用；
3. `sequence > lastSequence + 1`：标记 `stale`，由 Controller 请求 `session_snapshot`；
4. Snapshot 替换 Runtime View 并设置新的 `lastSequence`；
5. V1 没有持久 Update Replay，不能假装补回缺失 Delta。

高频 Text/Reasoning Delta 可以由 Event Bridge 在一个 animation frame 内合并后再进入 Zustand，但必须保持 Block 和 Sequence 顺序。

## 11. Event Controller

App 根部只挂载一次：

```ts
useCoreEventBridge()
```

职责：

1. 订阅 `openwork://session-update`；
2. 校验 Envelope Version 和最小字段；
3. 调用 `runtimeStore.apply(envelope)`；
4. 检测 Sequence Gap 时请求 Snapshot；
5. Turn Terminal 时使 canonical messages/session summary/trace summary 失效并刷新；
6. 组件卸载时解除 listener。

它不直接操作 Permission Store，也不把错误文本拼入 Assistant Message。

刷新动作放在独立 Effect Controller 中；Reducer 只返回状态和可选 Effect 描述：

```ts
type RuntimeEffect =
  | { type: 'refresh_messages'; sessionId: string }
  | { type: 'refresh_session_summary'; sessionId: string }
  | { type: 'refresh_trace'; turnId: string }
  | { type: 'request_snapshot'; sessionId: string }
```

## 12. Turn 发送与乐观 UI

发送流程：

1. 前端生成 `clientRequestId`；
2. Runtime Store 创建 Pending User Message；
3. 调用 `turn_start`；
4. Accepted 或 `turn_started` Event 先到时，都把 `clientRequestId` 绑定到真实 `turnId`；
5. Event 更新 Assistant Draft/Tool/Permission；
6. Terminal 后拉取 canonical Message；
7. canonical Message 到达后删除 Pending/Draft。

若 `turn_start` 在 Accepted 前失败：

- Pending User Message 标记失败；
- 允许用户 Retry；
- 不创建 Assistant Draft；
- 不伪造持久 Message。

若 Terminal Event 丢失：

- `session_snapshot` 返回 Idle/Terminal 状态；
- Controller 刷新 canonical Message；
- 不根据 `invoke` Promise 是否结束推断 Turn 终态。

## 13. Transcript View Model

页面不直接渲染两个 Store：

```ts
buildTranscript(
  canonicalMessages,
  runtimeView,
): TranscriptItem[]
```

输出：

```text
UserMessageItem
AssistantMessageItem
ToolActivityGroup
PendingUserMessageItem
AssistantDraftItem
InterruptedToolResultItem
```

规则：

- canonical Message 不修改；
- Assistant Message 内的 Tool Call 与后续 Tool Message 只在 View Model 中分组；
- 配对键使用 `providerCallId`/`toolCallId`，不使用数组相邻位置猜测；
- `outcome_unknown` 使用明确警告样式，不能渲染成普通失败；
- 运行 Draft 和 terminal canonical Message 不重复显示；
- Trace Link 使用 `turnId + providerCallId`。

当前 `mergeToolMessages` 可以保留为纯函数起点，但应迁到 `features/chat/transcript.ts`，输入改为目标 Message Contract。

## 14. Permission UI

删除全局 `approvalStore`。当前 Session 的 Permission 从以下 Selector 读取：

```ts
runtimeStore.bySession[activeSessionId].pendingPermission
```

操作：

```text
Allow -> permission_resolve(..., "allow")
Deny  -> permission_resolve(..., "deny")
Cancel Turn -> turn_cancel(...)
```

必须同时传：

```text
sessionId
turnId
toolCallId
decision
```

Core 校验决定是否仍有效。前端不能在 invoke 成功前自行删除 Permission；等待 `permission_resolved` Event 或 Snapshot。

可访问性：

- Permission Card 使用 `role="alertdialog"` 或语义等价的 Radix Dialog；
- 打开时把焦点移到标题/默认安全操作；
- Escape 等价于 Cancel/Deny 的行为必须明确；
- 关闭后焦点返回 Composer；
- Allow/Deny 有文本标签，不能只用颜色；
- 同一 Session 一次只展示 Runtime 提供的当前 Permission。

## 15. 多 Session 行为

目标允许多个 Session 在同一进程中分别有活动状态：

- Runtime Store 按 `sessionId` 分区；
- Sidebar 显示 `running/waiting_permission/failed` 状态点；
- 切换 Session 不取消原 Turn；
- 非当前 Session 的 Permission 在 Sidebar 显示 Needs Input；
- 用户进入该 Session 后显示 Permission Card；
- Cancel 只作用于指定 Session/Turn；
- Session 删除前若仍运行，由 Core 返回冲突或先显式取消。

若产品决定 V1 全局只允许一个活动 Turn，也由 Core 返回结构化冲突；前端仍不使用一个无法扩展的全局 `activeStream` 表达该规则。

## 16. Chat 页面组件

目标拆分：

```text
ChatPage                         # 页面组合/路由参数
├── ConversationViewport        # 滚动和空状态
│   ├── Transcript
│   │   ├── UserMessage
│   │   ├── AssistantMessage
│   │   └── ToolActivityGroup
│   └── ConversationNavigator
├── TurnComposer              # Draft/Model/Send/Cancel
├── PermissionCard              # 当前 Session Permission
└── TurnTraceDrawer           # 独立 Lazy Query
```

`ChatPage` 只通过 Feature Hook：

```ts
useSession(sessionId)
useTranscript(sessionId)
useSessionRuntime(sessionId)
useTurnActions(sessionId)
useTurnTraceSelection()
```

不要在组件内直接调用 `invoke/listen`。

## 17. Sidebar 与导航

当前 `Sidebar.tsx` 超过 500 行，混合 Project、Session、Settings 和动画。目标拆成：

```text
Sidebar
├── ProjectSection
│   ├── OpenProjectButton
│   └── ProjectRow
│       └── SessionList
│           └── SessionRow
└── SettingsNavigation
```

状态来源：

- Project Bookmark 展开：`projectStore` + localStorage；
- Session Summary：`sessionStore`；
- Session Activity Badge：`runtimeStore`；
- 当前 View/Trace/Session：`navigationStore`。

Project Bookmark 只是 `working_directory` 的本地导航偏好，不是后端领域事实。Core 把工作目录放入 `ToolContext`，实际路径规范化和安全校验由 `openwork-tools` 完成；前端字符串 normalization 只用于显示分组。

## 18. Model Settings

目标后端合并为 `models` 表后，前端同步改为：

```text
ProviderSettings -> ModelSettings
providerStore     -> modelStore
ProviderConfig    -> ModelSummary/ModelInput
activeProvider    -> session.defaultModelId / composer.modelId
```

不再维护一个全局 Active Provider。模型选择优先级：

1. Composer 显式选择；
2. Session `defaultModelId`；
3. Core 配置的默认可用 Model；
4. 没有模型时禁用发送并引导进入 Model Settings。

凭证只通过创建/更新表单提交；读取 API 永不把 Secret 或 Credential Ref 解析内容返回给前端。

## 19. Trace UI

当前 Trace UI 完整复制旧 Span Tree。目标类型：

```ts
type TraceSpanKind = 'model_call' | 'tool_call'

interface TurnTrace {
  summary: TurnTraceSummary
  spans: TraceSpan[]
  completeness: TraceCompleteness
}
```

页面结构：

```text
Turn Trace Summary (不是 Span)
├── Model Call #1
│   ├── Tool Call
│   └── Tool Call
├── Model Call #2
└── Orphan Tool Calls（父 Model Span 缺失时）
```

删除 UI 概念：

```text
Turn Root Span
Step
Transport Attempt Row
Approval Row
Recovery Row
Recovered Badge
Approval Count
Step Count
```

保留：

- Model/Tool Call Tree；
- Waterfall（如仍有诊断价值）；
- Duration/Token/Attempt Count；
- Permission Wait Duration；
- requested/resolved Tool Name；
- Error/Outcome Unknown；
- Trace Completeness；
- 从 Tool Call 跳回聊天活动。

遵守 [04-trace-design.md](04-trace-design.md) 的隐私边界：默认详情不显示完整 System/User Prompt、Tool Input、Tool Output 或 Provider Body。当前 `input_output` Tab 必须删除或只显示经过后端白名单化的大小/形状摘要。

Trace Query 不再绑定 `activeStream.requestId`。只在以下时机失效：

- `turn_completed/failed/cancelled`；
- 用户手动刷新；
- 打开详情时尚未加载。

## 20. Error、Loading 与降级

统一结构化错误：

```ts
interface CommandError {
  code: string
  message: string
  retryable: boolean
  details?: Record<string, unknown>
}
```

边界：

- Bridge 解析错误；
- Store 保存 error state；
- 页面决定展示 Toast/Inline Error/Retry；
- Reducer 不把错误文本追加成 Assistant Message；
- Error Boundary 只处理 Render 异常，不处理业务错误；
- Event Version 不支持时停止应用该 Session 更新并请求 Snapshot/显示升级错误。

## 21. 性能

V1 优先做：

- Zustand 使用细粒度 Selector；
- canonical Message 与 Runtime Draft 分离，避免每个 Delta 复制整段历史；
- 每 animation frame 批量应用 Text/Reasoning Delta；
- Transcript Item 使用稳定 ID 和 `React.memo`；
- Trace Drawer 按需加载；
- Session List 只订阅各自 Summary/Runtime Badge；
- 历史消息按 sequence 分页，先加载尾部。

长会话达到实际性能阈值后再引入列表虚拟化。不能在没有测量前让虚拟化破坏动态高度、代码块和 Trace Reveal 的滚动定位。

## 22. 可访问性

- Transcript 使用 `aria-live="polite"`，但 Token Delta 不应逐字符触发屏幕阅读器；按块或节流更新；
- Send/Cancel 在视觉和语义上是同一位置的状态切换；
- Tool Activity 可通过键盘展开；
- Outcome Unknown 具有文本说明；
- Trace Tree 使用 Tree/Treeitem 或普通按钮列表的完整键盘导航；
- Drawer/Dialog 打开关闭管理焦点；
- Respect `prefers-reduced-motion`；
- 状态不只依赖颜色；
- 所有新字符串进入三套 i18n locale，并保留结构一致性测试。

## 23. 当前到目标映射

| 当前文件/类型 | 目标 | 处理 |
| --- | --- | --- |
| `views/ChatView.tsx` | `features/chat/ChatPage.tsx` + 子组件/Hook | 拆发送、Trace、Transcript |
| `stores/sessionStore.ts` | canonical `sessionStore` + `runtimeStore` | 分开持久与临时状态 |
| `stores/approvalStore.ts` | `runtimeStore.pendingPermission` | 删除独立 Store |
| `hooks/useChatStreamListener.ts` | `app/useCoreEventBridge.ts` | 单订阅 + Sequence/Resync |
| `utils/streamAccumulator.ts` | `runtimeReducer.ts` | 扩展为纯目标协议 Reducer |
| `type/providers.ts` Live Event | `bridge/generated.ts` | Rust 生成 |
| `type/session.ts` | generated Session/Message/Turn | 删除 Step/Recovery |
| `type/trace.ts` | generated Turn Trace | 只保留 Model/Tool |
| `api/sessions.ts` | feature API + bridge commands | 不再混合 Chat/Trace |
| `api/providers.ts` | `features/models/api.ts` + bridge events | 移除 listener 和 Active Provider |
| `Sidebar.tsx` | Project/Session/Settings 子组件 | 拆分 500+ 行组件 |
| `TurnTracePanel.tsx` | `TurnTraceDrawer.tsx` | 新 Trace Contract |
| `TraceSpanDetail.tsx` | 精简 Model/Tool Detail | 删除 raw input/output |
| `src-tauri/commands/chat.rs` | turns/permissions commands | Command 快速返回 |
| 每次命令 callback emit | `event_bridge.rs` 单 Core 订阅 | 删除重复 Emit 路径 |

## 24. 迁移顺序

### Phase F0：冻结前端行为

- 为 Stream Reducer 增加所有 Event 测试；
- 为 Session 切换期间持续流式更新增加测试；
- 为 Permission Allow/Deny/Cancel 增加测试；
- 记录现有 Tauri Command/Event Contract；
- `pnpm test` 与 `pnpm build` 通过。

### Phase F1：生成 Host Contract

- 定义目标 Rust DTO；
- 生成 `bridge/generated.ts`；
- 新增 Contract Drift Check；
- 旧 Type 暂时通过 Compat Adapter 转换。

### Phase F2：Runtime Store 与纯 Reducer

- 建立 per-session `runtimeStore`；
- 实现 Sequence/Dedup/Gap；
- 移动 `streamAccumulator` 测试；
- 保持旧 Event Adapter，页面暂不改。

### Phase F3：Tauri Event Bridge

- Core 启动时单订阅；
- `turn_start` 快速返回；
- 新 `openwork://session-update` Event；
- `session_snapshot` Resync；
- 旧 `chat-stream-event` 暂时兼容但不再增加 Variant。

### Phase F4：Chat/Permission 切换

- canonical Message 与 Runtime Draft 分开；
- ChatPage 使用新 Hooks；
- 删除全局 `activeStream`；
- 删除 `approvalStore`；
- Terminal 后做 canonical reconciliation。

### Phase F5：Model/Trace/Sidebar

- Provider UI 改 Model UI；
- Trace 改 Turn/ModelCall/ToolCall；
- Sidebar 拆组件并接 Runtime Badge；
- 删除 Legacy Type、API 和 i18n Key。

### Phase F6：删除兼容层

- 删除 `chat_generate_stream/chat-stream-event`；
- 删除 Legacy `TurnLifecycle/StepLifecycle/ToolRunLifecycle/ApprovalRecovery` Type，保留目标 `TurnSummary/TurnOutcome`；
- 删除 Compat Adapter；
- 生成文件内容与 Rust Contract 一致；
- 运行最终构建和手工流程。

## 25. 测试矩阵

### 纯函数

- Text/Reasoning Block 顺序；
- Tool Call Start/Delta/Finish；
- Tool Result 配对；
- Permission Requested/Resolved；
- Turn Terminal；
- 重复 Event；
- Sequence Gap；
- Snapshot Replace；
- Outcome Unknown；
- canonical + runtime Transcript 合并。

### Store

- 两个 Session 同时更新互不污染；
- 切换 Session 不丢原 Session Draft；
- Terminal 只清目标 Session；
- Permission 只属于目标 Session；
- canonical refresh 不重复 Draft；
- 删除 Session 清理对应 Runtime View。

### Component

- TurnComposer Send/Cancel；
- Permission Focus 与键盘操作；
- Tool Activity 状态；
- Outcome Unknown 警告；
- Sidebar Activity Badge；
- Turn Trace 两类 Span；
- Trace partial/orphan；
- 三种语言 Key 一致。

### Bridge/Contract

- 每个 Command 参数名和 serde camelCase 一致；
- Rust Event 可以反序列化到生成的 tagged union fixture；
- Unsupported Version 明确失败；
- Event 先于/后于 TurnAccepted 到达都能按 `clientRequestId` 正确对账；
- 单 Event Bridge 不重复订阅；
- Structured CommandError 保持 code/retryable。

## 26. 验证命令

```sh
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
cargo test -p openwork-desktop
cargo check -p openwork-desktop
```

最终结构检查：

```sh
rg -n 'TurnLiveEvent|TurnLifecycle|StepLifecycle|ToolRunLifecycle|ApprovalRecovery' apps/desktop/src
rg -n 'chat-stream-event|chat_generate_stream|approvalStore|activeStream' apps/desktop/src apps/desktop/src-tauri/src
rg -n "invoke\(|listen\(" apps/desktop/src --glob '!bridge/**'
```

结果应为空，或只存在于明确的 Compatibility Test 中。

## 27. 前端完成标准

- Chat 页面不直接调用 Tauri；
- App 只建立一个 Core Event Subscription；
- 每个 Session 有独立 Runtime View；
- canonical Message 不包含流式临时字段；
- Update Reducer 是纯函数且处理 Sequence Gap；
- Permission 不依赖全局 Approval Store；
- Turn Command 快速返回，终态来自 Event/Snapshot；
- TypeScript Contract 由 Rust 生成；
- UI 中不存在 Step/ToolRun/Recovery；
- Trace 只展示 Turn、Model Call、Tool Call；
- Trace/Update 缺失不改变 Message 和 Turn 终态；
- Session 切换、权限等待和后台运行都可预测；
- Vitest、TypeScript Build 和 Tauri Check 全部通过。
