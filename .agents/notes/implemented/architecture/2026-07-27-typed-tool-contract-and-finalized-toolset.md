# Agent Note: 强类型工具契约与 FinalizedToolset

Status: implemented

## 问题

工具的依赖有不同的寿命。工作目录在一个 Session 内不变，取消令牌每次调用都不同。把它们放进同一个上下文，结果只有两种：跨调用的状态泄漏，或者每次调用都重建整个上下文。

模型看到的工具定义与 Core 实际能执行的工具，如果来自两个对象，就会出现“广告了但调不动”或“能调但没广告”。输入 schema 与 handler 读取参数的方式如果分开写，也会不一致。

## 决策

- 依赖按寿命分四层：工具契约、工具集、`ToolSessionContext`、`ToolCallContext`（[tools.md §1](../../../../docs/subsystems/tools.md)）。
- `Tool` trait 带关联类型 `Input` 与 `Output`。`serde` 与 `schemars` 用同一个 `Input` 类型（`crates/openwork-tools/src/tool.rs`）。`ToolAdapter<T>` 做一次类型擦除，具体工具保持强类型。
- `ToolRegistryBuilder::finalize`（`crates/openwork-tools/src/registry.rs`）用同一批选中项构造定义列表与调用表。结果 `FinalizedToolset` 不可变。
- Core 只持有一个 `Arc<FinalizedToolset>`。控制工具由 Core 的 `TurnToolset` 另外加入（`crates/openwork-core/src/session/toolset.rs`）。
- 沙箱模式不放进 `ToolSessionContext`。模式在 Session 内会变，越界授权只作用于一次调用，所以策略随调用传递。
- 内置工具经注入的 `AsyncFileSystem` 与 `ProcessBackend` 访问文件和进程。单元测试不需要真实工作目录；进程表、超时与取消集中在一处。

## 考虑过的方案

**Core 同时持有工具目录（Catalog）与执行器（Executor）。** 这是 FinalizedToolset 之前的结构。没有保留：两个对象各自维护工具列表，广告与执行可能不一致。

**handler 手工用 `Value::get("...")` 读取参数。** 这也是之前的写法。没有保留：schema 与实际读取的字段是两份事实，会漂移。

**可变的工具集加一个 `ToolBridge`，为动态 MCP 工具预留位置。** 没有采用：当前没有动态工具。以后需要时，在不可变的内置工具集之外增加独立的动态层，不让现有对象提前变可变。

**按需加载工具定义。** 没有采用：内置工具只有七个，按需加载节省不了多少上下文。

## 后果

- 广告与执行不可能分开。未选中的工具调用得到 `tool_not_found` 结果，Turn 继续。
- 增加动态工具时，必须新增一层，不能在 `FinalizedToolset` 上加方法。
- `ToolRisk` 只是提示。Core 的权限判定不读它，见 [permissions.md](../../../../docs/subsystems/permissions.md)。
- `AsyncFileSystem` 的方法接受 `&Path`。“只用 `CheckedPath` 访问文件”靠代码约定与审查，类型系统不强制。
