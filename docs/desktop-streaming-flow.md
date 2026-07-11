# 桌面端流式事件与 UI 状态

Last reviewed: 2026-07-11

## 1. 相关代码

```text
apps/desktop/src/views/ChatView.tsx
apps/desktop/src/hooks/useChatStreamListener.ts
apps/desktop/src/stores/sessionStore.ts
apps/desktop/src/utils/streamAccumulator.ts
apps/desktop/src/type/providers.ts
apps/desktop/src/type/parts.ts
apps/desktop/src-tauri/src/lib.rs
```

## 2. 请求入口

`ChatView` 负责发起请求：

```text
用户输入
  -> pushUserMessage
  -> ensureStreamingItem
  -> chatGenerateStream
```

请求参数包含：

- `requestId`
- `sessionId`
- `providerId`
- `model`
- `userText`
- `approvalPolicy`

当前聊天界面默认使用 `approvalPolicy: "untrusted"`，因此工具调用会进入人工审批。

## 3. Tauri 事件

后端通过 `chat-stream-event` 向前端发送事件。TypeScript 类型在：

```text
apps/desktop/src/type/providers.ts
```

当前事件名包括：

- `llm_step_start`
- `llm_step_finish`
- `llm_finish`
- `text_start`
- `text_delta`
- `text_end`
- `reasoning_start`
- `reasoning_delta`
- `reasoning_end`
- `step`
- `tool_call_start`
- `tool_call_delta`
- `tool_call_end`
- `tool_result`
- `approval_request`
- `approval_resolved`
- `finished`
- `done`
- `cancelled`
- `doom_loop`
- `error`

## 4. 全局监听

`useChatStreamListener` 是全局单订阅：

```text
chat-stream-event
  -> 按 sessionId 分派
  -> approval_request 进入 approvalStore
  -> approval_resolved 从 approvalStore 移除对应审批
  -> done 触发 session reload
  -> cancelled / doom_loop 更新当前流状态
  -> 其他事件进入 sessionStore.applyStreamEvent
```

这样切换 session 时，旧 session 的流式事件不会因为组件卸载而丢失。

## 5. streamAccumulator

`streamAccumulator.ts` 是前端流式状态归并核心。它把单帧 payload 累积到对应 `requestId` 的 assistant item：

- `text_delta` 追加到 text part
- `reasoning_delta` 追加到 thinking part
- `tool_call_start` 新增 tool_call part
- `tool_call_delta` 追加 tool input JSON 片段
- `tool_call_end` 将 tool_call 标记为 `finished`
- `tool_result` 追加 tool_result，并兜底将同 id 的 tool_call 标记为 `finished`
- `error` / `cancelled` / `doom_loop` 终止 streaming

`tool_result` 的兜底很重要：只要工具结果已经回来，前端就不应该继续把对应 tool call 显示成 running。

## 6. 状态恢复

请求完成后，后端会发 `done`。前端收到后调用：

```text
sessionStore.reload(sessionId)
```

这会调用后端 Journal-backed `SessionStore`：从 PostgreSQL 的 `recorded_events` 重放 Thread/Turn/Message 事实，再返回兼容的 Message DTO。当前数据库不存在 `messages` 表。

`text_delta`、`reasoning_delta` 和 `tool_call_delta` 只服务当前实时渲染，不逐帧持久化。请求结束时，后端会把新增的 Assistant/Tool Message 和 Turn 终态写入 Journal；为了避免 reload 后又显示 running，工具执行完成后持久化的 `ToolCallState` 会标记为 `Finished`。

## 7. 审批 UI

`approval_request` 会进入 `approvalStore`。`ApprovalDialog` 只显示当前 active session 的 pending 审批。

用户操作：

```text
Allow / Deny
  -> providersApi.resolveApproval
  -> Tauri resolve_approval
  -> openwork-app::TurnSupervisor
  -> ResolveApproval(turn_id, approval_id)
  -> openwork-core Turn command inbox
```

拒绝后，Core 会把拒绝原因作为工具错误结果回传给模型。审批卡片使用内部 `ApprovalId`，同时携带 `TurnId`，不会按厂商 tool-call ID 做全局路由。

## 8. 当前建议补强

1. 给 `streamAccumulator.ts` 增加单元测试。
2. 区分 `finished`、`success`、`error`、`denied`、`cancelled` 的 UI 表达。
3. `done` 事件可以带 request summary，减少 reload 前后的短暂状态差。
4. 审批弹窗增加风险原因和路径/diff 预览。
