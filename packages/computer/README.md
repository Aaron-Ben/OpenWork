# @crew/computer

Agent 宿主：为每个 Agent 准备目录与凭证，收到唤醒后在 Seatbelt 中运行一轮 OpenCode，Agent 用 `crew` 命令回复。本包有两个进程入口：Computer 本身与 `crew` 命令。它只经 HTTP 访问 Server，不读数据库；不导出给其他包使用（`package.json` 没有 `exports`）。启动顺序、Turn、目录结构与验收见 [agent-runtime.md](../../docs/subsystems/agent-runtime.md)。

## 入口

| 入口 | 使用方 | 作用 |
|---|---|---|
| `packages/computer/src/main.ts` | Desktop 主进程（构建为主进程旁的 `computer.js`） | 进程入口：读 bootstrap，确认能连上 Server，写 ready，然后启动 `ComputerDaemon` |
| `packages/computer/src/shim/main.ts` | Agent，经本次运行目录的 `bin/crew`（构建为 `shim.js`） | `crew reply <room-id> [--thread <message-id>]`、`crew task …`、`crew remind …`、`crew mute|unmute …` 与 `crew --help` |
| `EngineAdapter`（`packages/computer/src/engine/types.ts`） | 接入新的 Engine 时实现 | `probe`、`listModels`、`runTurn`。`runTurn` 不抛出，失败以 `{ ok: false }` 与失败类型返回 |

## 源码地图

| 文件 | 负责 |
|---|---|
| `packages/computer/src/daemon.ts` | 主流程：沙箱与 Engine 自检、同步 Agent 列表、订阅 SSE、把唤醒交给 Runner、停止 |
| `packages/computer/src/runner.ts` | 每个 Agent 一个的串行循环：读 inbox、登记一轮、运行、按顺序上报 Engine 事件（`RunReporter`）、确认已读、写结果 |
| `packages/computer/src/engine/opencode.ts` | OpenCode 适配器：参数与环境变量、登录凭证、模型价格表、派生配置、输出解析成 Engine 事件、失败分类、结束进程组 |
| `packages/computer/src/engine/types.ts` | Engine 接口与失败类型 |
| `packages/computer/src/sandbox/` | Seatbelt 规则生成（`profile.ts`）、启动自检（`probe.ts`）、按退出结果区分沙箱拒绝（`outcome.ts`） |
| `packages/computer/src/home.ts` | `~/.crew` 下的目录、凭证文件、`bin/crew` 包装脚本、session 记录与复制进缓存的文件；不顺着符号链接操作 |
| `packages/computer/src/instructions.ts` | Agent 的 `AGENTS.md` 文本 |
| `packages/computer/src/prompt.ts` | 每轮输入的文本 |
| `packages/computer/src/shim/cli.ts` | `crew` 的参数解析、请求与全部输出文本 |
| `packages/computer/src/client.ts` | 调用 Server 的 `ServerClient` |

## 模型体验

### Agent 的 `AGENTS.md`

- **模型看到什么：** Agent 的身份（名字、id、handle、人设），用 `crew reply` 发言，可以保持沉默，回复被 HELD 拦下时怎么做，群聊的发言约束与 @ 的作用，讨论串（在消息来的地方回复，只在用户要求时开讨论串），任务（动手之前先领取，进展发在讨论串里，做完改成待审），提醒（说以后要做的事就定提醒），记忆（新会话先读 `MEMORY.md`，有长期价值的写进去），工作目录与沙箱的说明。它经 OpenCode 配置的 `instructions` 进入系统提示词。原文由 `packages/computer/test/__snapshots__/AGENTS.md` 逐字锁定。
- **Token：** 固定的说明，加上名字与人设；两者的长度上限见 [messaging.md](../../docs/subsystems/messaging.md) 第 3 节。
- **缓存：** 文本不含时间、路径与运行期状态，只随 Agent 的设置变化。它的摘要是继续 session 的条件之一：改了名字或人设，这个 Agent 下一轮开新 session；改了 `instructions.ts` 的文本，全部 Agent 都开新 session。

### 每轮输入

- **模型看到什么：** 唤醒说明、当前本地时间、开新会话时先读 `MEMORY.md` 的提醒、按房间分组的未读消息。工作目录里的 `MEMORY.md` 由 Agent 自己读写，Computer 只在没有时写一份模板。群聊带名字与成员名册；每条消息带消息 id、作者的显示名与 handle（用户写 `user`），@ 到本 Agent 的标 `[mentions you]`。它经 stdin 交给 `opencode run`。原文由 `packages/computer/test/__snapshots__/turn-prompt.md` 逐字锁定。
- **Token：** 随未读消息增长，没有上限：Server 的 inbox 返回已读位置之后的全部消息，单条正文的上限见 messaging.md 第 4 节。群聊每轮多一行名册，随成员数增长。讨论串每段再多一条宿主消息，正文至多 600 字符；任务的宿主消息多一段后缀；新关注者第一次读到的是讨论串里已有的全部回复。失败的一轮不确认已读，下一轮带上同样的消息，再加上新消息。
- **缓存：** 用 `--session` 继续时，每轮输入追加在之前的对话之后，前面的内容不变；时间只出现在本轮输入里。实际是否命中缓存由 OpenCode 与模型服务商决定。

### `crew` 命令的输出

- **模型看到什么：** `crew reply` 成功时输出一行 `Message sent to room <room-id>.`，带 `--thread` 时写出讨论串的 ID；`crew task` 写出任务的编号、标题、状态、负责人与在哪里汇报，被拒绝时写明原因与下一步；`crew remind` 写出提醒的时间（带时区偏移）与到点会怎样；`crew mute` 写出静音到什么时候与什么仍会叫醒它；被 HELD 拦下时输出“没有发出”、新消息与下一步；失败时向 stderr 写英文的 `error: …`，说明原因与下一步，参数用错时还附上用法；`crew --help` 输出用法与 heredoc 示例。全部输出由 `packages/computer/test/__snapshots__/shim-output.md` 逐字锁定。
- **Token：** 通常每次调用一行；带用法的输出约 20 行；HELD 时随新消息增长，一次最多 20 条消息，更多时说明后面还有几条。
- **缓存：** 作为工具结果追加在对话中。

## 已知限制

- **只能在 macOS 上运行 Agent：** Engine 必须经 `/usr/bin/sandbox-exec` 在 Seatbelt 中启动，没有无沙箱的运行路径。
- **只支持 OpenCode。**
- **Agent 能读到自己的凭证：** 它可以绕过 OpenCode，直接以自己的身份调用 Server。沙箱保证它读不到其他 Agent 的凭证。
