# Agent Note: 每轮一次 OpenCode 与 crew 命令

Status: implemented

## 问题

Agent 被唤醒后要调用模型，并且能在房间里发言。需要决定怎样运行 Engine、怎样延续对话、怎样把 Engine 关在沙箱里，以及 Agent 用什么方式发言。第 2 步只接 OpenCode。

## 决策

**Engine**

- Engine 接口在 `packages/computer/src/engine/types.ts`，结构参考 Rust 版的两层结构（adapter 与每个 Agent 的 runtime）、cumora 的 `cumora:server/src/agents/computer/engine.ts` 与 raft 的 `raft:packages/daemon/src/drivers/`。`runTurn` 不抛出，失败以结果返回。
- 每个 Turn 启动一次 `opencode run`，prompt 写入 stdin，工作目录是 Agent 的 `work/`，参数见 agent-runtime.md 第 3 节。`--auto` 自动批准文件与命令操作，安全边界是 Seatbelt。raft（`raft:packages/daemon/src/drivers/opencode.ts`）与 cumora（`cumora:server/src/agents/computer/engine.ts`）都按 Turn 启动 OpenCode。
- 一个 Turn 处理一次唤醒时的全部未读消息，Agent 可以在其中多次调用 `crew reply`。Agent 运行时到达的消息不立即处理，Turn 结束后合并成一轮再处理。不做 cumora 的 2.5 秒等待。
- session ID 存在 Agent 目录的 `engines/opencode/session.json`，下次以 `--session` 继续同一段对话。Engine、模型或 `AGENTS.md` 变化时开新 session。
- Agent 的 `AGENTS.md` 只写身份、人设、用 `crew reply` 发言（文字输出没人看到）与可以保持沉默，由 `OPENCODE_CONFIG_CONTENT` 的 `instructions` 指向它。每轮 prompt 只有唤醒说明、当前时间与按房间分组的未读消息。
- 每次启动 OpenCode 前，Computer 在沙箱外读取用户的 OpenCode 登录文件，经 `OPENCODE_AUTH_CONTENT` 传入。raft 与 cumora 让 OpenCode 自己读这个文件，因为它们不把 OpenCode 关进沙箱。用户重新登录后，下一轮自动使用新凭证。两个环境变量沿用 Rust 版的做法。
- 从输出中只取三样：成功与否、session ID、失败时的错误信息。
- 正式 Turn 没有总时长上限。取消用 `AbortSignal`：停止时向整个进程组先发 SIGINT，超时后发 SIGKILL。OpenCode 退出后同样结束它的进程组，Agent 在这一轮起的后台进程不会活过这一轮。raft 只结束单个进程（`raft:packages/daemon/src/drivers/runtimeSession.ts`），OpenCode 启动的子命令可能残留。
- 每个 Agent 同一时间只跑一个 Turn。

**沙箱**

- Seatbelt 规则由 TypeScript 生成（`packages/computer/src/sandbox/profile.ts`），经 `/usr/bin/sandbox-exec -p` 启动 Engine，没有无沙箱的运行路径。
- 规则与 Rust 版相同：写入只放行 Agent 自己的目录，`$HOME` 之内的其他文件不能读取内容，网络不限制。完整规则见 agent-runtime.md 第 4 节。
- Computer 启动时自检，沙箱不可用时不启动任何 Agent。

**crew 命令**

- 命令名是 `crew`，Computer 的本机数据目录是 `~/.crew`。
- 只有 `crew reply <room-id>` 与 `crew --help`。正文只从 stdin 读取，Agent 用带引号的 heredoc（`<<'EOF'`）传入，shell 不改动其中的字符。参数解析用 commander。
- 失败时向 stderr 写英文的 `error: …`，说明原因与下一步。Server 的错误文本是中文且界面也在用，由 `crew` 按状态码翻译。请求超时后不重试：还没有幂等，重试可能发出重复的消息。
- 每个 Agent 有一个随机凭证：Computer 用 Computer 凭证向 Server 申请，写入只有该 Agent 能读的运行期文件。Server 在内存中记录凭证对应的 Agent，应用重启后失效。
- `crew` 的代码在 `packages/computer/src/shim/`，构建为主进程旁的 `shim.js`。Computer 在本次运行目录生成 `bin/crew` 包装脚本，用 `process.execPath`（Electron 可执行文件）加 `ELECTRON_RUN_AS_NODE=1` 运行它，并把这个 `bin` 放到 Agent 的 `PATH` 最前面。Electron 可执行文件与 `shim.js` 都在 `$HOME` 之外，沙箱可以读取。

当前的行为与验收见 [agent-runtime.md](../../../../docs/subsystems/agent-runtime.md)。

## 考虑过的方案

**`crew` 经 Computer 的本地代理调用 Server。** cumora 用文件 IPC 交给 daemon，raft 用本地凭证代理（`raft:packages/daemon/src/agentCredentialProxy.ts`），模型看不到凭证。没有采用：Computer 要多一个本地服务与转发层。以后需要防止 Agent 直接使用自己的凭证时再考虑。

**`crew` 也接受写在命令行上的正文。** cumora 的 `reply <convo_id> "<body>"` 这样做（`cumora:server/src/agents/cli.ts`）。没有采用：命令行上的正文先经过 shell，反引号与 `$` 会被展开，消息被悄悄改写；用单引号时 `\n` 又不会变成换行，cumora 为此写了 `unescapeChat`，再用 `--stdin` 与 `--file` 绕开它对代码片段的破坏（`cumora:server/src/agents/cli-parse.ts`）。raft 只接受 stdin（`raft:packages/cli/src/commands/message/send.ts`）。

**停止与完成同时发生时按完成处理。** 能避免停止后重新处理已经回复过的消息、重复回复。没有采用：OpenCode 被 SIGINT 打断时的退出码没有确认，如果也是 0，被打断的一轮会被当成完成并确认已读，消息就丢了，这比重复回复更糟。raft 与 cumora 也都让停止优先（`raft:packages/daemon/src/drivers/runtimeSession.ts`、`cumora:server/src/agents/computer/engine.ts`）。以后加入修改 Agent 的接口时，与幂等一起重新考虑。

**用模型最后的文字输出作为回复。** 可以省掉 `crew` 命令。没有采用：`crew` 已经验证可行，它让 Agent 可以选择沉默，也为群聊的协调留出余地。

## 后果

- Agent 可以选择沉默；群聊协调可以在 `crew` 上扩展新命令。
- 一轮结束后不留下后台进程，Agent 因此不能在两轮之间保留后台进程。
- 停止优先：被打断的一轮不确认已读，下一轮重新处理，可能重复回复，但不会丢消息。
- 依赖 Electron 的 `runAsNode` fuse。2026-10-04 的验证中，`crew` 在与 Rust 版同构的 Seatbelt 规则下经 `ELECTRON_RUN_AS_NODE` 启动，能调用本机 HTTP 服务，单次约 180 ms（系统 Node 约 114 ms）。沙箱只拒绝了 `~/.CFUserTextEncoding` 的读取与 `/dev/dtracehelper` 的写入，都不影响运行。
- Agent 能读到自己的 Agent 凭证，可以绕过 OpenCode 直接以自己的身份调用 Server。沙箱保证它读不到其他 Agent 的凭证。
- 沙箱不可用时 Computer 不启动任何 Agent（`packages/computer/test/daemon.test.ts`）；构建后的 `crew` 在 Seatbelt 中能调用 Server，由冒烟测试 `apps/desktop/test/smoke.e2e.ts` 覆盖。
