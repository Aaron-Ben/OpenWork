# Agent Note: OpenCode 在 Seatbelt 内运行，只限制文件，不限制网络

Status: implemented

## 问题

协作 Agent 的 Engine 是本机的 OpenCode 进程，模型能在其中运行任意命令。在此之前，每个 Agent 的 home 只是应用层的约定。一个 Agent 的进程能读其他 Agent 的 token、用户的 OpenCode 登录信息和 `$HOME` 下的任何文件。

Engine 又有两个必须满足的需要：连模型服务商；通过 `openwork` CLI 以本 Agent 的身份访问 Server。

## 决策

- `EngineConfinement`（`crates/openwork-sandbox/src/confinement.rs`）描述一个 Agent 的围栏。`SeatbeltProfile::confined`（`seatbelt.rs`）生成 profile，`Seatbelt::confine`（`backend.rs`）包装 argv。
- 写：只放行临时根、本 Agent 的 `agents/<id>/` 与 `derived/<id>/`。
- 读：`$HOME` 内只拒绝 `file-read-data`，不拒绝 `stat`。例外由 `HomeManager::confinement`（`crates/openwork-collab/src/computer/home.rs`）列出：两个可写根、本 Agent 的 `runtime-token`、runtime `bin/` 与 shim。`OpenCodeAdapter::confined_argv`（`computer/opencode/launch.rs`）再放行规范化后的 OpenCode 可执行文件。`$HOME` 外照常可读。
- 网络：profile 以 `(allow default)` 开头，不加网络规则。
- 每个 Agent 的 `XDG_DATA_HOME` 是 `agents/<id>/engines/opencode/data`。`OpenCodeAdapter::user_auth` 每次启动前在沙箱外读用户的 `opencode/auth.json`，经 `OPENCODE_AUTH_CONTENT` 传入。上限是 `MAX_AUTH_BYTES`（64 KiB）。
- JWT 仍以 token 文件交给 shim（`OPENWORK_RUNTIME_TOKEN_FILE`），模型能读到它。
- 沙箱自检失败时，`probe` 返回错误，inventory 为 error，不启动 Runner。

规则见 [collaboration.md §3.1](../../../../docs/subsystems/collaboration.md)。网络的立场见 [Agent Note：不做网络管控](2026-08-01-no-network-control-and-no-claim.md)。

## 考虑过的方案

**只做应用层隔离。** 这是 OpenCode 进入 Seatbelt 之前的状态。当时 collaboration.md §3 写明：home 与凭证是应用层逻辑隔离，不是 macOS 安全沙箱。被攻陷的 Engine 能读该用户能读的全部文件。之后决定用 Seatbelt 取代它。

**照搬 Cumora 的 MCP 中转。** Cumora daemon 独占 runtime token。模型侧的 `cumora` shim 把 argv 写进 home 之外的文件 IPC 目录，由 daemon 代发 HTTP。受限的 Claude 经固定的 MCP bridge 调用它（`server/src/agents/computer/daemon.ts` 中 `CUMORA_SHIM` 与 `CUMORA_MCP_SHIM` 前的注释）。按该注释，这样做是为了让工具子进程保持断网。OpenWork 决定不限网络，也不做中转。原始记录没有写更多理由。

**`$HOME` 内也拒绝 `stat`。** 工作台的 profile 用 `deny file-read*`。围栏不这样做：解析 Agent 目录的上级路径与 `realpath` 都要读上级目录的元数据（`SeatbeltProfile::confined` 的注释）。

**共用用户自己的 OpenCode 数据目录。** 进入 Seatbelt 之前，Engine 用用户的数据目录。之后改为每个 Agent 独立，因为那里有用户的登录信息与全部会话，沙箱必须拒绝读它。

## 后果

- 一个 Agent 的 Engine 读不到其他 Agent 的目录与 token，也读不到用户的凭证目录。验收见 `computer::home::tests::acc_10_an_agent_engine_cannot_reach_another_agent_or_the_user_home` 与 `crates/openwork-sandbox/tests/engine_confinement.rs`。
- 模型能看到本 Agent 的 JWT 与 Provider 登录信息。它们必须进入同一个进程树。
- 沙箱内的命令可以联网，能把它读到的内容发出去。
- OpenCode 必须装在 `$HOME` 外，或是单文件可执行文件。nvm 里的 npm 包不能在沙箱内启动。
- 换数据目录后旧 session id 失效。adapter 把 `Session not found`（包括只写在 stderr 的情况）当作 session 失效，开新的 session。
- 登录文件超过 64 KiB 时拒绝启动，不截断。环境变量与 argv 共用 macOS 1 MiB 的 `ARG_MAX`。
- 协作与工作台不共用 `SandboxPolicy`。工作台把整个 `~/.openwork` 列为硬保护，而 Agent home 在其中（`confinement.rs` 模块注释）。
