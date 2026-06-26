# Agent Runtime 与工具调用流程

Last reviewed: 2026-06-17

## 1. 相关代码

```text
crates/openwork-runtime/src/router.rs
crates/openwork-tools/src/tool.rs
crates/openwork-tools/src/registry.rs
crates/openwork-tools/src/builtin/
apps/desktop/src-tauri/src/lib.rs
```

`openwork-runtime` 负责 agent loop。它接收历史消息，调用 provider，处理模型返回的 tool calls，执行工具，再把工具结果回填给模型，直到模型返回最终文本或达到停止条件。

## 2. AgentConfig

`AgentConfig` 当前包含：

- `provider`
- `model`
- `tools`
- `approval_policy`
- `approvals_reviewer`
- `approval_bridge`
- `working_dir`
- `permission_profile`
- `cancel`
- `max_steps`

默认配置使用内置工具集、`ApprovalPolicy::Untrusted`、`ApprovalsReviewer::User`，并根据 `working_dir` 构造 `PermissionProfile::workspace_write`。

## 3. 一轮 Agent Loop

`Agent::run` 的简化流程：

```text
初始化 system + history

for step in 1..=max_steps:
  发出 Step(step)
  构造 GenerateRequest(stream=true, tools=tool_defs)
  调用 provider.stream_generate
  转发 LLM stream events

  将 assistant text / reasoning / tool_calls 加入 messages

  如果没有 tool_calls:
    发出 Finished
    返回 RunResult

  for tool_call in tool_calls:
    检测 doom loop
    根据 ApprovalPolicy 决定是否请求审批
    执行工具
    标记 tool_call finished
    发出 ToolResult
    将 tool result 加入 messages
```

## 4. Runtime 事件

`AgentEvent` 是 runtime 向外暴露的事件层：

| 事件 | 含义 |
| --- | --- |
| `Step` | agent 第几步 |
| `LlmStepStart` / `LlmStepFinish` | 单次 provider stream 生命周期 |
| `LlmFinish` | provider 结束原因与 token usage |
| `TextStart` / `TextDelta` / `TextEnd` | 文本输出 |
| `ReasoningStart` / `ReasoningDelta` / `ReasoningEnd` | reasoning 输出 |
| `ToolCallStart` / `ToolCallDelta` / `ToolCallEnd` | 模型请求工具 |
| `ApprovalRequest` | 需要 human-in-the-loop 审批 |
| `ToolResult` | 工具执行完成 |
| `Finished` | agent 最终文本 |
| `DoomLoopDetected` | 连续重复同名同参工具调用，被停止 |

Provider 发出的 `GenerateStreamEvent` 会在 `router.rs` 里转换成 `AgentEvent`。如果 provider 只发 delta，没有显式 start/end，runtime 会通过 `StreamLifecycle` 补齐 text/reasoning 的 start/end。

## 5. 工具上下文

工具实现统一接收 `ToolContext`：

```rust
pub struct ToolContext {
    pub working_dir: PathBuf,
    pub permissions: PermissionProfile,
    pub cancel: CancellationToken,
}
```

工具内部应遵守：

- 使用 `working_dir` 解析相对路径。
- 文件访问前调用 `ctx.check_path(path, AccessKind)`。
- 长耗时任务应响应 `ctx.cancel`。
- 不在工具内部做审批，审批由 runtime 统一处理。

## 6. 内置工具

当前内置工具包括：

| 工具 | 作用 | 权限检查 |
| --- | --- | --- |
| `read` | 读取文件 | 目标路径 `Read` |
| `write` | 写入文件 | 目标路径 `Write` |
| `edit` | 替换文件内容 | 目标路径 `Write` |
| `list` | 列目录 | 目标目录 `Read` |
| `grep` | 搜索文本 | 搜索根目录 `Read` |
| `glob` | 文件模式匹配 | 搜索根目录 `Read` |
| `bash` | 在工作目录执行 `sh -c` | 当前仅检查工作目录可读，并依赖审批 |

`bash` 已有超时、取消、`stdin` 置空、环境变量清理和 `kill_on_drop`，但还不是 sandbox。

## 7. Doom-loop 检测

runtime 会记录最近的工具调用 `(name, normalized_input)`。如果连续达到阈值且完全相同，会发出 `DoomLoopDetected` 并停止。这是防止模型反复执行同一工具调用的基础保护。

## 8. 当前缺口

- 缺少独立 `tool_runs` 表记录工具开始、结束、耗时、审批结果。
- `bash` 无文件级隔离，不能把审批等同于 sandbox。
- `ApprovalPolicy::OnFailure` / `OnRequest` / `Granular` 需要真正 sandbox 或 executor 支撑。
- 工具状态机还可以进一步明确为 `requested -> approved -> running -> completed/failed/cancelled`。
