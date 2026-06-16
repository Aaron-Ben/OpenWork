# 会话持久化与 LLM Trace

Last reviewed: 2026-06-17

## 1. 相关代码

```text
crates/anvil-session/src/store.rs
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src/stores/sessionStore.ts
```

`anvil-session` 使用 SQLite 保存会话、消息和 LLM 事件。桌面端启动时在 app data 目录打开 `anvil.db`。

## 2. 表结构

当前主要表：

| 表 | 用途 |
| --- | --- |
| `sessions` | 会话元信息：标题、provider、model、working_dir、时间戳 |
| `messages` | 聊天消息，按 session 和 seq 排序 |
| `message_parts` | 每条 message 的 block 级拆分 |
| `llm_events` | 流式事件与 trace 数据 |

## 3. `messages`

`messages` 保存完整 message：

```text
id
session_id
role
parts_json
seq
created_at
```

`parts_json` 是 `Vec<ContentBlock>` 的 JSON。它是当前恢复聊天上下文的主路径。

## 4. `message_parts`

`message_parts` 是对 `messages.parts_json` 的拆分保存：

```text
id
session_id
message_id
part_index
part_json
created_at
```

当前 `append_messages` 会同时写：

- `messages.parts_json`
- `message_parts`

这样既保留简单加载路径，也给后续 block 级查询、渲染、调试留出空间。

## 5. `llm_events`

`llm_events` 保存流式事件：

```text
id
session_id
request_id
event
payload_json
seq
created_at
```

Tauri 在 `chat_generate_stream` 中接收到 `AgentEvent` 后，会映射成前端 payload，并调用 `append_llm_event` 持久化。

`seq` 在同一 session 内递增，用于恢复事件顺序。

## 6. Message 与 Event 的区别

`messages` 是“会话状态”，用于恢复聊天记录和继续对话。

`llm_events` 是“运行轨迹”，用于观测、调试和回放：

- 模型何时开始输出文本
- reasoning 是否出现
- tool call 参数如何流式到达
- 是否请求审批
- 用户是否批准
- 工具结果何时返回
- 请求如何结束

两者都需要。只保存 messages 会丢失过程；只保存 events 会让 UI 恢复和上下文构造变复杂。

## 7. 当前持久化流程

成功完成一次请求：

```text
chat_generate_stream
  -> 读取 session 和 history
  -> Agent::run
  -> 每个 AgentEvent 持久化到 llm_events
  -> Agent 返回 RunResult.messages
  -> 按 history_len 截取新增 messages
  -> append_messages
  -> emit done
  -> 前端 reload session
```

取消或 doom-loop：

- 尽量持久化截止时已有的 messages。
- 发出 `cancelled` 或 `doom_loop` 事件。
- 前端保留已显示的部分内容。

## 8. 当前缺口

### 8.1 缺少 `tool_runs`

目前工具执行结果会体现在 message 和 event 中，但没有专门的工具运行表。建议新增：

```text
tool_runs
  id
  session_id
  request_id
  tool_call_id
  name
  input_json
  approval_status
  started_at
  finished_at
  duration_ms
  output_json
  is_error
```

这会让工具观测比从 `llm_events` 反推更可靠。

### 8.2 事件 payload 仍是 UI payload

现在 `llm_events.payload_json` 存的是 Tauri 发给前端的 payload。短期可用，但长期可以考虑保存更接近 `AgentEvent` 的结构化事件，再在前端层做映射。

### 8.3 缺少 trace 查询接口

目前已有写入能力，但还没有专门的 UI 或 API 查询 trace。后续可以增加：

- 按 request_id 查看事件流
- 查看 token usage
- 查看工具调用耗时
- 查看审批决策
- 导出 JSON trace
