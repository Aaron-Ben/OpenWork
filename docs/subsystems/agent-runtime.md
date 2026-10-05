# Agent 运行

Computer 在本机运行 Agent：为每个 Agent 准备目录与凭证，收到唤醒后在 Seatbelt 中启动一次 OpenCode，Agent 用 `crew` 命令回复。代码在 `packages/computer/`。进程关系见 [architecture.md](../architecture.md)，Server 一侧的接口见 [messaging.md](messaging.md)。

## 1. 启动与停止

1. 从 stdin 读 bootstrap，调用 `POST /computer/connect` 确认地址与凭证。失败时把原因写到 stderr 并以 1 退出。
2. 向 stdout 写 ready。之后的准备不推迟窗口出现。
3. 检查 Seatbelt 是否可用、能否找到 `opencode`。任一不满足时不创建任何 Runner，并给每个 Agent 报告跑不起来的原因，例如“沙箱不可用：…”。
4. 准备本次运行目录：删除以前运行留下的 `~/.crew/runtime/*`，写入 `bin/crew` 包装脚本。
5. 用 `opencode models` 读取可用模型并上报（最多等 30 秒，只保留 `提供方/模型` 形式的行）。
6. 订阅 `/computer/events`。每次连接成功后同步 Agent 列表，再唤醒全部 Runner，补上断线期间到达的消息。

- 同步 Agent 列表时，为新 Agent 创建 Runner；名字、人设、Engine 或模型变了的 Agent，先停掉旧 Runner 再建新的。创建 Runner 时签发 Agent 凭证并写入凭证文件，写入 `AGENTS.md`。
- 某个 Agent 准备失败（例如它的目录里出现了符号链接）时，给它报告原因，其他 Agent 照常运行；之后准备成功时清除。
- 第 4 到 6 步本身失败时（例如建不了本次运行目录），Computer 把原因写到 stderr 并以 1 退出，主进程随之弹出错误对话框。
- stdin 关闭或收到 SIGTERM、SIGINT 时停止：同时中止全部 Runner 与 SSE（中止正在运行的 Turn），等它们结束，删除本次运行目录，然后退出。

## 2. Runner 与 Turn

每个 Agent 一个 Runner，内部是一个串行循环（`packages/computer/src/runner.ts`）：

- Turn 运行期间到达的唤醒合并成下一轮，不并发运行。
- 一轮 Turn：读取未读消息（同时记为已投递）。没有未读时什么也不做。有未读时生成本轮输入，向 Server 登记这一轮（每个房间的起止序号与完整输入），运行一次 Engine。Agent 的“回复中”与“出错”由 Server 从运行记录推出，见 [messaging.md](messaging.md) 第 7 节。
  - **运行中：** Engine 的每个事件按顺序上报（`packages/computer/src/runner.ts` 的 `RunReporter`）。前一批还在路上时到达的事件攒成下一批；上报失败只记日志、丢掉这一批，不影响这一轮。
  - **成功：** 保存 session，确认已读（已读位置推进到已投递位置，包括被 HELD 返回过的消息），记为 `succeeded`。
  - **失败：** 记为 `failed` 与原因，不确认已读；下一次唤醒时这些消息会和新消息一起重新处理。
  - **停止：** 中止 Engine，不确认已读，尽力记为 `cancelled`；记不上时，下一个 Computer 连上后它被标为中断。读取未读消息期间被停止时，不再开始这一轮。
  - **意外错误**（保存 session、确认已读或 Engine 违反约定抛出）：记为 `failed` 与“处理失败：…”。读取未读消息或登记失败时只记日志，下一次唤醒重新读取。
- 本轮输入按房间列出未读消息，并写明本地时间。开了新会话（第一次运行、换了模型或 `AGENTS.md`、旧会话失效）时多一句“这是新会话，先读 MEMORY.md”，记忆文件超过 16KB 时这句话里请它精简。群聊写出名字与成员名册；讨论串单独成段，写出所在群聊的名字与成员，以及挂着的那条消息（正文至多 600 字符）；作者写成 `User (user)` 或 `名字 (@handle)`，本 Agent 加 `you`；@ 到本 Agent 的消息标 `[mentions you]`，通知标 `[notice]`，任务的宿主消息在正文后面带 `[task #3 in_progress, assigned to @alice]`。文本由 `packages/computer/test/__snapshots__/turn-prompt.md` 逐字锁定。

## 3. OpenCode

每轮 Turn 在 Seatbelt 中启动一个进程：

```text
opencode run --pure --format json --print-logs --auto [--session <id>] --model <模型>
```

- 本轮输入写入 stdin，工作目录是 Agent 的 `work/`。进程在独立的进程组中运行。
- 环境变量只有：`PATH`、`LANG`、`TMPDIR`；`TMPPREFIX` 指向可写的临时目录（zsh 在这里写 heredoc 的临时文件，默认的 `/tmp/zsh` 沙箱不让写）；`HOME` 与各个 `XDG_*` 目录指向 Agent 自己的目录；`OPENCODE_DISABLE_PROJECT_CONFIG=1`；`OPENCODE_AUTH_CONTENT`；`OPENCODE_CONFIG_CONTENT`；以及 `crew` 需要的 `CREW_SERVER_URL`、`CREW_TOKEN_FILE`，并把 `bin/crew` 所在目录放在 `PATH` 最前面。没有数据库相关的变量。
- `OPENCODE_AUTH_CONTENT` 来自用户的 `$XDG_DATA_HOME/opencode/auth.json`，在沙箱外读取。文件不存在、超过 64 KiB 或不是合法 JSON 时不启动。
- `OPENCODE_CONFIG_CONTENT` 是派生的配置：常驻规则指向 Agent 的 `AGENTS.md`；放行全部操作，安全边界是 Seatbelt；本次的模型标为 active。
- 模型价格表：每轮启动前，把用户的 `$XDG_CACHE_HOME/opencode/models.json`（默认 `~/.cache`）复制进 Agent 的缓存目录；副本不比来源旧时跳过。OpenCode 按这张表算费用，缓存里没有它时报告的费用一律是 0，它也不会自己去下载。复制失败只记日志。复制时逐级确认目录不是符号链接，先写临时文件再改名。
- 事件：每行输出换成运行记录的 Engine 事件（`engineEventOf`）。`step_start` 是开始一步；`tool_use` 在工具完成后才有，带工具名、标题、输入、输出、耗时与是否失败；`text` 是模型的文字；`step_finish` 是一步结束，带 token 与费用。工具的输入输出与文字截到 4,096 字符，不把 emoji 这类代理对从中间切断。
- session：从输出事件中取 `sessionID`，保存在 `engines/opencode/session.json`，并记下 Engine、模型与 `AGENTS.md` 的摘要。三者都没变时下一轮继续这个 session，否则开新 session。旧 session 不存在时，开新 session 重试一次。
- 失败分为：未登录、模型不可用、限流、session 失效、沙箱、OpenCode 报告的错误、进程异常、输出超限、已停止。错误信息中隐去 Agent 目录与凭证。`runTurn` 不抛出，意外错误也转成“进程异常”。
- 准备期间（查找 `opencode`、读取登录文件）已经被停止时，不启动 OpenCode。
- stdout 超过 8 MiB 时结束进程；停止时向整个进程组发 SIGINT，2 秒后仍未退出就发 SIGKILL。
- OpenCode 退出后，向它的进程组发 SIGTERM，2 秒后发 SIGKILL：Agent 在这一轮里起的后台进程不会活过这一轮，也不会因为占着输出管道让这一轮卡住。Agent 因此不能在两轮之间保留后台进程。
- 停止与完成同时发生时，这一轮按“已停止”处理，不确认已读，下一次运行会重新处理这些消息。

理由见 [每轮一次 OpenCode 与 crew 命令](../../.agents/notes/implemented/architecture/2026-10-04-opencode-turns-and-crew-cli.md)。

## 4. Seatbelt

规则由 `packages/computer/src/sandbox/profile.ts` 生成，经 `/usr/bin/sandbox-exec -p` 启动 Engine：

- 路径只作为 `-D` 参数传入，从不拼进规则文本。
- 写入默认拒绝，只放行 Agent 的持久目录、本次运行目录、系统临时目录与几个设备（例如 `/dev/null`）。
- `$HOME` 之内默认不能读取文件内容，只放行 Agent 的持久目录、本次运行目录、`bin/crew` 所在目录与位于 `$HOME` 之内的可执行文件。`$HOME` 之外全部可读。只拒绝读取内容，不拒绝查看文件是否存在。
- 网络不受限制。
- 启动自检：用一条拒绝写入的规则在沙箱中运行 `sh`，确认写入确实被拒绝。

理由见 [每轮一次 OpenCode 与 crew 命令](../../.agents/notes/implemented/architecture/2026-10-04-opencode-turns-and-crew-cli.md)。

## 5. 本机目录

```text
~/.crew/
├── agents/<agent-id>/                 持久：跨运行保留
│   ├── AGENTS.md                      身份与规则
│   ├── work/                          OpenCode 的工作目录
│   │   └── MEMORY.md                  Agent 的记忆，由它自己维护
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
- Agent 在沙箱里能写自己的两个目录，所以 Computer 在建目录、写 session 与凭证之前，逐级用 `lstat` 确认它们是真正的目录；遇到符号链接或其他类型时拒绝。`session.json` 不是普通文件时当作没有记录。
- 准备目录时，工作目录里还没有 `MEMORY.md` 就写一份模板；已有时不动，不是普通文件时也不动。记忆不放进 `AGENTS.md`，也不放进每轮输入：前者会让每次修改都重开会话，后者让每轮都多出这些 token。理由见 [提醒、记忆与静音 Note](../../.agents/notes/proposed/feature/2026-10-05-reminders-memory-mute.md)。
- `AGENTS.md` 只随 Agent 的设置变化，不含路径与时间：身份与 handle、人设、怎样用 `crew reply` 发言、HELD 时怎么做、群聊的发言约束、讨论串（在消息来的地方回复，只在用户要求时开讨论串）、任务（动手之前先领取，进展发在任务的讨论串里，做完改成待审）、提醒（说以后要做的事就定提醒，到点在房间里收到通知并被唤醒）、记忆（新会话先读 `MEMORY.md`，有长期价值的东西写进去，保持在 16KB 以内）。文本由 `packages/computer/test/__snapshots__/AGENTS.md` 逐字锁定。
- `bin/crew` 以 `ELECTRON_RUN_AS_NODE=1` 运行 Electron 可执行文件与打包后的 `shim.js`。两者都在 `$HOME` 之外，沙箱里可以读取。

## 6. crew 命令

- `crew reply <room-id>`：正文从 stdin 读取，去掉末尾的空白；带上 `CREW_TOKEN_FILE` 中的凭证调用 `CREW_SERVER_URL` 的 `POST /agent/reply`。正文不经过命令行，所以反引号与 `$` 不会被 shell 改写。
- `crew reply <room-id> --thread <message-id>`：发到房间里这条消息的讨论串，讨论串还没有时创建。成功时写出讨论串的 ID，之后用 `crew reply <thread-id>` 接着在里面发言。私聊、讨论串里与别的房间的消息被拒绝时，写出对应的英文说明。
- `crew task list|create|convert|claim|status|assign`：操作任务（`packages/computer/src/shim/tasks.ts`）。房间 ID 也可以是任务的讨论串；handle 可以带 `@`；`status` 的 `--note` 写进通知，退回时写要改什么、改成待审时写做了什么；成功时写出任务的编号、标题、状态、负责人与汇报的地方，被拒绝时按 Server 返回的 `refusal` 写英文说明。
- `crew remind <room-id> <title> --in|--at|--every|--daily|--weekly`、`crew remind list`、`crew remind cancel <id>`：给自己定提醒（`packages/computer/src/shim/reminders.ts`）。时间按本机时区：`--at 18:00` 是下一个 18:00，也接受 `2026-10-06 09:00` 与带时区的 ISO 8601。成功时写出提醒的时间（带时区偏移）与到点会怎样；时间写错、周期太短在本地拒绝，其余拒绝按 Server 返回的 `refusal` 写英文说明。
- `crew --help`：用法与 heredoc 示例。
- 本地先检查：房间 ID 与 `--thread` 的消息 ID 是 UUID，正文不为空且不超过 20,000 字符。
- 成功时向 stdout 写 `Message sent to room <room-id>.`，退出码 0。失败时向 stderr 写一行英文 `error: …`，说明原因与下一步，退出码 1；Server 的 401、403、404 由 `crew` 翻译成英文。
- 被 HELD 拦下时，向 stdout 写“没有发出”、新消息（格式与每轮输入相同）与下一步：再运行一次 `crew reply`，或什么也不做。退出码 1。
- 请求最多等 10 秒，不重试；超时时提示消息可能已经发出、不要重发。
- 全部输出由 `packages/computer/test/__snapshots__/shim-output.md` 逐字锁定。

理由见 [每轮一次 OpenCode 与 crew 命令](../../.agents/notes/implemented/architecture/2026-10-04-opencode-turns-and-crew-cli.md)。

## 7. 验收

| 条目 | 测试 |
|---|---|
| Seatbelt 规则：路径不进规则文本、`$HOME` 内的读取限制、写入范围、自检 | `packages/computer/test/sandbox.test.ts` |
| 沙箱不可用时不创建 Runner，并上报原因 | `packages/computer/test/daemon.test.ts` 的 `starts no runner and reports the reason when the sandbox is unavailable` |
| 启动前到达的消息、SSE 唤醒、新建的 Agent 都能被处理；退出时删除本次运行目录 | `daemon.test.ts` |
| Turn 成功后确认已读；失败时保留消息并在下次重试；运行中的唤醒合并成一轮；停止时不确认 | `packages/computer/test/runner.test.ts` |
| session 在 Engine、模型与 `AGENTS.md` 不变时延续，否则重开 | `packages/computer/test/home.test.ts` 的 `session continuity` |
| 不顺着 Agent 换成的符号链接或命名管道操作；准备失败的 Agent 上报 error，其他 Agent 不受影响 | `home.test.ts` 的 `paths the agent controls`；`daemon.test.ts` 的 `reports an unsafe agent directory and keeps serving the other agents` |
| 准备失败时 Computer 退出，不空转 | `packages/computer/test/main.test.ts` 的 `exits with the reason instead of idling when it cannot prepare its directories` |
| OpenCode 的参数、环境隔离、session 重试、失败分类、进程组停止、输出超限；一轮结束后不留下后台进程，也不被占着管道的后台进程卡住；沙箱里的 zsh 能运行 heredoc | `packages/computer/test/opencode.test.ts` |
| Engine 事件的解析（样本取自真实输出）与截短 | `packages/computer/test/opencode-events.test.ts` |
| 价格表复制进 Agent 的缓存，不顺着符号链接写，副本较新时跳过 | `opencode.test.ts` 的 `copies the user's model price table…`；`home.test.ts` 的 `refuses to copy into the cache through a directory replaced by a link…` |
| 每一轮登记唤醒的消息与完整输入，事件按顺序上报，结束写结果；停止时记为已停止 | `runner.test.ts` 的 `records each turn…`、`stops a running turn…` |
| `crew` 原样提交正文，拒绝写在命令行上的正文，各类失败退出码为 1；被 HELD 拦下时打印新消息，再次运行后发出；`--thread` 开出并复用讨论串；`crew task` 新建、领取、改状态与被拒绝；`crew remind` 设定、列出、取消与被拒绝；输出逐字锁定 | `packages/computer/test/shim.test.ts` |
| `AGENTS.md` 与每轮输入的文本逐字锁定 | `home.test.ts`、`packages/computer/test/prompt.test.ts` |
| 构建产物的完整链路：用户发消息，Seatbelt 中的 Engine 经构建好的 `crew` 回复并落库 | `apps/desktop/test/smoke.e2e.ts` |
| 真实模型的完整链路 | 手动：`CREW_E2E_MODEL=<模型> pnpm test:e2e` |
| 真实模型的运行记录：工具调用、HELD、回复、用量与费用都记进这一轮 | 手动：2026-10-05 用 `deepseek/deepseek-flash` 在群聊里让 Alice 写文件并确认，记下 5 步、HELD 1 次、回复 1 条、费用约 0.0015 美元；没被点名的 Bob 两轮都是白跑 |
| 真实模型的群聊：全员唤醒、Agent 之间的 @、HELD 后改写再发 | 手动：2026-10-05 用 `deepseek/deepseek-flash` 跑一个两人群聊，Alice 被 HELD 后把补充的信息写进回复再发出 |
| 真实模型的讨论串 | 手动：2026-10-05 用 `deepseek/deepseek-flash`，用户在群里 @alice 提问，Alice 与 Bob 都被唤醒（Bob 白跑）；用户在那条消息的讨论串里 @alice，只有 Alice 被唤醒并在讨论串里回复，群聊时间线里只多了讨论串摘要 |
| 真实模型的任务 | 手动：2026-10-05 用 `deepseek/deepseek-flash`，用户把一条消息转成任务并分配给 @alice，Alice 领取、写好文件、改成待审并在任务的讨论串里汇报；用户改成完成。同一轮里 Alice 也领取了用户新建的未分配任务并做完 |
| 准备目录时写记忆模板，不覆盖 Agent 写的内容，不顺着符号链接写；新会话的输入里提醒先读记忆，超过 16KB 时请它精简 | `home.test.ts` 的 `seeds MEMORY.md…`、`leaves a MEMORY.md that is not a regular file alone…`；`prompt.test.ts` 的 `turnPrompt in a new session`；`runner.test.ts` 的 `tells the agent to read its memory in a new session…` |
| 真实模型的记忆 | 手动：2026-10-05 用 `deepseek/deepseek-flash`，告诉 Alice“我叫小王，以后用英文回答”，她写进 `MEMORY.md`；删掉会话记录让下一轮开新会话后问“我叫什么名字”，她先读记忆，用英文答出名字 |
| 真实模型的提醒 | 手动：2026-10-05 用 `deepseek/deepseek-flash`，19:14 请 Alice“两分钟后提醒你自己告诉我几点”，她定了提醒并回复“好的”；19:16:06 私聊里出现“Alice 的提醒到了”，她被唤醒，回复“现在是 19:16” |
