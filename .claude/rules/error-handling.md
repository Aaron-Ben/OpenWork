# 错误处理规范

## 1. 错误类型

- 每个 crate 用 `thiserror` 定义自己的错误枚举，按边界划分（`SessionError`、`StorageError`、`CompactionError`、`ProviderRepositoryError`）。
- **不用 `anyhow`**：调用方需要按变体做不同处理（映射成 `CommandErrorCode`、决定是否重试），字符串错误做不到。
- 变体携带结构化字段，不把上下文拼进字符串：

```rust
// ✅ 正确：调用方能按变体和字段处理
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session already has an active turn: {0}")]
    Busy(TurnId),
}

// ❌ 错误：调用方只能解析字符串
return Err(SessionError::Other(format!("busy: {turn_id}")));
```

- 包装下层错误用 `#[from]` 或 `#[source]`，保留错误链；不要 `.map_err(|e| e.to_string())` 丢掉来源。

## 2. panic

- 非测试代码禁止 `unwrap()`。
- `expect()` 只用于**不变量**：写明为什么不可能失败，而不是描述操作本身。

```rust
// ✅ 正确：说明为什么不会失败
let offset = UtcOffset::from_hms(8, 0, 0).expect("+08:00 is a valid offset");

// ❌ 错误：只是换了个写法的 unwrap
let file = File::open(path).expect("open file");
```

- 外部输入（用户输入、模型参数、文件内容、数据库行、网络响应）引起的失败永远走 `Result`，不能 `expect`。
- 测试代码可以 `unwrap()` / `expect()`，`expect` 的信息写清楚在等什么（`expect("spill file")`）。

## 3. 不吞错误

- 禁止 `let _ = fallible();` 和 `.ok();` 静默丢弃，除非同一行注释写明为什么可以丢：

```rust
// ✅ 正确：接收方已经结束时没有人需要这个结果
let _ = respond_to.send(PermissionDecision::Deny); // receiver gone: turn already ended

// ❌ 错误
let _ = std::fs::remove_file(&path);
```

- 尽力而为的操作（清理落盘目录、写 Trace）失败时用 `tracing::warn!` 记录，然后继续。

## 4. 面向模型的错误

工具返回给模型的错误（`ToolError` / `ToolErrorCode`）是模型的输入，要能指导下一步：

```text
✅ Read lib.rs before editing it.
✅ lib.rs changed since you last read it (by you via bash, or by the user). Read it again before editing.
❌ edit failed
❌ PermissionDenied
```

- 说清楚发生了什么、模型接下来该做什么。
- 路径相对工作区显示。
- 不暴露内部类型名、栈、调试格式（`{:?}`）。

## 5. 面向 Desktop 的错误

- Core 错误在 `desktop/src-tauri/src/error.rs` 用 `From` 映射成 `CommandError { code, message }`。
- `CommandErrorCode` 是稳定的机器码（snake_case）。增删变体时，同时改 `desktop/src/lib/commandError.ts` 的联合类型与集合，以及 `tests/command_error_contract.rs`。
- `message` 给用户看：不含密钥、数据库连接串、完整文件内容；能指路时写出去哪里修（例如 "open Settings > Models and edit its provider"）。

## 6. 日志

- 库代码只用 `tracing`：`warn!` 用于可恢复的问题，`error!` 用于不可恢复的问题，`debug!` 用于排查细节。
- 禁止在库代码里 `println!` / `eprintln!`；二进制入口（`src/bin/`）打印用法或致命错误可以用 `eprintln!`。
- 日志不含 API key、用户文件内容、完整的模型请求或响应。

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- 新代码引入 `anyhow`，或用 `String` 作为错误类型
- `.map_err(|e| e.to_string())` 丢失错误来源
- 非测试代码中的 `unwrap()`；`expect()` 的信息没有说明不变量
- 外部输入引起的失败用 `expect` 处理
- 没有说明理由的 `let _ =` 或 `.ok();`
- 给模型的错误文本只有错误码，或没有下一步指引
- `CommandErrorCode` 改了而 TypeScript 侧与契约测试没同步
- 库代码里的 `println!` / `eprintln!`；日志中出现密钥或文件内容
