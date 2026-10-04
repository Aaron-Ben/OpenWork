# Agent 运行

Computer 在本机运行 Agent：为每个 Agent 准备目录与凭证，收到唤醒后在 Seatbelt 中启动一次 OpenCode，Agent 用 `crew` 命令回复。代码在 `packages/computer/`。进程关系见 [architecture.md](../architecture.md)，Server 一侧的接口见 [messaging.md](messaging.md)。

## 1. 启动与停止

1. 从 stdin 读 bootstrap，调用 `POST /computer/connect` 确认地址与凭证。失败时把原因写到 stderr 并以 1 退出。
2. 向 stdout 写 ready。之后的准备不推迟窗口出现。
3. 检查 Seatbelt 是否可用、能否找到 `opencode`。任一不满足时不创建任何 Runner，并给每个 Agent 上报 `error` 状态与原因，例如“沙箱不可用：…”。
4. 准备本次运行目录：删除以前运行留下的 `~/.crew/runtime/*`，写入 `bin/crew` 包装脚本。
5. 用 `opencode models` 读取可用模型并上报（最多等 30 秒，只保留 `提供方/模型` 形式的行）。
6. 订阅 `/computer/events`。每次连接成功后同步 Agent 列表，再唤醒全部 Runner，补上断线期间到达的消息。

- 同步 Agent 列表时，为新 Agent 创建 Runner；名字、人设、Engine 或模型变了的 Agent，先停掉旧 Runner 再建新的。创建 Runner 时签发 Agent 凭证并写入凭证文件，写入 `AGENTS.md`。
- stdin 关闭或收到 SIGTERM、SIGINT 时停止：中止 SSE，停止全部 Runner（中止正在运行的 Turn），删除本次运行目录，然后退出。

## 2. Runner 与 Turn

每个 Agent 一个 Runner，内部是一个串行循环（`packages/computer/src/runner.ts`）：

- Turn 运行期间到达的唤醒合并成下一轮，不并发运行。
- 一轮 Turn：读取未读消息。没有未读时什么也不做。有未读时上报 `working`，生成本轮输入，运行一次 Engine。
  - **成功：** 保存 session，确认读到本轮最后一条消息，上报 `idle`。
  - **失败：** 上报 `error` 与原因，不确认已读；下一次唤醒时这些消息会和新消息一起重新处理。
  - **停止：** 中止 Engine，不确认已读，也不上报状态。
- 本轮输入按房间列出未读消息，并写明本地时间；文本由 `packages/computer/test/__snapshots__/turn-prompt.md` 逐字锁定。

## 3. OpenCode

每轮 Turn 在 Seatbelt 中启动一个进程：

```text
opencode run --pure --format json --print-logs --auto [--session <id>] --model <模型>
```

- 本轮输入写入 stdin，工作目录是 Agent 的 `work/`。进程在独立的进程组中运行。
- 环境变量只有：`PATH`、`LANG`、`TMPDIR`；`HOME` 与各个 `XDG_*` 目录指向 Agent 自己的目录；`OPENCODE_DISABLE_PROJECT_CONFIG=1`；`OPENCODE_AUTH_CONTENT`；`OPENCODE_CONFIG_CONTENT`；以及 `crew` 需要的 `CREW_SERVER_URL`、`CREW_TOKEN_FILE`，并把 `bin/crew` 所在目录放在 `PATH` 最前面。没有数据库相关的变量。
- `OPENCODE_AUTH_CONTENT` 来自用户的 `$XDG_DATA_HOME/opencode/auth.json`，在沙箱外读取。文件不存在、超过 64 KiB 或不是合法 JSON 时不启动。
- `OPENCODE_CONFIG_CONTENT` 是派生的配置：常驻规则指向 Agent 的 `AGENTS.md`；放行全部操作，安全边界是 Seatbelt；本次的模型标为 active。
- session：从输出事件中取 `sessionID`，保存在 `engines/opencode/session.json`，并记下 Engine、模型与 `AGENTS.md` 的摘要。三者都没变时下一轮继续这个 session，否则开新 session。旧 session 不存在时，开新 session 重试一次。
- 失败分为：未登录、模型不可用、限流、session 失效、沙箱、OpenCode 报告的错误、进程异常、输出超限、已停止。错误信息中隐去 Agent 目录与凭证。
- stdout 超过 8 MiB 时结束进程；停止时向整个进程组发 SIGINT，2 秒后仍未退出就发 SIGKILL。

## 4. Seatbelt

规则由 `packages/computer/src/sandbox/profile.ts` 生成，经 `/usr/bin/sandbox-exec -p` 启动 Engine：

- 路径只作为 `-D` 参数传入，从不拼进规则文本。
- 写入默认拒绝，只放行 Agent 的持久目录、本次运行目录、系统临时目录与几个设备（例如 `/dev/null`）。
- `$HOME` 之内默认不能读取文件内容，只放行 Agent 的持久目录、本次运行目录、`bin/crew` 所在目录与位于 `$HOME` 之内的可执行文件。`$HOME` 之外全部可读。只拒绝读取内容，不拒绝查看文件是否存在。
- 网络不受限制。
- 启动自检：用一条拒绝写入的规则在沙箱中运行 `sh`，确认写入确实被拒绝。

## 5. 本机目录

```text
~/.crew/
├── agents/<agent-id>/                 持久：跨运行保留
│   ├── AGENTS.md                      身份与规则
│   ├── work/                          OpenCode 的工作目录
│   └── engines/opencode/
│       ├── data/                      OpenCode 的数据目录，每个 Agent 独立
│       └── session.json               上次的 session
└── runtime/<运行 ID>/                 本次运行：启动时清理旧的，正常退出时删除
    ├── bin/crew                       crew 的包装脚本
    └── agents/<agent-id>/
        ├── token                      Agent 凭证
        └── config/ cache/ state/      OpenCode 的配置、缓存与状态目录
```

- 目录权限 0700，文件 0600。写文件先写临时文件再改名。
- `AGENTS.md` 只随 Agent 的设置变化，不含路径与时间；文本由 `packages/computer/test/__snapshots__/AGENTS.md` 逐字锁定。
- `bin/crew` 以 `ELECTRON_RUN_AS_NODE=1` 运行 Electron 可执行文件与打包后的 `shim.js`。两者都在 `$HOME` 之外，沙箱里可以读取。

## 6. crew 命令

- `crew reply <room-id>`：正文从 stdin 读取，去掉末尾的空白；带上 `CREW_TOKEN_FILE` 中的凭证调用 `CREW_SERVER_URL` 的 `POST /agent/reply`。正文不经过命令行，所以反引号与 `$` 不会被 shell 改写。
- `crew --help`：用法与 heredoc 示例。
- 本地先检查：房间 ID 是 UUID，正文不为空且不超过 20,000 字符。
- 成功时向 stdout 写 `Message sent to room <room-id>.`，退出码 0。失败时向 stderr 写一行英文 `error: …`，说明原因与下一步，退出码 1；Server 的 401、403、404 由 `crew` 翻译成英文。
- 请求最多等 10 秒，不重试；超时时提示消息可能已经发出、不要重发。
- 全部输出由 `packages/computer/test/__snapshots__/shim-output.md` 逐字锁定。

## 7. 验收

| 条目 | 测试 |
|---|---|
| Seatbelt 规则：路径不进规则文本、`$HOME` 内的读取限制、写入范围、自检 | `packages/computer/test/sandbox.test.ts` |
| 沙箱不可用时不创建 Runner，并上报原因 | `packages/computer/test/daemon.test.ts` 的 `starts no runner and reports the reason when the sandbox is unavailable` |
| 启动前到达的消息、SSE 唤醒、新建的 Agent 都能被处理；退出时删除本次运行目录 | `daemon.test.ts` |
| Turn 成功后确认已读；失败时保留消息并在下次重试；运行中的唤醒合并成一轮；停止时不确认 | `packages/computer/test/runner.test.ts` |
| session 在 Engine、模型与 `AGENTS.md` 不变时延续，否则重开 | `packages/computer/test/home.test.ts` 的 `session continuity` |
| OpenCode 的参数、环境隔离、session 重试、失败分类、进程组停止、输出超限 | `packages/computer/test/opencode.test.ts` |
| `crew` 原样提交正文，拒绝写在命令行上的正文，各类失败退出码为 1；输出逐字锁定 | `packages/computer/test/shim.test.ts` |
| `AGENTS.md` 与每轮输入的文本逐字锁定 | `home.test.ts`、`packages/computer/test/prompt.test.ts` |
| 构建产物的完整链路：用户发消息，Seatbelt 中的 Engine 经构建好的 `crew` 回复并落库 | `apps/desktop/test/smoke.e2e.ts` |
| 真实模型的完整链路 | 手动：`CREW_E2E_MODEL=<模型> pnpm test:e2e` |
