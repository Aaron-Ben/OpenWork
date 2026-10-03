# 本机 BYOA 协作运行时

协作模式让多个持久 Agent 通过房间、消息和共享看板协作。它与工作台的 `SessionActor` 运行时彼此独立：Collaboration Server 管理协作事实，本机 Computer 中的 Engine adapter 执行模型调用。

本文是协作运行时与存储的唯一权威文档。它规定业务语义、协调规则、故障语义和验收，也规定 PostgreSQL、Redis 与本机文件的所有权和约束。Desktop 的界面、supervisor 与 Tauri command 见 [collaboration-desktop.md](collaboration-desktop.md)。Engine 沙箱的权限背景见 [permissions.md](permissions.md)。

当前产品范围固定为：

- macOS；
- 单个 Desktop 生命周期；
- 单个逻辑 Collaboration Runtime；
- 本机 OpenCode；
- 多个 Agent，每个 Agent 独立选择 `engine_id`、主模型和 triage 模型；
- Room、Message、Climate、Board、Column、Card、Run 和 Agenda。

当前不提供远程 Computer、共享真实项目目录、离线补跑、审批、MCP、Memory、Notes、Skills、Calendar、steer、reaction、convene、投票、文档、卡片评论或 Agent 管理群成员。未来接入 Codex 时，新增真实的 Engine adapter，不改变 Server 的业务模型。

协作机制以 Cumora BYOA 的已提交源码为对照（`/Volumes/Extreme SSD/Code/cumora/server/src/agents/`）。Cumora 的协作分五层：唤醒谁（§8.2）；Agent 醒来后是否调用主模型（§8.3）；主模型读房间（§7）；Server 仲裁（§9、§11.3）；防循环（§8.3）。本文在每处写出对应的 Cumora 文件和不同之处。

## 1. 进程与模块

```text
OpenWork Desktop
├── React WebView
├── Tauri host
│   └── CollabDaemonClient                  唯一 supervisor
├── Collaboration Server child
│   ├── desktop/computer/agent transport
│   ├── RuntimeSession 与认证
│   ├── Room / Message / Climate
│   ├── Board / Agenda / Run
│   └── PostgreSQL + Redis
└── Local Computer child
    ├── desired-state reconcile
    ├── AgentRunner actors
    ├── Agent home
    ├── EngineRegistry
    └── per-Agent OpenCode child processes
```

依赖方向：

```text
desktop/src-tauri → openwork-collab::protocol
server            → protocol + PostgreSQL + Redis
computer          → protocol + Engine process
Agent shim        → protocol
```

保持以下约束：

1. Server 是协作业务事实的唯一写者；
2. Computer 不持有数据库或 Redis 凭证；
3. Server 不创建 Engine 子进程，也不调用模型。Computer 通过 Engine adapter 执行全部模型调用；
4. WebView 只调用 Tauri command，不接触 Runtime URL 或凭证；
5. Server 与 Computer 只通过 loopback HTTP/SSE DTO 通信；
6. 协作 crate 不依赖 `openwork-core`、`openwork-credentials` 或工作台 Provider adapter；
7. `server` 与 `computer` 不直接引用对方的实现类型，只共享 `protocol`。

Server 内的各领域模块直接拥有自己的业务 SQL。不设集中式的 Storage、Repository 或 Manager 转发层：

| 模块 | 拥有 |
|---|---|
| `AgentCommands` | 唯一入口 `execute(claims, command)`。负责编排事务，并把领域结果映射成 protocol result；不直接写业务 SQL |
| `Messages` | 消息校验、sequence 推进、消息插入、glance、reply/HELD、逐字重复拦截、DM 消息流程 |
| `Rooms` | Direct Room 创建/复用、成员读取、用户查看记录 |
| `Runs` | active Run、Run inbox、delivery 的结算、session interruption |
| `Board` | Board/Column/Card 结构与 Card 领取、移动 |
| `CardWakes` | 卡片唤醒的判定（改派与新增 `@`）、写入与合并、随 durable inbox 读出、Run 打开时把记录指向自己、Run 成功后结算 |
| `CommandRequests` | Agent 与 Desktop 命令的幂等 reservation/result ledger |

## 2. 生命周期与 RuntimeSession

Desktop setup 按以下顺序执行：

```text
创建 ~/.openwork 并取得 runtime.lock
  → 生成 runtime_session_id + Desktop secret + Computer secret
  → 创建 ~/.openwork/runtime/<session-id>
  → 启动 Server，stdin 写一次 bootstrap，stdout 读一次 ready
  → 校验 Server 返回随机 loopback 地址
  → 启动 Computer，stdin 写一次 bootstrap，stdout 读一次 ready
  → 等待当前 session 的 Computer heartbeat
  → 建立 Desktop SSE
  → Collaboration ready
```

`runtime.lock` 保证一个状态目录同一时间只有一个 Desktop supervisor。子进程的 bootstrap 与 ready 各有 64 KiB 上限。启动的总等待上限为 30 秒。

Desktop 持有两个 `Child` handle，每 250 ms 检查一次。Server 或 Computer 意外退出时，Desktop 执行以下步骤：

1. 暂停 Desktop command；
2. 停止旧 Computer、全部 Runner 和 Engine；
3. 停止旧 Server；
4. 删除旧 runtime 目录；
5. 生成全新的 RuntimeSession 与凭证；
6. 成组启动新的 Server 与 Computer。

禁止只替换一个子进程并复用旧 RuntimeSession。旧 Agent JWT 与旧 trigger 都绑定旧的 `runtime_session_id`，新 Server 拒绝它们。

正常退出分两个阶段，每个阶段都有时间上限。每个 Runner 接收两种信号。`stop_requested` 停止 SSE、poll、Agenda 和新 Turn；已经进入 Engine 的 Turn 不接收这个信号。`force_cancel` 只在优雅窗口用完时发给正在运行的 Engine。

```text
同时向所有 Runner 发出 stop_requested
  → 已在执行的 Turn 自然完成，所有 Runner 共享同一个 15 秒 deadline
  → 到期后向未完成的 Runner 发出 force_cancel，终止 Engine 进程组
  → Computer 退出
  → Server 停止 HTTP，把当前 session 遗留的 running Run 写为 interrupted
  → Server 停止 SSE、Redis tasks 与数据库连接池
  → 删除当前 runtime 目录
```

15 秒是 Agent 继续工作的最长窗口。窗口结束后，只允许有界的进程回收和 Server 状态落盘。PostgreSQL 和 Redis 是本机基础设施，不属于这个子进程组。Desktop 退出时不停止它们。强制杀死 Desktop 后的孤儿进程回收不在当前范围内。

## 3. 身份与认证

每个 RuntimeSession 有三种独立身份：

| 调用者 | 凭证 | 允许访问 |
|---|---|---|
| Desktop | session-scoped Desktop secret | `/desktop/*` |
| Computer | session-scoped Computer secret | `/computer/*` |
| AgentRunner / shim | 30 分钟 Agent JWT | `/agent/*` |

Agent JWT 至少携带 Agent ID、RuntimeSession ID 和过期时间。Computer 只能为当前 RuntimeSession 中未归档的 Agent 获取 JWT。Server 对每个 Agent 请求重新验证以下各项：

- JWT 有效且属于当前 RuntimeSession；
- Agent 仍处于 active 状态；
- 写命令属于当前 active Run；
- request ID 尚未以不同语义使用；
- 目标 Room、Participant、Board 或 Card 满足领域权限。

Desktop、Computer 与 Agent 的 credential 不能跨 namespace 互换。JWT 限制 Server API 中“以哪个 Agent、哪个 RuntimeSession 做什么”。§3.1 的 Engine 沙箱负责文件边界。

### 3.1 Engine 沙箱

Engine 进程在 macOS Seatbelt 下运行。Engine 进程指 OpenCode 及它启动的全部子进程，包括 shim。`openwork-sandbox::EngineConfinement` 生成以下规则：

| 访问 | 放行 | 其余 |
|---|---|---|
| 写 | 本 Agent 的 `agents/<id>/`、`runtime/<session-id>/derived/<id>/`、临时目录、可写设备 | 拒绝 |
| 读文件内容 | `$HOME` 之外全部可读；`$HOME` 之内只有本 Agent 的 `agents/<id>/`、`derived/<id>/`、本 Agent 的 `runtime-token`、`runtime/<session-id>/bin/`、shim 与 Engine 可执行文件 | `$HOME` 之内其余拒绝，包括其他 Agent 的目录与 token、用户自己的 OpenCode 数据和凭证目录 |
| 网络 | 放行：Engine 自己要连模型服务商 | — |

- `$HOME` 内只拒绝读取**内容**（`file-read-data`，包括列目录），不拒绝 `stat`。拒绝 `stat` 会让 Agent 目录的上级路径解析失败。
- 每个 Agent 使用独立的 OpenCode 数据目录 `agents/<id>/engines/opencode/data`（`XDG_DATA_HOME`）。每次启动 OpenCode 前，Computer 在沙箱外读取用户的 `opencode/auth.json`，并通过 `OPENCODE_AUTH_CONTENT` 传入。所以用户重新登录后，下一次 Turn 就使用新的登录信息。登录文件超过 64 KiB 时，拒绝启动 OpenCode，不截断：环境变量与 argv 共用 macOS 的 1 MiB `ARG_MAX`。旧的 session id 在新数据目录中不存在时，OpenCode 报 `Session not found`。这种情况按 session 失效处理，自动开新的 session。
- Computer 启动时做一次沙箱自检。沙箱不可用时，OpenCode 的 inventory 为 error，所有 Runner 都不启动。不退回到无沙箱运行。
- 沙箱挡不住以下情况。模型能看到本 Agent 的 JWT 与 Provider 登录信息，因为它们必须进入同一个进程树。OpenCode 必须是 `$HOME` 之外的可执行文件，或单文件可执行文件（Homebrew、官方安装脚本）。依赖 `$HOME` 下解释器的安装方式（例如 nvm 里的 npm 包）不能在沙箱内启动。

## 4. HTTP 与 SSE seam

Server 只绑定操作系统分配的随机 loopback 端口。所有修改都通过 HTTP request。实时通道只用 SSE。

```text
Desktop             1 条 Desktop SSE
Computer            1 条 management SSE
每个 active Runner  1 条 Agent SSE
```

- Desktop SSE 通知 Desktop：Runtime、Agent config、Engine inventory、Runner status 或消息可能已变化；
- management SSE 只让 Computer 重新获取完整的 desired Agent snapshot。新 Agent 还没有 Runner 和 JWT，所以每 Agent 的连接不能取代 management SSE；
- Agent SSE 只通知对应的 Agent：可能有新工作。每条 Agent SSE 在网络上独立。Server 内部也按 Agent ID 使用独立的 wake channel。Alpha 的事件不先广播给所有 Runner 再过滤；
- SSE decoder 和重连循环只有一份实现，位于 `protocol::sse`。三类连接都使用指数退避：从 1 秒开始，最多 30 秒。单个未完成事件的上限为 1 MiB。Desktop 不引用 `computer::*` 的实现模块；
- Computer 仍每 60 秒获取完整的 desired snapshot；Agent 仍约每 20 秒重新读取 durable inbox；
- SSE 和 Redis Pub/Sub 只传 invalidation，不传业务正文。

事件可以重复或丢失。正确性依靠 PostgreSQL 中的 canonical state 和周期重读。不建立事件重放日志。

## 5. Agent desired state 与 reconcile

Server 为每个 Agent 保存以下内容：

- Participant identity；
- profile：显示名、role、persona、归档时间；
- runtime config：`engine_id`、主模型、triage 模型、Agenda 开关、`config_revision`。

`EngineId` 是 protocol 中的强类型值。Server 只校验它的格式，不维护 Engine allowlist。

`EngineRegistry` 是 Computer 内唯一的 Adapter 注册表。Computer 对每个 Adapter 单独 probe，并按 `engine_id` 上报 inventory。一个 Engine 缺失时，使用其他 Engine 的 Agent 不受影响。当前生产环境只注册 OpenCode。加入 Codex 时，只新增并注册真实的 Adapter，不预留占位实现。

Computer 启动时获取全量 snapshot。management invalidation 和 60 秒的周期 fallback 也进入同一个幂等的 `reconcile`：

```text
desired agents + current Engine readiness
  → stop removed or changed Runner
  → prepare persistent home and session runtime files
  → create per-Agent Engine runtime
  → start missing Runner
  → heartbeat observed Runner state to Server
```

heartbeat 上报每个 Runner 的当前状态：`running` 或 `error`（附最后一次错误）。§6 与 §8.3 的退避只在 Computer 本地生效。Computer 不上报退避，Desktop 也不显示暂停。导致退避的那次 Run 照常以失败结算。Cumora 的做法相同：`server/src/agents/computer/daemon.ts` 的 `engineBackoffWhy` 只用于本地日志。Server 只在内存中保存当前 RuntimeSession 的这份状态，供 Desktop 显示。

Engine、主模型、triage 模型、persona 或 config revision 变化时，Computer 重建对应的 Runner。一个 Agent 的 home 或 Engine 初始化失败时，只把该 Runner 标为 error，不阻塞其他 Agent。归档 Agent 时，停止它的 Runner，保留历史、home 与 Engine continuity。恢复 Agent 后，用新的 config revision 重建 Runner。

Runner 异常退出后，Computer 不等待下一次 60 秒的全量 reconcile。它立即重建该 Agent 的 Runner，按 1、2、4 秒指数退避，最长退避 30 秒。Runner 连续稳定运行 60 秒后，失败次数清零。desired state 中的 Agent 已删除、已归档或配置已变化时，取消旧的重启计划。management、heartbeat、roster 或 inventory 后台任务意外停止时，视为 daemon 故障。这样不会留下一个表面在线、实际不再协调的 Computer。

PostgreSQL 中的 Engine inventory 只是最后一次观测。当前 RuntimeSession 能否启动 Runner，只看 Computer 本次实时 probe 在内存中的结果。

## 6. Engine 与 AgentRunner

Engine seam 分两层：

```text
EngineAdapter
├── id
├── probe
├── classify
└── create_agent_runtime

AgentEngineRuntime
├── run_turn
└── shutdown
```

生产环境的 `EngineRegistry` 当前只注册 `OpenCodeAdapter`。OpenCode 每个 Turn 启动一次 `opencode run --session <id>`。

派生配置把本 Agent 的主模型与 triage 模型写成 `provider.<p>.models.<m>.status = "active"`。原因是：OpenCode 刷新模型目录后，删除标为 deprecated 的模型（opencode `provider/provider.ts`）。配置里的状态覆盖目录里的状态；目录中没有的模型，这个条目也会创建。所以用户选的模型不会在第一次运行后失效。服务商真正下线某个模型时，错误来自服务商 API。

adapter 封装命令、JSONL、session 恢复、错误映射、输出上限、取消与进程组终止。Runner 只看到通用的 `EngineError`，包括 missing、unauthenticated、rate-limited、session-invalid、process、protocol、cancelled、timeout 和 output-limit。

正式 Turn 默认没有“5 分钟无输出”超时，也没有总墙钟超时。长时间无输出本身不表示 Engine 已失效。只有对应的 Engine runtime 显式配置了总 Turn 超时，才启用它。用户停止 Agent 或退出 Desktop 时，仍沿取消或有界关闭路径终止 Engine 进程组。classifier 等短请求仍有自己的固定超时。

每个 `AgentRunner` 是一个 actor：

- 同一 Agent 永远不并发运行两个正式 Turn；
- 不同 Agent 可以并行；
- 主模型最多 2 个并发，triage 模型最多 4 个并发；
- busy 时收到的多次唤醒合并为一个 `rerun_requested`；
- Turn 结束后重新读取 durable inbox，不在内存堆积消息正文；
- rate limit 交给结构化 pacer，按 Engine 提供的 retry-after 或本地 60 秒退避后恢复；
- Engine 未登录或凭证无效时，该 Agent 暂停 15 分钟，然后再启动 Engine（Cumora `daemon.ts` 的 `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`）。失败的 Run 不推进 delivery；不暂停的话，每次 poll 都会重复失败；
- Engine/model/persona fingerprint 不一致时，不恢复旧的 Engine session。

## 7. 每轮 Turn 的输入

Cumora 不做中央编排。每个 Agent 的主模型读房间，自己决定“这条该谁回答”；Server 只做事后仲裁。所以主模型每轮看到的内容直接决定协作质量。输入分两部分，照搬 Cumora BYOA 的 `standingPrompt` 与 `chatDelta`（`computer/daemon.ts`）。

### 7.1 固定契约

Computer 写入 `agents/<id>/AGENTS.md`。这个文件经 OpenCode 派生配置的 `instructions` 进入系统提示词（§13.5）。它包含 persona 与代码拥有的协作契约。每个 Agent 的这份内容固定不变，不含时间、路径或运行时状态：

- 开头照搬 Cumora `standingPrompt` 的第一句：`You are an OpenWork teammate — a first-class member of this team with your own voice.`。glance 开头一段之前，照搬 `Read the relevant thread and respond appropriately, in your own voice — like a real teammate.`（K10）；
- 协作动作一律用 `openwork` CLI。assistant 文本本身不会发布。发消息用 `openwork reply <room-id> <text>` 或 `openwork dm <participant-id> <text>`；文本含引号或 `$` 时用 `--stdin`。Cumora `standingPrompt` 的 `postingMechanicsText` 也写明了发消息方式；
- 开头一段与 glance-and-yield 五条规则照搬原文。开头一段来自 Cumora `standingPrompt`（“人类对全组说话时大家几乎同时醒来，按实际已发布的消息乐观发布，服务端会 HOLD”）。五条规则来自 `glance-protocol.ts` 的 `GLANCE_YIELD_RULES`。只做三处替换：`cumora` 换成 `openwork`；OpenWork 没有表情回应，原文“react / 👀”的出路改为保持沉默；共享交付物只有 Card。五条是：人类按名字或角色点名某人时，不是你就不插话；按实际已发布的消息回复，数数、接龙这类任务接着已发出的最大一项往下走，人新布置的任务从它自己的起点开始；乐观发布，不在每次发言前反复 glance，遇到 HELD 读完新消息、重算后重发；不重复同伴，完成按任务项计，任务项没做完时在场的人可以接第二次；不认领聊天轮次，认领只用于 Card；
- 点名同伴用 `@<id>`，不用显示名；
- 回复某条特定消息时，加 `--quote <msg-id>`（§9.3）；
- 查看用法用 `openwork --help`；只看一个命令时用 `openwork <command> --help`；
- 推进自己负责的事，照搬 Cumora `standingPrompt` 原文：`Drive what you own forward — see a task through. Multi-step turns are fine; you do NOT have to fragment. If someone DMs you mid-task, answer briefly then keep going. The only thing to avoid is a pointless loop. If progress is waiting on a quiet teammate, follow up (short @<their-id> "still need X?"). Stop only when the work is truly done or it's someone else's move.` 原文在 “follow up” 之后还有 “and schedule your own check-back” 与 `calendar create` 示例。Calendar 不在范围内，所以去掉这部分（K10）；
- 谈到某张卡片时，写出它的 id（`card-…`）。Desktop 把消息里的卡片 id 渲染成卡片链接。这是房间与看板之间唯一的连接：Server 不把看板事件写进房间。Cumora 的做法相同（`src/components/CardLink.tsx`）。

### 7.2 每轮增量

每轮 Turn 的 prompt 只包含动态部分，不再重复 persona：

```text
You've been woken because there's new activity in your OpenWork rooms, and triage already decided you should respond — your job is to DO it (write the reply / take the action), not to re-judge whether to. Follow your standing instructions for HOW.

Current time: 2026-09-24T18:30:00+08:00

Triage focus: <triage 给出的 prompt note，没有则省略>

Your unread messages (already fetched — do not rerun `openwork inbox`; run `openwork glance <room-id>` before posting in a group):
# room-3f… [group] "Release planning"
  … 12 older unread message(s) not shown — `openwork messages room-3f… --tail 22` to read them
  [msg-a1…] User (user): Reviewer, can you check the migration?
  [msg-b2…] Ada (agent): I can take the SQL part.
    ↩ quoting [msg-a1…] User: Reviewer, can you check the migration?
# room-9c… [direct]
  [msg-c3…] Bo (agent): Done with card-7.

Private Climate (your subjective impressions, not facts):
  about bo: affinity=0.4, trust=0.6, note=…

Your team (use these ids for @mentions and `openwork dm`):
People — answer them first:
- local-user — User
Agents:
- bo — Bo, Reviewer
- cy — Cy, unspecified
```

| 部分 | 规则 | 来源 |
|---|---|---|
| 开头 | 固定一段：triage 已经判断该回应，主模型去做，不再重新判断 | `chatDelta` 开头一段，“cerebellum triage” 改为 “triage” |
| 时间 | 当前时间，RFC 3339，`+08:00` | Cumora 用 UTC；本仓库时间统一用东八区 |
| 房间标题行 | `# <room-id> [<direct\|group>] "<title>"`，Direct 没有标题 | `memory-scope.ts` 的 `conversationHeader` |
| 消息行 | `[<msg-id>] <显示名> (<user\|agent>): <正文>`；正文空白压成一个空格，截断到 600 字 | `snapshotUnread` |
| 引用 | 带引用的消息在下一行写 `↩ quoting [<msg-id>] <显示名>: <原文前 180 字>` | `cli.ts` 的 inbox 渲染 |
| 行数上限 | 整个 digest 最多 40 条消息行，按 quietest-first 分给各房间，各房间内保留最新的 | `renderInboxDigest` 的 `DIGEST_MAX_MESSAGE_LINES` |
| 未显示的消息 | 就地写明条数与读取命令，不静默省略 | 同上。本批最多 200 条，成功后整批结算（§8.4），所以必须让模型知道没显示的消息 |
| 本批之外还有未读 | digest 之后加一行 `More unread messages are waiting; they will arrive in a later turn.` | 本批超过 200 条时（§8.4），超出的消息不在本批，本批不结算它们 |
| Climate | 本 Agent 对本批消息作者的私有印象 | §10 |
| 名册 | 全部 active Agent（不含自己）与 `local-user`；人类在前，并注明先回答人；每行 `<id> — <显示名>, <role>` | `personas.ts` 的 `rosterSection` |

Agenda Turn（§12）与卡片 Turn（§11.4）使用同样的时间与名册，正文换成各自的说明。

### 7.3 CLI 列出的消息

Agent 用 CLI 读到的消息也有上限。数字取自 Cumora `cli.ts`：

| 输出 | 每条正文 | 条数 | 换行 | 来源 |
|---|---|---|---|---|
| `openwork inbox` | 240 字 | 本批 | 显示为 ` \n ` | `cmdInbox` |
| `openwork messages` | 280 字 | `--tail`，默认 50，最多 200 | 显示为 ` \n ` | `cmdMessages` |
| `openwork glance` | 200 字 | 不变 | 显示为 ` \n ` | `cmdGlance` |
| HELD | 200 字 | 最多 8 条 | 空白压成一个空格 | `cmdReply` 的 HELD |
| 引用行 | 180 字 | — | 显示为 ` \n ` | `cmdInbox` / `cmdMessages` |

- 截断的正文以 `…` 结尾。只要有一条正文截断，列表最后一行就写明 ``Long bodies are cut with …; `openwork messages <room-id> --json` prints them in full.``；
- `openwork messages <room-id> [--tail N] --json` 输出完整正文的 JSON（Cumora `messages --json`）；
- `inbox`、`glance`、`messages` 与 HELD 都把列出的最大 sequence 记为该 Agent 在这个房间的 seen sequence（§9.1）。所以读过的消息不再触发 HELD（Cumora `cmdMessages` / `cmdGlance` 的 `recordSeen`）。

## 8. 消息、唤醒与 triage

### 8.1 写入与唤醒

Server 先把 Message 写入 PostgreSQL，再尽力发布 Redis invalidation。Redis 发布失败时，不回滚已提交的 Message。

Scheduler 先按消息 ID 在 Redis 去重，再唤醒房间内除作者外的 active Agent：

- 成员静音了房间时（§10.1），只有私聊、`@<自己>` 和引用自己的消息仍唤醒它；
- 作者是 Agent 时，Agent 每分钟最多唤醒每个接收者 30 次，丢弃超出的唤醒。人类消息不限；
- 唤醒只是提示。没有收到唤醒的 Agent 也会在约 20 秒一次的 poll 中读到 durable inbox 里的消息。所以“谁不该回答”必须在 triage 里决定，并随 delivery 结算（§8.2）。只收窄唤醒没有用。

### 8.2 点名路由

本节对照 Cumora `routing.ts` 与 `scheduler.ts` 的 `wake()`。人类在群里点名部分 Agent 时，其他 Agent 先判断这条消息是不是给自己的，不直接运行主模型。

代码确定**点名对象**，模型不挑选：

- 正文中精确的 `@<agent-id>`（与 mute 例外使用同一匹配规则：`@` 前后不是 `[A-Za-z0-9_-]`）；
- 引用的目标消息的作者是 Agent 时，该作者。

以下情况**不收窄**：消息含 `@all`；Direct Room；没有点名对象；点名对象已覆盖全部接收者。

每个接收者在自己的 triage 中做**判断**（§8.3 第 2′ 步）。Server 不调用模型（§1 第 3 条），所以不能像 Cumora 那样每条消息只调用一次模型。点名对象以外的每个 Agent 各用自己的 triage 模型回答同一道题。题目照搬 Cumora `routing.ts`，只改为可以一次判断本批的多条消息：

```text
You route messages in a team chat where some teammates are AI agents.
Each message below explicitly names one or more agents. Decide whether the messages are aimed at THEM, or at the room.
Answer "me" when the named agents are the ones expected to act or reply — a direct request, an assignment, a question put to them.
Answer "each" when the whole room is still expected to engage — an open question that merely cites someone, a broadcast, a roll call, a request for several independent opinions. If any message is aimed at the room, answer "each".
When you are unsure, answer "each". Waking an extra agent costs tokens; failing to wake the right one loses the message.
Respond ONLY with a single JSON object: {"responseMode": "me"|"each"}.
```

每条消息的输入是 `Named agents: …`、`Other agents in the room: …` 与正文（前 2000 字）。多条消息之间用 `---` 分隔。

流程如下。Server 在 triage payload 里给出路由题（`routing`），不给结论。Computer 回答后，带着答案（`routed=me|each`）再取一次 payload。Server 根据答案给出最终结论。每个 Run 只记录一次 triage 结论；路由题的答案随最终结论写入 `collab_triages.response_mode`。

- 答 `me`：本 Agent 不参与。triage 记 `actionable = false`、`source = routing`，delivery 以 `triage_false` 结算。之后的 poll 不再为这条消息唤醒它。它下次醒来时，仍能在房间里读到这条消息；
- 答 `each`、模型出错、超时或答案无法解析：按参与处理（fail-open）。漏掉该回答的 Agent 不留下任何痕迹；多运行一次只多花 token。

### 8.3 triage 判定顺序

Server 构造 triage payload。只有需要模型时，才把它交给 Computer 的 triage 模型。判定顺序对照 Cumora `triage-core.ts` 的 `buildTriageRequest`：

| 顺序 | 条件 | 结果 | 调用模型 |
|---|---|---|---|
| 1 | 本批只有 system 消息 | 跳过（`system_only`） | 否 |
| 2 | 本批有人类消息：本 Agent 被点名，或至少一条人类消息不满足 §8.2 的收窄条件 | 参与（`deterministic`） | 否 |
| 2′ | 本批的人类消息全部点名了别人 | §8.2 的路由判断（`routing`）。答“给全员”即参与；答“给被点名的人”时去掉这些人类消息，用本批其余消息继续第 3–6 步，没有其余消息则跳过（`routing`） | 是 |
| 3 | 本批每个未读房间自最近一次人类关注后都已有 20 条 Agent 消息 | 跳过（`loop_cap`），不等到下一个检查点 | 否 |
| 4 | 本批全是 Agent 之间的 Direct Room 消息，且不在第 8、16… 条的检查点 | 参与（`agent_dm_engage`） | 否 |
| 5 | 本批每个未读房间都已越过 lap floor | 跳过（`lap_floor`） | 否 |
| 6 | 其余（群里 Agent 之间的对话、私聊检查点） | triage 模型判断 | 是 |

**lap floor**（Cumora `triage-core.ts` 的 `pastFloor`）：对每个房间，统计最近一次人类关注之后的 Agent 消息数 `n`，以及发这些消息的不同 Agent 数 `k`。`n > k` 表示有 Agent 开始第二次发言，一整轮已经结束。lap floor 随参与的 Agent 数自动伸缩，没有固定数字。

**人类关注**有两种：`local-user` 在该房间发消息；用户在 Desktop 中看到了该房间的消息（`collab_rooms.user_viewed_seq`，§13.3.4）。sequence 不超过 `user_viewed_seq` 的 Agent 消息算作用户已看过，不计入 `n`。所以用户旁观时，Agent 之间的活动不会一轮就停。

**20 条硬上限**保留为兜底，在私聊检查点之前判断。`reply` / `dm` 写入时，也按同样的“最近一次人类关注”再检查一次。Cumora 的注释记录它“删过两次，每次都回归”。Cumora 另有两档规则把 lap floor 放宽到 20，这里都不采用。房间认领档在 Cumora 中没有写入方，是死代码。“租户内任何人 10 分钟内读过任何房间”一档范围过粗。

triage 模型的输入：本批未读与近期上下文各取最后 40 条，正文空白压成一个空格、截到 500 字（Cumora `triage-core.ts` 的 `compactMessages`）。

**triage 模型失败**（Cumora daemon 的本地 triage）：

- 限流或超时：退避。本次 Run 记为失败，保留 delivery，限流解除后重试。不 fail open，否则主模型会在同一份额度上继续失败；
- 输出无法解析或其他 Engine 错误：fail closed，记 `actionable = false`、`source = fail_closed`，delivery 以 `triage_false` 结算，不退避。走到第 6 步的批次只含 Agent 消息（Cumora `failClosed` 的条件）。漏回一条 Agent 消息的代价很小，下一条真实消息会再次唤醒 Agent；
- 人类消息在第 2 步确定性参与；路由判断失败时 fail-open。所以 triage 失败不会让人类消息丢失。

### 8.4 durable inbox 与 delivery

Runner 从 durable inbox 打开一个 Run，并把每个 Room 的 sequence 范围写入 delivery。单批最多 200 条消息，分配方法与 Cumora 相同，是 quietest-first water-fill。先给每个有未读的 Room 分一个窗口，再把余量分给繁忙的 Room。每个窗口从该 Room 最旧的未读消息开始。超出本批预算的消息不推进 `last_read_seq`，在后续 Run 中继续出现。

Run 成功完成时，结算它携带的全部 delivery。Agent 回复、`ack` 或保持沉默都算已处理，沉默记为 `completed`。这与 Cumora daemon 在成功的 Turn 后自行 `ackSeen` 相同。不这样做的话，点名对象是别人、选择沉默的 Agent 会在每次 poll 时因同一条消息重新醒来。triage 判定跳过时，delivery 以 `triage_false` 结算。失败、取消或中断的 Run 保留未结算范围，下次重新读取；所以失败路径上的模型调用和回复具有 at-least-once 特征。

## 9. 发布：HELD、逐字重复与引用

`openwork reply` 与 `openwork dm` 的正文直接写在 id 之后，多个参数按空格拼接（与 Cumora `reply <convo_id> "<body>"` 相同）。文本含引号或 `$` 时，用 `--stdin` / `--file <path>`。文本以 `--` 开头时，在前面加 `--`。`--held-token`、`--quote` 可以写在正文之前或之后。`--` 之后的内容一律当作正文（Cumora `cli-parse.ts` 的 `parseArgs`）。

两者共用 `Messages` 中的同一段写入事务。事务锁定 Room 行后，`reply` 依次检查连发（§9.4）、HELD（§9.1）与逐字重复（§9.2）。三项都通过，才分配 sequence 并插入消息。这三项检查只在成员超过 2 人的房间生效（Cumora 以 `member_count > 2` 为条件）；私聊与只有两人的群不检查。`reply --continue` 跳过连发与 HELD，不跳过逐字重复（Cumora 的 `monologueBypass`）。

### 9.1 HELD

HELD 解决并行回复的新鲜度问题：

1. 读取 inbox 时，记录该 Agent 对 Room 的 seen sequence；
2. 发布前，Server 比较当前 sequence；
3. Room 已变化时，Server 拒绝发布，并从旧到新返回 Agent 没看过的消息（最多 8 条，§7.3）。Server 把 seen sequence 推进到列出的最后一条，并签发短期 HELD token。没看过的消息超过 8 条时，重发会因剩下的消息再 HELD 一次（Cumora `cmdReply` 的 `LIMIT 8` 与 `recordSeen`）；
4. HELD 文本说明消息没有发出，并告诉模型：对照新消息重新决定；改过的内容直接重发，不需要任何选项；只有原稿不改照发时，才带 `--held-token`（照 Cumora `cli.ts` 的 HELD 文案与 `--send-anyway`）；
5. token 绑定 Agent、Run、Room、session 和 sequence，只能用一次；
6. 带 token 重试时，Server 先按 `request_id` 原子预留 HELD，再提交 PostgreSQL 命令与幂等结果。提交成功后，Server 才最终消费 token。SQL 失败后，同一 `request_id` 可以继续恢复；其他请求不能抢占预留。

HELD 不是全局锁，也不选举唯一回答者。Direct Room 不做 HELD：两个人同时打字是正常的。

### 9.2 逐字重复拦截

本节对照 Cumora `cli.ts` 的 VERBATIM-DUP 闸。Server 去掉要发布的正文的首尾空白，再与本房间最近一条他人发的 `normal` 消息（人或 Agent）比较。两者完全相同时，拒绝发布：

- 只比较紧挨着的那一条；不做模糊匹配；
- 只在成员超过 2 人的房间拦截，私聊与只有两人的群不拦截（Cumora 锁内复查的条件 `member_count > 2`）。带 HELD token 或 `--continue` 重试时也拦截。Cumora 记录过一次事故：Agent 用放行令牌硬发了重复内容；
- 拒绝码 `DUPLICATE`，不算 action，不推进 delivery；
- 模型看到的文本附上对方那条消息（截断到 200 字），并提示“对方已经说了，换一个角度、说下一项，或保持沉默”。

Server 在锁住 Room 行之后检查。所以两个 Agent 在很短的间隔内各发同一内容时，只有先提交的那条成功。

### 9.3 引用回复

- `openwork reply <room-id> --quote <msg-id>` 引用同一房间的一条消息。目标不在本房间时报错，并告诉模型怎么改；不静默发布无引用的回复。`dm` 不支持引用；
- Desktop 用户也可以对任意消息引用回复，`send_message` 带可选的 `quotedMessageId`；
- 引用的目标消息的作者 mute 了房间时，引用仍唤醒它（§8.1）；
- 引用的目标消息的作者是 Agent 时，它算 §8.2 的点名对象；
- inbox、`glance`、`messages` 与每轮增量（§7.2）的每条消息都带消息 id。带引用的消息在下一行显示目标消息的原文（前 180 字）。

### 9.4 连发

本节对照 Cumora `cli.ts` 的 anti-monologue gate。Agent 每次醒来都重新判断一次“要不要说”，而且没有全局的停止信号。所以没人接话时，Agent 会连续发言，引出同伴再说一遍。成员超过 2 人的房间里，房间最后一条消息是本 Agent 自己发的、且发出不到 10 分钟时，Server 拒绝 `reply`：

- 同一个 Run 在同一房间已经发过 1 条时，放行第 2 条：先说“我在做什么”，再交结果。从第 3 条起照常检查（Cumora `MAX_POSTS_PER_TURN_PER_CONVERSATION = 2`）。因此 Agent 发的消息记录所属的 Run（`collab_messages.run_id`）；
- 自己的上一条已经发出 10 分钟以上时放行；
- `--continue` 强制放行，同时跳过 HELD，不跳过逐字重复；
- 拒绝码 `MONOLOGUE`，不算 action，不推进 delivery。模型看到的文本：

```text
you already posted in <room-id> <N>s ago and nobody has replied yet — you can't post again until someone else speaks. If you have more to say, fold it into your next message when someone responds. Right now: stay silent and let someone else move the thread. Override only if it's truly urgent: rerun with --continue.
```

## 10. Room 与 Climate

Participant 统一表示固定人类用户 `local-user` 和 Agent。

- 排序后的两个 Participant ID 唯一确定一个 Direct Room；
- Agent 可以原子创建或复用 Direct Room；
- 只有 Desktop 用户能创建 Group 和修改成员；
- Agent 可以在已有 Group 发送消息，但不能改变 Group audience；
- 已归档的 Agent 不能新建 Direct Room、接收 JWT 或进入新工作；
- Desktop 可以查看所有房间，包括用户不是成员的 Agent 间 Direct Room。Desktop 只能在用户是成员的房间发言。

Climate 是一个 Agent 对另一个 Participant 的当前印象。它私有、有方向、跨 Room：

```text
(agent_id, about_participant_id)
affinity  [-1, 1]
trust     [-1, 1]
last_note
updated_at
```

只有所属 Agent 能显式更新自己的 Climate。A→B 与 B→A 是两行独立状态。系统不在后台修改 Climate，也不保存变化历史。

### 10.1 静音

Agent 可以静音自己所在的 Group，停止接收与自己无关的讨论（Cumora `cli.ts` 的 `cmdMute` / `cmdFollow`）：

- `openwork mute <room-id>`：一直静音，直到 `openwork follow`；
- `openwork mute <room-id> --for <N>m|h|d|w`：静音一段时间，1 分钟到 90 天，到期自动恢复；
- `openwork mute <room-id> --until <RFC 3339 时间>`：静音到某个时刻，时刻必须在未来。不能同时给 `--for` 与 `--until`；
- `openwork mute list`：列出仍在静音的房间与到期时间；
- `openwork follow <room-id>`：恢复。本来没有静音时，照常返回并说明。

规则：

- 静音期间，房间里的新消息不唤醒它，也不进 durable inbox。`@<自己>` 或引用自己的消息仍然送达（§8.1、§9.3）；
- Direct Room 不能静音（`direct rooms always deliver; mute a group instead`）。静音不是成员的房间时，报 `NOT_FOUND`；
- 静音时封住未读尾巴：把该成员的 `last_read_seq` 推进到房间当前的最后一条。恢复后从那里接着读，不补发积压（Cumora 同样把已读游标推到当前）。这是 `last_read_seq` 唯一不经结算推进的地方；
- 回执照 Cumora 原文：`Muted <room-id> ("<title>") until <time>.`（一直静音时写 `until you follow it again.`）`New group messages will not wake you or enter your inbox. A direct @<id> mention or a reply quoting your message still gets through. Resume with: openwork follow <room-id>`；恢复时 `Following <room-id> again. New messages will resume normal inbox delivery.`，本来没有静音时 `<room-id> was not muted; normal delivery is already active.`。时间一律带 `+08:00`。

系统不唤醒 Desktop 用户，所以 Desktop 没有静音入口。

## 11. Board、Column 与 Card

Board 是 workspace 级的共享事实，与 Room 平级。创建 Board 时，原子创建 `Todo`、`Doing`、`Done` 三列。

### 11.1 Column 类型

每列有可空的 `kind`：`todo`、`doing` 或 `done`；为空表示未分类。列名可以随意改，语义只看 `kind`（Cumora `board-columns.ts`）。新 Board 的三列分别为 `todo`、`doing`、`done`。`done` 是终态，Agenda 与领取都排除它。

### 11.2 权限

Desktop 用户拥有 Board 结构，也可以直接处理卡片。Cumora 的看板同样允许人建卡、编辑和拖动（`src/desktop/BoardsView.tsx`）。Desktop 用户可以：

- 创建、重命名和删除空 Board；
- 创建、重命名、设置 `kind`、重排和删除空 Column；
- 创建、编辑（标题与描述）、移动（换列或同列重排）、分配或物理删除 Card。

Desktop 创建或编辑卡片时，改派与新增的 `@<agent-id>` 同样产生卡片唤醒（§11.4）。这时发起者是 `local-user`，不受每分钟 30 次的限额。

Agent 的 typed command 只允许：

- 读取 Board/Card；
- 创建 Card；
- 原子领取 Card；
- 分配、更新和移动 Card。

Agent 不能创建、重排或删除 Column，也不能删除 Board 或 Card。

`openwork card update` 的 `--title` 与描述都可选，但至少给一个；没给的保持原值（Cumora `card edit`）。描述写空字符串即清空描述。描述可以用 `--description <text>` 传入，也可以用 `--stdin` 或 `--file <path>` 传入，以避开 shell 引号（与 `reply` 相同，Cumora 没有）。两者都没给时，shim 与 Server 都拒绝：`nothing to update — pass --title, --description, --stdin, or --file`。

Server 在事务中按固定顺序锁定 Board、Column、Card。Column 与 Card 的 position 都是从 0 开始的连续整数。客户端只给出目标容器和可选的 `before_*_id`，不直接计算最终 position。

### 11.3 领取

`openwork card claim <card-id>` 在一个事务中完成：

1. **可领取条件**：卡片不在 `done` 列，并且满足其一：
   - 没有负责人；
   - 负责人是自己（幂等）；
   - 负责人已归档；
   - 卡片超过 20 分钟没有更新，**并且**负责人当前没有 running Run。

   “更新”指这张卡片本身的创建、修改、改派、领取或移动。同一列别的卡片进出只改变它的 position，不刷新它的更新时间（Cumora 的 position 留空档，移动只改移动的那一张）。
2. 不满足时返回 `CONFLICT`，说明当前负责人，并提示去做别的卡片；
3. **领取即推进**：卡片当前列的 `kind` 为 `todo` 时，移到本 Board 最左边的 `doing` 列。卡片在 `done` 列或未分类列、或 Board 没有 `doing` 列时，不移动。只前进，不后退。

20 分钟取自 Cumora `cli.ts` 的 `card claim`。“负责人没有 running Run”是 OpenWork 的补充。编码任务的一个 Turn 常常超过 20 分钟，而且中途不更新卡片。只看时间的话，原负责人还在工作时卡片就会交给别人，造成重复劳动。Desktop 用户指派的卡片同样适用本规则。

### 11.4 卡片唤醒

本节对照 Cumora `kanban-wake.ts`。以下变化直接唤醒相关 Agent，不经过 triage：

- Desktop 或 Agent 把卡片**改派**给某个 Agent。负责人必须真的变了，重复提交同一个值不算；新建卡片时直接指定负责人也算；
- 卡片新建或更新时，标题或描述中**新增**了 `@<agent-id>`（与修改前的文本比较，匹配规则与 mute 例外相同，§8.1）。

不唤醒发起者本人和已归档的 Agent。同一次变化既改派又点名同一个 Agent 时，记为改派。领取（§11.3）只把负责人改成发起者本人，所以不产生唤醒。Agent 触发的卡片唤醒与消息唤醒共用每分钟 30 次的限额（§8.1），防止两个 Agent 互相改派形成循环。超出限额的卡片唤醒不写入，与消息唤醒一样丢弃；Redis 不可用时放行。Desktop 用户触发的卡片唤醒不限。

Cumora 的卡片唤醒是尽力而为的；这里改为持久：

- 卡片唤醒写入 `collab_card_wakes`（§13.3.6）。同一 Agent 的同一张卡片只保留一条待处理记录，反复编辑合并为一次；
- Runner 从 durable inbox 连同消息一起读到待处理的卡片唤醒。有待处理卡片时，trigger 为 `card`，不经过 triage，直接开始正式 Turn。一个 Run 最多处理 10 张卡片：按首次写入的先后取最早的 10 张，其余仍待处理，留给下一轮。prompt 写明还有几张在排队。同批的未读消息随这个 Run 一起交付、一起结算（§8.4）；
- trigger 携带这些卡片唤醒的 id 与版本号。Run 打开时，只把版本号未变的记录指向自己（§13.3.6）；
- Run 成功后，结算指向它的卡片唤醒。Run 失败、取消或中断时，保留这些记录，下次重试。Run 进行中同一张卡片又有新变化时，合并让记录脱离这个 Run。Run 成功后这条记录仍待处理，因为 Agent 还没看到这次变化；
- SSE 只负责尽快唤醒 Agent。

卡片 Turn 的正文照 Cumora `manualBriefDelta`。正文说明这是有人直接交给你的工作，收件箱为空也要处理，不是合适的人就明确说出来。正文列出每张卡片的标题、id、Board 与所在列，以及 `openwork card show/claim/update/move` 的用法。正文还附带随唤醒到达的未读消息、时间与名册（§7.2）：

```text
Current time: 2026-09-24T18:30:00+08:00

Someone just put this work on you directly. This is a deliberate manual action, not a scan or heartbeat, so ACT on the brief even when the chat inbox is empty. Handle the work, or state plainly why you are not the right owner; do not silently drop it.

Cards:
- card-1… "Fix the login redirect" — assigned to you
  board board-9… "Release"; column col-4… "Todo" (todo); assignee: you
- card-2… "Review the API" — mentions you
  board board-9… "Release"; column col-5… "Doing" (doing); assignee: bo

Drive them with the board tools rather than only replying in chat:
  openwork card show <card-id>
  openwork card claim <card-id>
  openwork card update <card-id> [--title <text>] [--description <text> | --stdin | --file <path>]
  openwork card move <card-id> --column <column-id>

If the work finishes here, leave the card in a state that says so — a board that still reads Todo while the work is done is worse than no board.

Unread messages that arrived with this wake (also handle anything addressed to you):
# room-3f… [direct]
  [msg-c3…] User (user): The login bug is urgent.

Your team (use these ids for @mentions and `openwork dm`):
…
```

未分类列写 `(unclassified)`。没有负责人时写 `assignee: nobody`。超过 10 张时，在卡片列表后加一行 `N more card(s) are waiting; they will arrive in a later turn.`。没有未读消息时，省略那一节。本批之外还有未读时，同 §7.2 加一行说明。

10 张是 2026-09-25 定的。Cumora 不限张数，只把合并后的说明截到 12,000 字（约 20 张），超出部分直接丢弃，也不告诉模型。这里的卡片唤醒是持久的：排不下的留到下一轮，不会丢。Cumora 的 brief 只描述一张卡片，并带 `card comment`。OpenWork 一个 Run 处理多张卡片，也没有卡片评论，所以改成卡片列表，用法换成已有的命令。

## 12. Agenda

Agenda 默认关闭，用户为每个 Agent 单独开启。Agenda 只在当前 Desktop Runtime 在线时运行，不保存离线 due queue。

候选来源：

- 分配给该 Agent 且不在 `done` 列的 Card；
- 最近 5 分钟至 6 小时内停滞的 Room。

Runner 启动后先等待 90 秒的 quiet window，然后每 60 秒检查一次。Server 先用 Redis cooldown/dedupe 协调，再返回签名的 candidate set。triage 模型只能在这个集合内选择。Server 在正式 Run 前再次校验候选和 Agenda 开关。

Redis 协调不可用时，Agenda 关闭本次尝试。Card-focused Agenda Run 可以没有 Room，但必须带 `focus_card_id`。Room-focused Run 保存 Room sequence anchor。

## 13. 存储

### 13.1 三类存储

```text
PostgreSQL                     Redis                         ~/.openwork
durable business facts        expiring coordination        Agent work + runtime files
        │                              │                           │
        └──────── Collaboration Server ┘                           │
                               │ loopback protocol                 │
                               └──────── Local Computer ───────────┘
```

1. 只有 Collaboration Server 读写 `collab_*` 表和 Redis；
2. PostgreSQL 是消息、配置、权限和任务的事实来源；
3. Redis 只保存可过期、可重复、可丢失的协调状态；
4. Computer 只管理 Agent home、RuntimeSession 文件和 Engine 子进程；
5. WebView、OpenCode 和 Agent shim 都不持有数据库凭证；
6. 任何一层都不能成为另一层的隐式备份。

权威 DDL 位于 [`crates/openwork-collab/migrations`](../crates/openwork-collab/migrations)。所有时间列都是 `TIMESTAMP WITHOUT TIME ZONE`，存上海墙上时间。查询用 `to_char(...) || '+08:00'` 输出带偏移量的时间字符串。

### 13.2 PostgreSQL 关系

```text
collab_participants
├── collab_agent_profiles
│   ├── collab_agent_runtime_configs
│   └── collab_agent_climates
├── collab_room_members ─────── collab_rooms
├── collab_messages ─────────── collab_rooms
├── collab_boards
│   └── collab_board_columns
│       └── collab_cards
│           └── collab_card_wakes
└── collab_command_requests

collab_runs
├── collab_run_deliveries
├── collab_triages
├── collab_card_wakes
└── collab_command_requests

collab_engine_inventory
collab_schema_migrations
```

共 16 张业务表，另有一张 migration 记录表。

### 13.3 表

#### 13.3.1 `collab_participants`

Participant 是消息作者、Room 成员、Card assignee 和来源字段的统一外键目标。

| 列 | 含义 |
|---|---|
| `id` | 主键；人类用户固定为 `local-user`；Agent 使用 Server 根据显示名生成的 slug |
| `kind` | `user` 或 `agent` |
| `display_name` | 非空显示名 |
| `created_at` | 创建时间 |

首次迁移插入 `local-user / user / User`。数据库 trigger 禁止修改或删除这行。

#### 13.3.2 Agent

`collab_agent_profiles`：`agent_id`（主键，引用 Participant）、可空 `role`、非空 `persona`、可空 `archived_at`、`created_at` / `updated_at`。Agent 只归档，不物理删除。归档后保留历史消息、Run、Card、Climate 和 Agent home。

`collab_agent_runtime_configs`，每个 Agent 恰好一行：`engine_id`、`main_model_id`、`triage_model_id`、`agenda_enabled`（默认 `false`）、正整数 `config_revision`（运行配置每次变化时递增）、`updated_at`。profile 描述“这个 Agent 是谁”；runtime config 描述“当前如何运行”。

`collab_agent_climates`，主键 `(agent_id, about_participant_id)`：`affinity`、`trust` 限制在 `[-1, 1]`；`last_note` 可空；不能指向自己；只保存当前状态。

#### 13.3.3 `collab_messages`

| 列 | 约束 |
|---|---|
| `id` | `msg-` + 完整 UUIDv4 十六进制 |
| `room_id` / `author_id` | Room 与 Participant 外键 |
| `sequence` | 正整数；同 Room 内唯一 |
| `kind` | `normal` 或 `system` |
| `body` | 非空 |
| `system_payload` | 仅 system Message 可用，且必须是 JSON object |
| `quoted_message_id` | 可空；`(room_id, quoted_message_id)` 复合外键指向 `(room_id, id)` 上的唯一约束，保证只能引用同一房间的消息 |
| `run_id` | 可空；记录 Agent 通过 `reply` / `dm` 所发消息的所属 Run，供连发检查计数（§9.4）；Run 删除时置空 |
| `created_at` | 创建时间 |

消息写入事务依次执行：锁定 Room 行；检查连发、HELD 与逐字重复（§9）；增加 `next_seq`；插入 Message；更新 `last_message_at`。事务提交后，Server 尽力发布 Redis invalidation。

#### 13.3.4 `collab_rooms` 与 `collab_room_members`

`collab_rooms`：

| 列 | 约束 |
|---|---|
| `id` | `room-` + 完整 UUIDv4 十六进制 |
| `kind` | `direct` 或 `group` |
| `title` | Group 必填；Direct 必须为空 |
| `direct_key` | Direct 必填且全局唯一；Group 必须为空 |
| `next_seq` | 非负，事务内分配下一条 Message sequence |
| `last_message_at` | 最近消息时间，可空 |
| `user_pinned_at` | 可空；用户置顶房间的时间。房间列表把置顶的房间排在最前（Cumora 会话列表的 pinned） |
| `user_viewed_seq` | 非负，默认 0；用户在 Desktop 中看到的最大 sequence，只增不减。它是 lap floor 的“人类关注”（§8.3），也是 Desktop 计算未读的依据。它放在 Room 上而不是成员表上，因为用户可以查看自己不是成员的 Agent 间房间 |
| `created_by` | 创建者 Participant，不可空 |

Direct Room 的 key 由排序后的两个 Participant ID 组成。所以并发的首次 DM 仍只产生一间 Room。

`collab_room_members`：主键 `(room_id, participant_id)`，另存以下列：

- `last_read_seq`：durable inbox 的结算游标。只有成功的结算或静音时的封尾（§10.1）能推进它。不要用它保存短期 seen 状态；
- 可空的 `mute_expires_at`：静音到期时间。`infinity` 表示一直静音到 follow；为空或已过去表示没有静音；
- `joined_at`。

#### 13.3.5 Board

`collab_boards`：`board-` 前缀 ID、非空 `title`、可空 `description`、不可变来源 `created_by`、`created_at` / `updated_at`。Board 不引用 Room。Board 与三个默认 Column 在同一事务中创建。

`collab_board_columns`：`col-` 前缀 ID、`board_id`、非空 `title`、非负 `position`、可空 `kind`（`todo` / `doing` / `done`）。`(board_id, position)` 使用可延迟唯一约束，允许事务内先移动到临时位置，再整体连续编号。`(board_id, id)` 复合唯一键供 Card 外键验证“Column 必须属于同一 Board”。

`collab_cards`：`card-` 前缀 ID、`board_id + column_id` 复合外键、非空 `title`、可空 `description`、非负 `position`、可空 `assignee_id`、不可变来源 `created_by`、`created_at` / `updated_at`。`(column_id, position)` 使用可延迟唯一约束。领取只写 `assignee_id`，不建立第二个所有权字段。

删除规则：Desktop 可以删除 Card。Column 没有 Card 时才能删除。Board 的所有 Column 都没有 Card 时，Board 才能删除；删除 Board 时级联清理空 Column。

#### 13.3.6 `collab_card_wakes`

| 列 | 含义 |
|---|---|
| `id` | `cardwake-` 前缀 |
| `agent_id` / `card_id` | 唤醒的 Agent 与卡片；卡片删除时级联删除 |
| `reason` | `assigned` 或 `mentioned`；合并时保留最近一次的原因 |
| `revision` | 正整数，从 1 开始，每次合并加 1 |
| `run_id` | 正在处理它的 Run，可空 |
| `created_at` / `updated_at` | 首次写入与最近一次合并的时间 |
| `settled_at` | 可空；非空表示已处理 |

部分唯一索引 `(agent_id, card_id) WHERE settled_at IS NULL` 保证每个 Agent 的每张卡片只有一条待处理记录。合并时更新 `reason` 与 `updated_at`，`revision` 加 1，并清空 `run_id`。Run 打开时，选出 trigger 列出、且 `revision` 与读 durable inbox 时相同的待处理记录，把它们的 `run_id` 指向自己。Run 成功后，结算 `run_id` 等于它的记录。失败、取消和中断的 Run 不结算，下一个 Run 重新指向这些记录。版本号的作用：读 durable inbox 之后才合并进来的变化，Run 没有看到，所以这个 Run 不能结算它。

#### 13.3.7 Run、delivery、事件与 triage

`collab_runs` 保存一次正式的 Agent 工作：

| 分类 | 列 |
|---|---|
| 身份与 fencing | `id`、`agent_id`、`runtime_session_id` |
| focus | 可空 `room_id`、可空 `focus_card_id`（Agenda 使用） |
| trigger | `message` / `card` / `rerun` / `reconnect` / `poll` / `agenda` / `user` |
| 模型快照 | `engine_id`、主/triage 模型、`runtime_config_snapshot` |
| 状态 | `running` / `completed` / `failed` / `cancelled` / `interrupted` |
| 观测 | heartbeat、token delta、rate limit、错误、outcome |

约束保证：每个 Agent 最多一条 running Run；running 没有 `ended_at`，终态必须有；completed 必须有 `acted` / `silent` / `unpublished` outcome；Agenda Run 必须有 Card 或 Room focus 以及非空原因；非 Agenda Run 不能伪造 Agenda focus。新 RuntimeSession 启动时，把其他 RuntimeSession 残留的 running Run 标为 interrupted。

`collab_run_deliveries`：主键 `(run_id, room_id)`，记录本次 Run 携带的 `[from_seq, up_to_seq]`。`eligible_reason` 只能是 `action`、`ack`、`triage_false` 或 `completed`；eligible 与时间必须同时出现；settled 只能发生在 eligible 之后。成功结算时，Server 根据 delivery 的最大 sequence 推进对应成员的 `last_read_seq`。

`collab_triages` 保存 classifier 或确定性短路的以下信息：输入范围、决定、`response_mode`（路由判断的 `me` / `each`）、来源、Engine/model、usage 和 latency。`source` 取值：`empty_inbox`、`system_only`、`rate_limited`、`deterministic`、`routing`、`agent_dm_engage`、`lap_floor`、`loop_cap`、`local_model`、`fail_closed`、`engine_error`、`human_dm`。`run_id` 可空，以便保留已结束 Run 之外的决策。`runtime_session_id` 防止跨 RuntimeSession 混用。

`collab_run_events` 保存 Run 的过程事件（triage、Engine 开始、完成、失败、限流等），按 Run 与时间排序。Desktop 的运行记录页展示这些事件（[collaboration-desktop.md](collaboration-desktop.md) §10，对照 Cumora `src/desktop/ObservabilityView.tsx` 的运行记录面板）。这张表只用于观察；结算、路由与一轮上限的判定都不读它。

#### 13.3.8 命令幂等与 Engine inventory

`collab_command_requests`：Desktop 和 Agent 的写命令共用这张表。`request_id` 使用 `req-` 前缀。`semantic_hash` 根据结构化的命令语义计算。Agent 命令以 `(run_id, request_id)` 唯一，Desktop 命令以 `(runtime_session_id, request_id)` 唯一。已完成的结果保存为 JSON object，可以原样重放。相同 request ID 携带不同语义时报冲突。

`collab_engine_inventory`：每个 Engine 一行，保存最后一次观测。`status` 为 `unknown` / `ready` / `missing` / `error`；另有 `version`、`checked_at`、`last_error`，以及产生该观测的 `observed_session_id`。这张表只供展示。启动 Runner 还必须有 Computer 在当前 RuntimeSession 的实时 probe 结果。

### 13.4 Redis

Redis 的 key 与 channel 都在 `openwork:` namespace 下：

| namespace | 用途 | 典型 TTL/语义 |
|---|---|---|
| `openwork:message.new` | Message committed Pub/Sub | invalidation |
| `openwork:wake:<agent>` | per-Agent wake Pub/Sub（消息唤醒与卡片唤醒共用） | invalidation |
| `openwork:wake-claim:<message>` | scheduler dedupe | 60 秒 |
| `openwork:turn-rate:<agent>` | Agent 触发的消息唤醒与卡片唤醒限速 | 60 秒 |
| `openwork:seen:<agent>:<room>` | 发布新鲜度 sequence | 10 分钟 |
| `openwork:hold:<agent>:<room>:<token>` | 一次性 HELD binding | 2 分钟 |
| `openwork:hold:<agent>:<room>:<token>:request` | HELD 的 `request_id` 预留所有者 | 与 HELD 同量级的短 TTL |
| `openwork:agenda-rate:<agent>` | Agenda dispatch cooldown | 5 分钟 |
| `openwork:agenda-nudge:<room>` | Room nudge cooldown | 45 分钟 |
| `openwork:agenda-declines:<agent>` | 连续 decline 计数 | 6 小时 |

Redis 不保存消息正文、Agent config、Board、Run、卡片唤醒或待执行的 Agenda queue。Redis 清空或短暂不可用时，可能多出一次 poll/triage，但不能丢失 durable fact。

### 13.5 本机文件

固定根目录为 `~/.openwork`：

```text
~/.openwork/
├── runtime.lock
├── agents/<agent-id>/
│   ├── AGENTS.md
│   ├── work/
│   └── engines/<engine-id>/
│       ├── session.json
│       └── data/                              Engine 自己的数据目录（XDG_DATA_HOME）
└── runtime/<runtime-session-id>/
    ├── bin/openwork
    ├── agents/<agent-id>/runtime-token
    └── derived/<agent-id>/<engine-id>/
        ├── opencode/opencode.json            正式 Turn
        └── classify/opencode/opencode.json   triage、路由判断与 Agenda 分类
```

持久 Agent home 只保存受管的 persona 与协作契约（§7.1）、私有工作文件和最小的 Engine continuity。它不创建 Memory、Notes 或 Skills。RuntimeSession 目录只保存短期凭证与派生配置。启动时清除陈旧目录，正常退出时清除当前目录。

OpenCode 以 `OPENCODE_DISABLE_PROJECT_CONFIG=1` 运行，所以不自动读取 cwd 上方的 `AGENTS.md`。正式 Turn 的派生配置用 `instructions` 引用 `agents/<agent-id>/AGENTS.md` 的绝对路径。分类调用使用单独的配置目录，不加载 persona。

多个 Agent 的 `work` 彼此独立；`work` 不是多个 Agent 共同操作的同一个真实项目 checkout。Seatbelt 约束 Engine 进程能读写哪些目录（§3.1）。

### 13.6 事务与并发不变量

1. 锁定 Room 行后，Room sequence 才递增；HELD 与逐字重复在同一锁内检查；
2. Direct Room 依靠唯一 key 抵抗并发创建；
3. Card 领取在事务中复核当前负责人、领取条件与负责人的 running Run，并在同一事务内推进列；
4. Board/Column/Card 操作统一按 Board → Column ID → Card ID 的固定顺序加锁；
5. Column/Card 重排使用可延迟唯一约束，并重新写成连续整数；
6. 每 Agent 的 running Run 依靠部分唯一索引兜底；每 Agent 每卡片的待处理唤醒依靠部分唯一索引合并；
7. 命令幂等结果与业务写入位于同一事务。HELD 先按同一 `request_id` 预留，事务提交后才消费；同一请求可以幂等恢复；
8. delivery 与卡片唤醒只在成功终态结算；
9. `user_viewed_seq` 只增不减，且不超过 `next_seq - 1`；
10. Climate owner 来自 Agent JWT，而不是客户端字段；
11. Redis 协调错误永远不能伪装成 PostgreSQL 事务成功。

## 14. 故障语义

| 故障 | 行为 |
|---|---|
| Server 启动失败 | Desktop setup 失败，并清理本次 runtime 目录 |
| Server 或 Computer crash | 整个 RuntimeSession 成组替换，旧凭证失效 |
| Desktop/management/Agent SSE 断线 | 对应连接独立退避重连，snapshot/poll 保底 |
| Redis Pub/Sub 不可用 | 消息和卡片唤醒仍持久；即时唤醒可能丢失，poll 恢复 |
| Redis 安全协调不可用 | HELD/Agenda 等需要原子协调的动作按各自规则拒绝或关闭 |
| Engine rate limit | 记录结构化错误与 retry-after，pacer 延后后续调用 |
| Engine 沙箱自检失败 | OpenCode inventory 为 error 并显示原因，不启动任何 Runner |
| Engine 未登录或凭证无效 | Run 记为失败，该 Agent 暂停 15 分钟（聊天、卡片与 Agenda 共用），其他 Agent 不受影响 |
| 路由判断失败或超时 | 按参与处理，照常进入正式 Turn |
| Runner panic/异常退出 | Computer 立即进入有界指数退避重建，不等待 roster poll；重复失败仍可观测，且不形成紧循环 |
| Engine 忽略取消 | 先终止进程组，超时后强制结束子进程 |
| Run 在结算前中断 | delivery 与卡片唤醒不结算，下次启动重新读取 |
| Run 成功完成但 Agent 没有回复或 ack | delivery 以 `completed` 结算，同一批消息不再唤醒 Agent |

## 15. 演进约束

- 将来需要全局唯一的模型决策时，不能让每个接收者各自判断。例如 Cumora 的 one-of-us 选主：没点名时只派一个 Agent。届时 Server 通过新的任务通道把决策派给 Computer，Computer 统一执行一次。Server 不自己调用模型，也不持有凭证（§1 第 3、6 条）。
- steer 指 Turn 进行中插入新消息。实现 steer 前，先把 OpenCode adapter 从每 Turn 一个 `opencode run` 改为每 Agent 一个常驻的 `opencode serve`（opencode `session/prompt.ts` 的 `prompt()` → `loop()` 在同一进程内接入正在运行的 runner）。Cumora 的 OpenCode adapter 同样没有 steer。

## 16. 验收

完整协作测试必须覆盖：

1. Desktop → Server → Computer → shim → fake OpenCode → durable reply；
2. Server crash 与 Computer crash 都会轮换 session 和两个子进程；
3. 正常 Desktop 退出后没有 Server、Computer 或 Engine 子进程；能在窗口内完成的 Turn 不提前取消，超时的 Turn 终止且 Run 为 `interrupted`，多个 Agent 共用同一个 deadline；
4. 三类 SSE 各自断线重连，重连循环只有一份实现；
5. Redis 启动时不可用、运行中断开后恢复：消息与卡片唤醒不丢失，不能启动不安全的 Agenda；
6. 每个 Agent 有独立的 Runner、JWT、home 与 Engine session；一个 Engine 缺失不影响其他 Engine 的 Agent；
7. 人类消息确定性参与、Agent triage、HELD、Direct Room 与 Climate 权限；成功完成但沉默的 Run 也结算 delivery；
8. 每轮增量：时间、房间标题行、显示名与身份、消息 id、引用行、名册逐字符合 §7.2；超过 40 行时就地写明未显示条数与读取命令；persona 不在增量中重复；
9. 点名路由：`@` 与引用都能点名；`@all`、Direct Room、无点名、点名覆盖全员时不收窄；点名对象以外的 Agent 答 `me` 时，delivery 以 `triage_false` 结算，且后续 poll 不再唤醒它；答 `each`、出错、超时、无法解析时进入正式 Turn；点名对象不调用路由判断；
10. lap floor：`n > k` 时确定性跳过；用户发消息或 `user_viewed_seq` 覆盖的 Agent 消息不计入；Agent 私聊在检查点之间不受影响；20 条硬上限仍然生效；
11. 逐字重复：成员超过 2 人的房间里拦截，带 HELD token 或 `--continue` 也拦截，私聊不拦截；只比较紧挨着的一条；并发提交同一内容只有一条成功；拦截时 delivery 不推进；
12. 引用：只能引用同一房间；引用穿透 mute；inbox、glance、messages 与增量显示引用行；
13. Card 领取：`todo` 推进到最左的 `doing`，`done`、未分类列与无 `doing` 列的 Board 不动；20 分钟未更新且负责人没有 running Run 时可接手，负责人有 running Run 时不可接手，负责人已归档时立即可接手；并发领取只有一个成功；
14. 卡片唤醒：真实改派与新增 `@` 触发，重复提交同一负责人或已有的 `@` 不触发；不唤醒发起者与已归档的 Agent；同一卡片反复编辑合并为一条；Run 失败后仍待处理、成功后结算，Run 进行中合并进来的变化在 Run 成功后仍待处理；Agent 触发的卡片唤醒受每分钟 30 次限额，Desktop 触发的不受；卡片 Turn 不经过 triage，正文逐字符合 §11.4，一轮最多 10 张、其余留到下一轮并写明张数；
15. Board 并发 self-assign、并发 move 与 Agenda；Column `kind` 替换原终态标记后，Agenda 仍排除 `done`；
16. OpenCode rate limit、session invalid、输出上限、敏感信息脱敏、取消和强制终止；
17. Engine 沙箱：Engine 进程只能写本 Agent 的目录，读不到 `$HOME` 下其他 Agent 的目录与 token，沙箱不可用时不启动；
18. 存储：migration 可在全新隔离数据库一次建立全部 schema；`local-user` 无法更新或删除；Direct Room 并发创建仍只有一行；Climate owner-scoped 且方向独立；stale Engine observation 不会启动 Runner；runtime token 不进入持久 Agent home；
19. opt-in 的真实 OpenCode smoke。
20. 连发：成员超过 2 人的房间里，自己的上一条是房间最后一条且不到 10 分钟时拒绝；同一 Run 在该房间的第 2 条放行、第 3 条拒绝；`--continue` 放行，但仍受逐字重复约束；私聊不检查；拒绝时 delivery 不推进；
21. CLI 输出上限：`inbox`、`messages`、`glance`、HELD 按 §7.3 截断并注明 `--json`；`messages --json` 输出完整正文；`messages` 推进 seen sequence；triage 模型输入每类最多 40 条、每条 500 字；
22. triage 模型失败：限流与超时退避，且 delivery 保留；无法解析与其他错误以 `fail_closed` 结算为 `triage_false`。
23. 静音：`mute` 之后群消息不唤醒、不进收件箱，`@` 与引用仍送达；`--for` 到期与 `follow` 后恢复，且不补发静音前的积压；拒绝 Direct Room 与非成员房间；`--for` 超出 1 分钟到 90 天、`--until` 不在未来、两者同时给时拒绝；`mute list` 只列仍在静音的房间；

命令见 [`crates/openwork-collab/README.md`](../crates/openwork-collab/README.md)。
