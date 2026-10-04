# Agent Note: 子进程意外退出时，成组替换 RuntimeSession

Status: legacy

## 问题

协作运行时有两个子进程：Server 与 Computer。任何一个都可能意外退出。Agent JWT、签名的 trigger 与 Agenda candidate set 都绑定 `runtime_session_id`，签名密钥只存在于 Server 进程内存中。Runner、Engine 进程与 runtime 目录也属于某一个 RuntimeSession。

## 决策

- `supervise`（`desktop/src-tauri/src/collab_client.rs`）每 250 ms 检查两个 `Child`。任何一个退出，就停止整组、删除 runtime 目录，再由 `start_group` 生成新的 RuntimeSession 与全部凭证。
- 替换期间，Desktop 命令返回 `Unavailable`。
- 新 Server 启动时，`Runs::interrupt_stale`（`crates/openwork-collab/src/server/runs.rs`）把其他 RuntimeSession 的 running Run 记为 `interrupted`。`SigningKey::verify_agent_token` 拒绝旧 session 的 JWT。

规则见 [collaboration.md §2](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**只替换退出的子进程，复用旧 RuntimeSession。** 设计明确禁止。旧 JWT 与旧 trigger 绑定旧 session，签名密钥随旧 Server 一起消失。复用旧 session，就要把密钥与凭证交给新进程。

## 后果

- 任何一个子进程退出，全部 Runner 与 Engine 都重启。正在运行的 Turn 被中断，delivery 不结算，下次重新读取。
- 每次替换都轮换全部凭证，旧进程树中遗留的 shim 调用都会失败。
- 替换启动失败时，supervisor 每 2 秒重试一次。
- 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`。
