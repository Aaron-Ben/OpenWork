# 模型可见内容规范

OpenWork 是编码 agent：系统提示词、工具描述、工具结果、错误文本都是模型的输入，直接决定行为与成本。这部分改动和改代码一样需要审查。设计以 [tools.md](../../docs/tools.md)、[compaction.md](../../docs/compaction.md)、[context-window.md](../../docs/context-window.md) 为准。

## 1. 语言与措辞

- 一律英文，简洁、具体、可操作。
- 指导下一步，而不是只报告状态（参见 [error-handling.md](error-handling.md) §4）。
- 路径相对工作区显示；工作区外的路径显示绝对路径。
- 不出现内部类型名、Rust 调试格式、栈信息。

## 2. 保持前缀稳定

模型厂商的提示缓存按前缀匹配，前缀变一个字节，后面全部重新计费。

- **系统提示词与工具描述是静态文本**：不含时间、随机值、会话 ID、工作区路径、权限模式。
- 会变化的运行时信息（权限策略、沙箱是否可用、项目说明、skills 列表）放进 world state 的 section（`context/world_state/`），不拼进系统提示词。
- 工具列表按固定顺序注册，不按运行时条件重排。

```rust
// ❌ 错误：每个会话的前缀都不同，缓存全部失效
format!("You are OpenWork. Today is {date}. Workspace: {root}. Mode: {mode}.")

// ✅ 正确：静态提示词 + world state 中的 runtime/sandbox-policy section
```

## 3. 有上限

- 每个工具结果都有上限，由 `registry.call` 统一经 `bound_result` 约束；工具自身的上限（行数、条数、单行长度）是具名常量，数字以 tools.md 为准。
- 截断只截在末尾（bash 例外：保留开头与结尾），并告诉模型**怎么拿到其余部分**：

```text
✅ [showing lines 1-717 of 1532; stopped at 32 KB. Continue with offset=718]
✅ [showing 250 of 992 matching lines in 64 files; full list saved to <path>]
❌ ...(truncated)
```

- 给出准确总数；做不到准确时写"at least N"并说明原因（例如超时）。
- 大结果落盘后，结果中给出文件路径；落盘失败就不给路径。

## 4. 工具描述

- 第一句说明用途；接着写模型最容易用错的地方：上限、排序、路径是相对什么的、何时用别的工具。
- 参数的 `///` 文档注释会进入 JSON schema，写法同样面向模型：说明取值与默认值。

```rust
/// Glob pattern relative to `path`, for example `**/*.rs` or `src/**/*.ts`.
pub pattern: String,
```

## 5. 修改流程

- 改模型可见文本时，同步改逐字断言它的测试（[testing.md](testing.md) §3）。
- 改动可能影响 token 占用时（上限、描述长度、结果格式），用参照任务回放对比前后数字，写进进度文件。

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- 系统提示词或工具描述中出现动态内容（时间、路径、ID、模式）
- 运行时状态拼进系统提示词，而不是 world state
- 工具结果没有上限；截断没有说明怎么继续；总数不准确却没有标明
- 模型可见文本出现内部类型名、调试格式，或只有错误码
- 修改模型可见文本却没有更新逐字断言的测试
