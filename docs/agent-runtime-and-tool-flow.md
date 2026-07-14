# Agent Runtime 与工具调用流程

Last reviewed: 2026-07-15

> Status: current implementation detail. 目标状态机、恢复和评测边界见 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md)。

## 1. 相关代码

```text
crates/openwork-core/src/{agent,approval}.rs
crates/openwork-app/src/{application,chat,turn_service,turn_supervisor}.rs
crates/openwork-protocol/src/approval/mod.rs
crates/openwork-protocol/src/turn/mod.rs
crates/openwork-protocol/src/capability/
crates/openwork-persistence/src/session/lifecycle.rs
crates/openwork-capabilities/src/
crates/openwork-execution/src/actions/filesystem/
crates/openwork-execution/src/actions/process/
crates/openwork-execution/src/{context,handler,invoker,schema,service}.rs
apps/desktop/src-tauri/src/lib.rs
```

`openwork-core` 负责 Turn/Agent loop 和审批状态。Desktop 只调用 `openwork-app::OpenWorkApplication::turns()`；内部 `TurnApplicationService` 持有 `ChatRuntime` 和取消注册表，`ChatRuntime` 组合 provider/session/Core 并把 Core 事件转换为宿主/UI payload。

## 2. AgentConfig

`AgentConfig` 当前包含：

- `provider`
- `model`
- `capabilities: Arc<dyn CapabilityResolverPort>`
- `execution: Arc<dyn ExecutionPort>`
- `recorder: Arc<dyn TurnRecorderPort>`
- `turn_id`
- `approval_policy`
- `approval_commands: TurnCommandInbox`
- `cancel`
- `max_steps`

`AgentConfig` 不构造或持有具体 Tool Registry。`OpenWorkApplication` 是唯一 Composition Root；其内部 `ChatRuntime` 为每个 Turn 创建内置 `CapabilityCatalog`、`BuiltinActionInvoker` 和 `ExecutionService`，并用相同的 cancellation token 组合 Core 与 Execution。默认审批是 `ApprovalPolicy::Untrusted`。

## 3. 一轮 Agent Loop

`Agent::run` 的简化流程：

```text
初始化 system + history
通过 CapabilityResolverPort 获取 Tool Schema

for step in 1..=max_steps:
  await persist step_started
  发出 Step(step)
  构造 ModelRequest(tools=tool_defs)
  调用 ModelPort::invoke，消费返回的异步 ModelStream
  转发 LLM stream events

  await persist assistant_message_recorded
  将 assistant text / reasoning / tool_calls 加入 messages

  如果没有 tool_calls:
    发出 Finished
    返回 RunResult

  for tool_call in tool_calls:
    检测 doom loop
    创建 ToolRunId，await persist tool_run_requested
    调用 ExecutionPort::authorize
    RequireApproval 时 await persist approval_requested，并等待 ResolveApproval 命令
    执行前 await persist tool_run_started
    通过 ExecutionPort 执行 Action
    await persist ToolRun 终态 + tool_message_recorded
    标记 tool_call finished
    发出 ToolResult
    将 tool result 加入 messages

  await persist step_completed / step_failed
```

`ModelRequest` 不再包含 `stream` 开关。流式生命周期由 `ModelPort::invoke` 返回的 `ModelStream` 表达，Core 直接异步消费该 Stream，不再经过同步 callback 或第二层 channel 桥接。

App 在进入 Agent Loop 前通过 `SessionStore::start_turn` 原子写入 `turn_started` 和 `user_message_recorded`，并向 Core 注入绑定该 Turn 的 `JournalTurnRecorder`。Assistant/Tool Message 与 Step/ToolRun/Approval 生命周期由 Core 在语义点同步追加；Turn 结束后，App 只写 Turn 终态。文本、推理和工具参数 delta 只是 Live Event，不逐帧写入数据库。

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
| `ApprovalRequested` | Core 已进入等待审批状态 |
| `ApprovalResolved` | Core 已应用用户决定 |
| `ToolResult` | 工具执行完成 |
| `Finished` | agent 最终文本 |
| `DoomLoopDetected` | 连续重复同名同参工具调用，被停止 |

Provider 发出的 `ModelEvent` 只包含文本、推理和工具调用语义输出，并在 `openwork-core/src/agent.rs` 中转换成 `AgentEvent`。如果 provider 只发 delta，没有显式 start/end，Core 会通过 `StreamLifecycle` 补齐 text/reasoning 的 start/end；Turn Step、Retry 和 UI done/error 不属于 `ModelEvent`。

`openwork-providers::RetryingModelPort` 只允许在尚未发出任何 `ModelEvent` 时重试。已经出现文本、推理或工具调用增量后，网络中断会直接返回错误，避免重复文本或重复工具参数。

## 5. Capability 与 Execution 边界

声明由 `CapabilitySpec` 表示：

```rust
pub struct CapabilitySpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub risk_hint: CapabilityRiskHint,
}
```

`risk_hint` 表达声明侧的粗粒度分类，并由 Execution 用于生成审批原因。它不是最终信任结论；后续仍需结合实际参数、路径和运行位置增强风险计算。

真实 Handler 只在 `openwork-execution` 内部，通过 `ExecutionContext` 获取执行环境：

```rust
pub struct ExecutionContext {
    pub working_dir: PathBuf,
    pub permissions: PermissionProfile,
    pub cancel: CancellationToken,
}
```

Execution 当前遵守：

- 调用 Handler 前根据 Capability Schema 校验参数。
- 使用 `working_dir` 解析相对路径。
- 文件访问前调用 `ctx.check_path(path, AccessKind)`。
- 长耗时任务应响应 `ctx.cancel`。
- 把成功、失败、拒绝、取消和未知结果归一化为 `Observation`。
- 不在 Handler 或 Execution 内等待 UI；Execution 只返回 `Allow / Deny / RequireApproval`，Core 持有等待状态。

## 6. 内置 Action

Capability 声明位于 `openwork-capabilities/src/builtin.rs`；真实 Handler 按执行领域放置：文件和搜索 Action 位于 `actions/filesystem/`，进程 Action 位于 `actions/process/`。

当前内置 Action 包括：

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

参数 Schema 与 Handler 已分离，并有合同测试保证七个内置名称完全一致。

## 7. Doom-loop 检测

runtime 会记录最近的工具调用 `(name, normalized_input)`。如果连续达到阈值且完全相同，会发出 `DoomLoopDetected` 并停止。这是防止模型反复执行同一工具调用的基础保护。

## 8. Durable 恢复边界与当前缺口

- `ApprovalRequested`、`ApprovalResolved` 和 ToolRun 生命周期已经在语义点写入可重放 Event Journal。
- 应用重启后，可从 `pending_approval` 恢复尚未开始执行的工具调用；普通 interrupted Turn 暂不自动续跑。
- 已有 `tool_run_started` 但没有终态的调用只会投影为 `outcome_unknown`，不会自动重跑；外部副作用对账尚未实现。
- `bash` 无文件级隔离，不能把审批等同于 sandbox。
- `risk_hint` 已参与审批原因生成，但仍不能替代针对实际参数和路径的最终风险判断。
- `schema.rs` 只实现当前内置 Action 所需子集；接入任意 MCP Schema 前需要重新确定兼容策略。
