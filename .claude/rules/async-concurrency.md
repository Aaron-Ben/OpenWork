# 异步与并发规范

运行时统一为 tokio（多线程）。

## 1. 不阻塞执行器

async 函数里不能直接做耗时不确定的阻塞操作：

| 操作 | 做法 |
|---|---|
| 目录遍历、全文搜索、读大文件、哈希大文件 | `tokio::task::spawn_blocking`，参考 `glob.rs` / `grep.rs` 的 `scan` |
| 工具里的文件读写 | 走 `ToolSessionContext.filesystem`（`AsyncFileSystem`），不直接用 `std::fs` |
| 其他单次小文件读写 | `tokio::fs` |
| 子进程 | `tokio::process`，不用 `std::process::Command` 的阻塞 `output()` |

`std::fs` 只用于同步函数、`spawn_blocking` 闭包内部和测试。

```rust
// ✅ 正确
let scan = tokio::task::spawn_blocking(move || scan(request)).await?;

// ❌ 错误：遍历整个仓库时占住一个执行器线程
async fn execute(...) {
    for entry in walkdir::WalkDir::new(root) { ... }
}
```

## 2. 可取消、有上限

- 长时间运行的操作必须响应取消：定期检查 `ToolCallContext.cancel`（`CancellationToken`），在阻塞循环里用 `ScanBudget` 这类预算对象同时检查取消与超时。
- 每个外部等待都有超时：模型请求、子进程、扫描都要有上限，超时时间是具名常量（例如 `SCAN_TIMEOUT`）。
- 取消与超时要分开报告：取消返回 `ToolErrorCode::Cancelled`；超时给出已得到的部分结果，并提示缩小范围。

## 3. 共享状态

- **Session 状态归 actor 所有**（`session/actor.rs`、`openwork-chat-state` 的 actor）：外部通过命令消息与它交互，不要把内部状态包成 `Arc<Mutex<_>>` 分给多个层直接改。
- 临界区内没有 `.await` 时用 `std::sync::Mutex`；只有必须跨 `.await` 持锁时才用 `tokio::sync::Mutex`。
- 禁止持有 `std::sync::Mutex` 的锁跨越 `.await`（clippy `await_holding_lock`）。
- 锁里只做内存操作：先把数据取出或克隆出来，释放锁，再做 I/O。

```rust
// ✅ 正确：锁内只取数据
let seen = self.seen.lock().expect("observations lock").get(&path).copied();
let current = hash_file(&path).await?;

// ❌ 错误：持锁做 I/O
let mut seen = self.seen.lock().expect("observations lock");
let current = std::fs::read(&path)?;
```

- `lock()` 的 poison 错误用 `expect` 并说明原因即可：其他线程持锁时 panic 本身就是 bug。

## 4. 任务与通道

- `tokio::spawn` 出去的任务要有所有者：保存 `JoinHandle` 或挂在 actor 上，Session 结束时能停止它。禁止"发射后不管"。
- 通道有界（`mpsc::channel(n)`）；使用 `unbounded_channel` 时写明为什么不会无限增长。
- 发送端在接收方已经结束时报错是正常情况，按 [error-handling.md](error-handling.md) §3 注明后忽略。

## 5. 顺序与原子性

- 涉及"内存状态 + 数据库"的两步更新，先写数据库，成功后再改内存；失败时内存保持原样。参考修剪水位线的顺序：`replace_items` → 数据库推进 → 设置修剪状态，任何一步失败都退回到摘要压缩。
- 事件的顺序要可验证：Tool Call、状态变化、Tool Result、UI Update 按设计文档规定的顺序发出，并有测试覆盖。

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- async 函数中直接遍历目录、读大文件、调用阻塞的 `std::process`
- 工具实现绕过 `AsyncFileSystem` 直接用 `std::fs`
- 长循环不检查 `CancellationToken`；外部等待没有超时
- 持有 `std::sync::Mutex` 锁跨 `.await`；锁内做 I/O
- 把 Session 内部状态包成 `Arc<Mutex<_>>` 分给其他层修改
- 没有所有者的 `tokio::spawn`；没有说明理由的无界通道
- 先改内存再写数据库，写库失败时内存状态不回退
