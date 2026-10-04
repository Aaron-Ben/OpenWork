# openwork-sandbox

本 crate 定义一个进程能读写什么，并用 Seatbelt 在内核中强制执行。路径分档的理由见 [Agent Note：路径四档](../../.agents/notes/legacy/architecture/2026-08-01-path-tiers-and-credential-read-deny.md)。

本 crate 不依赖其他 OpenWork crate。它不启动被限制的进程（启动自检除外）。

## 使用方

| crate | 用途 |
|---|---|
| `openwork-collab` | 用 `EngineConfinement` 约束协作 Engine 进程 |

## 公开接口

| 项 | 作用 |
|---|---|
| `SandboxMode` | `AcceptEdits` 或 `Auto` |
| `SandboxEnvironment` | 主目录、临时目录、skill 根；`detect` 从当前进程读取；`bash_environment` 给出 bash 额外的环境变量 |
| `SandboxPolicy` | 模式、工作区根与本次调用的越界授权。`check` 判断一个路径对某个 `Actor` 是否可读写；`tier` 返回路径所属的档 |
| `PathGrant`、`Access`、`GrantScope` | 一条越界授权：路径、读或写、单个文件或子树 |
| `SandboxPolicy::validate_grants`、`GrantError`、`MAX_GRANTS` | 不经询问就拒绝的越界请求 |
| `PathTier`、`Denial`、`Actor` | 路径的档、拒绝原因、调用方（bash 或文件工具） |
| `SeatbeltProfile`、`SANDBOX_EXEC` | 由策略生成 Seatbelt profile 与 `sandbox-exec` 参数 |
| `SandboxBackend`、`Seatbelt`、`SandboxUnavailable` | 后端接口；`Seatbelt::probe` 在创建时做功能性自检 |
| `SandboxStatus`、`probe` | 沙箱可用或不可用（带原因） |
| `classify`、`RunOutcome` | 由退出码与输出区分正常结束、被沙箱拒绝与 `sandbox-exec` 自身失败 |
| `EngineConfinement` | 协作 Engine 进程的读写范围 |

## 测试

| 文件 | 内容 |
|---|---|
| `tests/parity.rs` | 文件工具围栏与 Seatbelt profile 对同一组路径给出相同结论 |
| `tests/matrix.rs` | 在真实 `$HOME` 与真实 Seatbelt 下运行日常命令 |
| `tests/probe.rs` | 自检 |
| `tests/engine_confinement.rs` | 协作 Engine 的读写范围 |

macOS 上的真机测试用 `#[cfg(target_os = "macos")]`，每次 `cargo test` 都在真实内核上运行。
