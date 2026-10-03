# 协作运行时

本页描述协作模式的运行时与存储：多个持久 Agent 通过房间、消息与共享看板协作。`openwork-collab` 负责 protocol、Collaboration Server 与 Local Computer；Desktop 的 supervisor 在 `desktop/src-tauri/src/collab_client.rs`；Engine 沙箱规则由 `openwork-sandbox::EngineConfinement` 生成。

协作运行时与工作台的 `SessionActor` 运行时彼此独立。界面、Tauri command 与 SSE 到 WebView 的转发见 [collaboration-desktop.md](collaboration-desktop.md)。工作台的权限见 [permissions.md](permissions.md)。命令清单见 [crate README](../../crates/openwork-collab/README.md)。

## 1. 范围、进程与模块

产品范围：

- macOS；单个 Desktop 生命周期；单个逻辑 Collaboration Runtime；本机 OpenCode；
- 多个 Agent，每个 Agent 独立选择 `engine_id`、主模型与 triage 模型；
- Room、Message、Climate、Board、Column、Card、Run 与 Agenda。

不提供：远程 Computer、共享真实项目目录、离线补跑、审批、MCP、Memory、Notes、Skills、Calendar、steer、reaction、convene、投票、文档、卡片评论、Agent 管理群成员。steer 的提议见 [Agent Note：为 steer 改用常驻 opencode serve](../../.agents/notes/proposed/architecture/2026-09-24-opencode-serve-for-steer.md)。

```text
OpenWork Desktop
├── React WebView
├── Tauri host
│   └── CollabDaemonClient                  唯一 supervisor
├── Collaboration Server child              --openwork-collab-server
│   ├── desktop/computer/agent transport
│   ├── RuntimeSession 与认证
│   ├── Room / Message / Climate
│   ├── Board / Agenda / Run
│   └── PostgreSQL + Redis
└── Local Computer child                    --openwork-collab-computer
    ├── desired-state reconcile
    ├── AgentRunner actors
    ├── Agent home
    ├── EngineRegistry
    └── per-Agent OpenCode child processes
```

两个子进程都是 Desktop 可执行文件本身，以第一个参数区分角色。以 `openwork` 为文件名调用时，它是 Agent shim。依赖方向：

```text
desktop/src-tauri → openwork-collab::protocol
server            → protocol + PostgreSQL + Redis
computer          → protocol + Engine process
Agent shim        → protocol
```

约束：

1. Server 是协作业务事实的唯一写者。
2. Computer 不持有数据库或 Redis 凭证。Desktop 启动 Computer 时删除 `DATABASE_URL`、`REDIS_URL` 与全部 `PG*` 环境变量。
3. Server 不创建 Engine 子进程，也不调用模型。Computer 经 Engine adapter 执行全部模型调用。
4. WebView 只调用 Tauri command，不接触 Runtime URL 或凭证。
5. Server 与 Computer 只经 loopback HTTP/SSE DTO 通信。
6. 协作 crate 不依赖 `openwork-core`、`openwork-credentials` 或工作台 Provider adapter。
7. `server` 与 `computer` 不引用对方的实现类型，只共享 `protocol`。

Server 内的各领域模块直接拥有自己的业务 SQL，没有集中式的 Storage、Repository 或 Manager 转发层：

| 模块 | 拥有 |
|---|---|
| `AgentCommands` | 唯一入口 `execute(claims, command)`。编排事务，把领域结果映射成 protocol result；不直接写业务 SQL |
| `Messages` | 消息校验、sequence 推进、消息插入、glance、reply/HELD、逐字重复与连发检查 |
| `Rooms` | Direct Room 创建与复用、成员读取、用户查看记录 |
| `Runs` | active Run、Run inbox、delivery 结算、session 中断 |
| `Board` | Board/Column/Card 结构，Card 领取与移动 |
| `CardWakes` | 卡片唤醒的判定、写入与合并、随 durable inbox 读出、Run 打开时认领、Run 成功后结算 |
| `CommandRequests` | Agent 与 Desktop 命令的幂等 reservation/result ledger |

## 2. 生命周期与 RuntimeSession

Desktop setup 按以下顺序执行：

```text
创建 ~/.openwork（0700）并对 runtime.lock 加 flock
  → 生成 runtime_session_id + Desktop secret + Computer secret
  → 删除 ~/.openwork/runtime/ 下的全部旧目录，创建 runtime/<session-id>
  → 启动 Server，stdin 写一行 bootstrap，stdout 读一行 ready
  → 校验 ready 中的 session id 与随机 loopback 地址
  → 启动 Computer，stdin 写一行 bootstrap，stdout 读一行 ready
  → 等待当前 session 的 Computer heartbeat
  → 建立 Desktop SSE
  → Collaboration ready
```

- `runtime.lock` 保证一个状态目录同一时间只有一个 Desktop supervisor。
- bootstrap 与 ready 各有 64 KiB 上限。
- 每个子进程从启动到 ready 最多等 30 秒；等待 Computer heartbeat 另有 30 秒上限（`STARTUP_TIMEOUT`）。
- 任何一步失败，Desktop 停止已启动的子进程，并删除本次 runtime 目录。

Desktop 持有两个 `Child` handle，每 250 ms 检查一次。Server 或 Computer 意外退出时，Desktop 执行以下步骤：

1. 清空连接。这期间 Desktop command 返回 `Unavailable`。
2. 停止 Desktop SSE。
3. 停止 Computer：先发 SIGTERM，20 秒后 SIGKILL。
4. 停止 Server：先发 SIGTERM，5 秒后 SIGKILL。
5. 删除旧 runtime 目录。
6. 生成全新的 RuntimeSession 与凭证，成组启动新的 Server 与 Computer。启动失败时每 2 秒重试。

**不要只替换一个子进程并复用旧 RuntimeSession。** 旧 Agent JWT、旧 trigger 与旧 Agenda candidate set 都绑定旧的 `runtime_session_id`，新 Server 拒绝它们。理由见 [Agent Note：成组替换 RuntimeSession](../../.agents/notes/implemented/architecture/2026-09-01-runtime-session-group-replacement.md)。

正常退出时，Desktop 按同样的顺序停止两个子进程，整个过程最多等 30 秒。每个 Runner 接收两种信号：

- `stop_requested`：停止 SSE、poll、Agenda 与新 Turn。已经进入 Engine 的 Turn 不受影响。
- `force_cancel`：只在优雅窗口用完时发给仍在运行的 Runner，终止 Engine 进程组。

```text
Computer 收到 SIGTERM
  → 同时向所有 Runner 发出 stop_requested
  → 已在执行的 Turn 自然完成，所有 Runner 共享同一个 15 秒 deadline
  → 到期后向未完成的 Runner 发出 force_cancel，再等 1 秒，然后中止任务
  → Computer 退出（进程内总上限 17 秒）
Server 收到 SIGTERM
  → 停止 HTTP、Redis subscriber 与 scheduler
  → 把当前 session 遗留的 running Run 写为 interrupted（RUNTIME_SESSION_STOPPED）
  → 关闭数据库连接池
Desktop 删除当前 runtime 目录
```

15 秒是 Agent 继续工作的最长窗口。PostgreSQL 与 Redis 是本机基础设施，不属于这个子进程组，Desktop 退出时不停止它们。Desktop 被强制杀死时，不回收它留下的子进程。

## 3. 身份与认证

每个 RuntimeSession 有三种独立身份：

| 调用者 | 凭证 | 允许访问 |
|---|---|---|
| Desktop | session-scoped Desktop secret | `/desktop/*` |
| Computer | session-scoped Computer secret | `/computer/*` |
| AgentRunner / shim | 30 分钟 Agent JWT（HS256） | `/agent/*` |

- Agent JWT 携带 Agent ID、RuntimeSession ID、`scope = agent`、签发时间、过期时间与 `jti`。签名密钥在每个 RuntimeSession 中随机生成，只存在于 Server 内存中。
- Computer 只能为未归档的 Agent 取得 JWT。Runner 在 JWT 剩余不到 5 分钟时换新，并写回 token 文件。
- Server 对每个 Agent 请求重新验证：JWT 有效且属于当前 RuntimeSession；Agent 未归档；写命令属于该 Agent 当前的 running Run；request ID 没有以不同语义用过；目标 Room、Participant、Board 或 Card 满足领域权限。
- Desktop、Computer 与 Agent 的凭证不能跨 namespace 互换。

JWT 限制 Server API 中“哪个 Agent 在哪个 RuntimeSession 做什么”。文件边界由 §3.1 的 Engine 沙箱负责。

### 3.1 Engine 沙箱

Engine 进程指 OpenCode 及它启动的全部子进程，包括 shim。它们在 macOS Seatbelt 下运行。`HomeManager::confinement`（`computer/home.rs`）为每个 Agent 生成一个 `EngineConfinement`：

| 访问 | 放行 | 其余 |
|---|---|---|
| 写 | 本 Agent 的 `agents/<id>/`、`runtime/<session-id>/derived/<id>/`、临时根、可写设备 | 拒绝 |
| 读文件内容 | `$HOME` 之外全部可读。`$HOME` 之内只有：本 Agent 的 `agents/<id>/` 与 `derived/<id>/`、临时根、本 Agent 的 `runtime-token`、`runtime/<session-id>/bin/`、shim 与 OpenCode 可执行文件的真实路径 | `$HOME` 之内其余拒绝，包括其他 Agent 的目录与 token、用户自己的 OpenCode 数据与凭证目录 |
| 网络 | 不限制，Engine 要连模型服务商 | — |

- `$HOME` 之内只拒绝读取内容（`file-read-data`，包括列目录），不拒绝 `stat`。
- Engine 进程的 `HOME` 是 `agents/<id>`，`PATH` 前置 `runtime/<session-id>/bin`。
- 每个 Agent 使用独立的 OpenCode 数据目录 `agents/<id>/engines/opencode/data`（`XDG_DATA_HOME`）。
- 每次启动 OpenCode 前，Computer 在沙箱外读取用户的 `$XDG_DATA_HOME/opencode/auth.json`（默认 `~/.local/share`），经 `OPENCODE_AUTH_CONTENT` 传入。用户重新登录后，下一次启动就使用新的登录信息。
- 登录文件超过 64 KiB（`MAX_AUTH_BYTES`）时，拒绝启动 OpenCode，不截断。
- 旧 session id 在 Agent 的数据目录中不存在时，OpenCode 报 `Session not found`。adapter 把它当作 session 失效，清掉旧 session，开新的 session 重跑一次。
- Computer 启动时做一次 Seatbelt 自检。沙箱不可用时，OpenCode 的 probe 返回错误，inventory 为 `error`，所有 Runner 都不启动。没有无沙箱的运行路径。
- 沙箱挡不住：模型能读到本 Agent 的 JWT 与 Provider 登录信息。OpenCode 必须是 `$HOME` 之外的可执行文件，或单文件可执行文件。依赖 `$HOME` 下解释器的安装方式（例如 nvm 中的 npm 包）不能在沙箱内启动。

理由见 [Agent Note：OpenCode Engine 围栏](../../.agents/notes/implemented/architecture/2026-09-24-opencode-engine-confinement.md)。

## 4. HTTP 与 SSE

Server 只绑定 `127.0.0.1:0`，由操作系统分配端口。非 loopback 地址或固定端口都拒绝启动。所有修改都经 HTTP request，实时通道只用 SSE：

```text
Desktop             1 条 Desktop SSE       /desktop/events
Computer            1 条 management SSE    /computer/events
每个 active Runner  1 条 Agent SSE         /agent/events
```

- Desktop SSE 传 `InvalidationEvent`：`runtime_ready`、`agent_config`、`room`、`message`、`board`、`engine_inventory`、`runner_status`、`agent_activity`。
- management SSE 只让 Computer 重新获取完整的 desired Agent snapshot。
- Agent SSE 只通知对应的 Agent：可能有新工作（`message.new` 或 `card.wake`）。Server 内部按 Agent ID 分发 wake channel，一个 Agent 的事件不送到其他 Agent 的连接。
- SSE decoder 与重连循环只有一份实现：`protocol::sse::reconnecting_invalidation_loop`。三类连接都用指数退避，从 1 秒开始，最多 30 秒；连接保持 60 秒后，退避回到 1 秒。单个未完成事件的上限是 1 MiB。
- Computer 每 60 秒获取一次完整的 desired snapshot。Runner 每 20 秒读一次 durable inbox。
- SSE 与 Redis Pub/Sub 只传 invalidation，不传业务正文。

事件可以重复或丢失。正确性依靠 PostgreSQL 中的事实与周期重读，没有事件重放日志。理由见 [Agent Note：SSE 只传失效提示](../../.agents/notes/implemented/architecture/2026-09-01-sse-invalidation-only.md)。

## 5. Agent desired state 与 reconcile

Server 为每个 Agent 保存：

- Participant identity；
- profile：显示名、role、persona、归档时间；
- runtime config：`engine_id`、主模型、triage 模型、Agenda 开关、`config_revision`。

Agent ID 由 Server 从显示名生成：NFKD 后取 ASCII 字母与数字，小写，用 `-` 连接，最多 24 字符；不以字母开头时加 `a-`；为空时用 `agent`。ID 已被占用时，追加 `-` 与 4 位十六进制，最多尝试 32 次。

`EngineId` 是 protocol 中的强类型值：1–64 字节，小写字母开头，只含小写字母、数字与 `-`。Server 只校验格式，不维护 Engine allowlist。

`EngineRegistry` 是 Computer 内唯一的 adapter 注册表。生产环境只注册 `OpenCodeAdapter`。Computer 对每个 adapter 单独 probe，并按 `engine_id` 上报 inventory。启动时 probe 一次，之后每 5 分钟一次。Engine 未就绪时，使用它的 Agent 记为 error（`Engine <id> is not ready in this RuntimeSession`），使用其他 Engine 的 Agent 不受影响。

Computer 启动时获取全量 snapshot。management invalidation、60 秒的周期读取与 inventory 变化都进入同一个幂等的 `reconcile`：

```text
desired agents + current Engine readiness
  → stop removed or changed Runner
  → prepare persistent home and session runtime files
  → create per-Agent Engine runtime
  → start missing Runner
  → heartbeat observed Runner state to Server
```

- heartbeat 每 30 秒上报一次，状态变化时立即上报。内容是每个 Engine 的就绪状态，以及每个 Runner 的 `running` 或 `error`（附最后一次错误，最多 1,000 字符）。Server 只在内存中保存当前 RuntimeSession 的这份状态。
- §6 与 §8.3 的退避只在 Computer 本地生效，不上报，Desktop 也不显示暂停。导致退避的 Run 照常以失败结算。理由见 [Agent Note：不上报 Runner 暂停](../../.agents/notes/implemented/simplification/2026-09-25-no-runner-pause-reporting.md)。
- assignment 的任何字段变化（显示名、role、persona、Engine、两个模型、Agenda 开关、config revision）时，Computer 停止旧 Runner（同时发出 `stop_requested` 与 `force_cancel`），再按新配置重建。
- 一个 Agent 的 home 或 Engine 初始化失败时，只把该 Runner 标为 error，不阻塞其他 Agent。
- 归档 Agent 时，停止它的 Runner，保留历史、home 与 Engine continuity。恢复 Agent 后，用新的 config revision 重建 Runner。
- Runner 意外退出后，Computer 不等下一次周期读取。它按 1、2、4…秒指数退避重建该 Runner，最长 30 秒。Runner 连续运行 60 秒后，失败次数清零。desired state 中的 Agent 删除、归档或配置变化时，取消旧的重启计划。
- management、heartbeat、roster 或 inventory 后台任务意外停止时，Computer 以错误退出，Desktop 成组替换 RuntimeSession（§2）。

`collab_engine_inventory` 只是最后一次观测。当前 RuntimeSession 能否启动 Runner，只看 Computer 本次 probe 在内存中的结果。

## 6. Engine 与 AgentRunner

Engine seam 分两层（`computer/engine.rs`）：

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

OpenCode adapter：

- probe 用 `/usr/bin/which` 在 PATH 上查找 OpenCode，3 秒超时，不启动 OpenCode。
- 正式 Turn 每次启动一个进程：`opencode run --pure --format json --auto [--session <id>] --model <主模型>`，prompt 经 stdin 写入。cwd 是 `agents/<id>/work`。
- 分类调用（triage、路由题、Agenda）：`opencode run --pure --format json --agent openwork-triage [--model <triage 模型>]`。`openwork-triage` 经 `OPENCODE_CONFIG_CONTENT` 注入，拒绝全部工具。分类调用有 60 秒上限。
- 派生配置以 `OPENCODE_DISABLE_PROJECT_CONFIG=1` 运行，内容是 `permission: {"*": "allow"}`。正式 Turn 的配置用 `instructions` 引用 `agents/<id>/AGENTS.md` 的绝对路径。分类调用使用 `classify/` 下单独的配置，不加载 persona。
- 派生配置把本次使用的模型写成 `provider.<p>.models.<m>.status = "active"`，模型 id 按第一个 `/` 拆分。理由见 [Agent Note：派生配置把模型标为 active](../../.agents/notes/implemented/bug-fix/2026-09-24-opencode-models-pinned-active.md)。
- 输出上限：stdout 8 MiB，stderr 1 MiB，单行 JSONL 1 MiB。错误文本取 stderr 末尾 16 KiB，把 cwd 换成 `<agent-home>`，把 `Bearer `、`token=` 之后的值换成 `<redacted>`。
- session continuity 存在 `agents/<id>/engines/opencode/session.json`，记录 Engine、模型与 `AGENTS.md` 内容摘要。三者之一变化时，不恢复旧 session。

adapter 把错误映射成通用的 `EngineError`：`NotRegistered`、`Missing`、`Unauthenticated`、`RateLimited`、`Process`、`Protocol`、`Reported`、`SessionInvalid`、`Sandbox`、`Io`、`Cancelled`、`Timeout`、`OutputLimit`。

正式 Turn 默认没有“无输出”超时，也没有总时长超时。只有 `EngineRuntimeConfig.turn_timeout` 有值时才启用总超时，Computer 当前传 `None`。用户停止 Agent 或退出 Desktop 时，取消路径先发 SIGINT 给 Engine 进程组，2 秒后 SIGKILL。理由见 [Agent Note：正式 Turn 默认没有超时](../../.agents/notes/implemented/architecture/2026-09-01-no-default-turn-timeout.md)。

每个 `AgentRunner` 是一个 actor：

- 同一 Agent 不并发运行两个正式 Turn；不同 Agent 可以并行。
- 整个 Computer 最多 2 个主模型调用、4 个分类调用同时运行。
- 收到唤醒后等 2.5 秒，合并期间到达的唤醒。busy 时收到的多次唤醒合并为一个 `rerun_requested`。
- Turn 结束后重新读取 durable inbox，不在内存中积累消息正文。
- 运行中的 Run 每 30 秒向 Server 发一次 heartbeat。
- 所有 Engine 调用经同一个 pacer：调用间隔从 250 ms 起；限流时间隔翻倍（最多 10 秒），并按 retry-after 或 60 秒推迟下一次调用。
- 正式 Turn 失败后，`engine_backoff_after`（`computer/scheduling.rs`）决定暂停：`Unauthenticated` 暂停 15 分钟；`RateLimited` 按 retry-after，没有时 60 秒；其他错误不暂停。暂停期间该 Agent 不读 inbox，聊天、卡片 Turn 与 Agenda 都不启动。理由见 [Agent Note：Engine 失败暂停](../../.agents/notes/implemented/feature/2026-09-24-engine-failure-backoff.md)。

## 7. 每轮 Turn 的输入

主模型每轮读到两部分：`AGENTS.md` 中的固定契约，与 prompt 中的增量。理由见 [Agent Note：模型可见的消息上限与原文](../../.agents/notes/implemented/architecture/2026-09-24-model-visible-message-limits.md)。

### 7.1 固定契约

Computer 写入 `agents/<id>/AGENTS.md`（`standing_prompt`，`computer/home.rs`）。它经派生配置的 `instructions` 进入系统提示词。内容依次是：

1. `# Identity`：显示名、id、role（没有时 `unspecified`）与 persona。
2. `# Collaboration contract`：第一句 `You are an OpenWork teammate — a first-class member of this team with your own voice.`；协作动作一律用 `openwork` CLI，assistant 文本本身不会发布；发消息用 `openwork reply <room-id> <text>` 或 `openwork dm <participant-id> <text>`，含引号或 `$` 时用 `--stdin`。
3. `Read the relevant thread and respond appropriately, in your own voice — like a real teammate.`，接着是开头一段与 `## Glance and yield` 五条规则（`GLANCE_AND_YIELD_RULES`）。
4. `## Addressing`：点名同伴用 `@<id>`，不用显示名；回复某条消息时加 `--quote <message-id>`；谈到卡片时写出它的 id（`card-…`）。
5. `## Driving your work`（`DRIVE_YOUR_WORK`）。
6. `# Local workspace` 与 `# CLI discovery`：`openwork --help`，或 `openwork <command> --help`。

规则：

- 这份内容对每个 Agent 固定，不含时间、路径或运行时状态。它的 SHA-256 摘要是 session continuity 的 context fingerprint（§6）。
- 开头一段与五条规则照搬 Cumora `standingPrompt` 与 `glance-protocol.ts` 的 `GLANCE_YIELD_RULES`，只做三处替换：`cumora` 换成 `openwork`；原文“react / 👀”的出路改为保持沉默；共享交付物只有 Card。
- `DRIVE_YOUR_WORK` 照搬 Cumora `standingPrompt`，去掉 “and schedule your own check-back” 与 `calendar create` 示例。

消息里的卡片 id 是房间与看板之间唯一的连接，Server 不把看板事件写进房间。理由见 [Agent Note：卡片链接](../../.agents/notes/implemented/architecture/2026-09-24-card-links-instead-of-board-events.md)。CLI 写法的理由见 [Agent Note：CLI 正文写法与帮助](../../.agents/notes/implemented/feature/2026-09-24-cli-message-body-and-help.md)。

### 7.2 每轮增量

消息 Turn 的 prompt 只包含动态部分，不重复 persona（`message_turn_prompt`，`computer/prompt.rs`）：

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

| 部分 | 规则 |
|---|---|
| 开头 | 固定一段 `WOKEN` |
| 时间 | 当前时间，RFC 3339，`+08:00`，精确到秒 |
| Triage focus | triage 结论的 prompt note；为空时省略这一行 |
| 房间标题行 | `# <room-id> [<direct\|group>] "<title>"`；Direct 没有标题 |
| 消息行 | `[<msg-id>] <显示名> (<user\|agent>): <正文>`；正文空白压成一个空格，截到 600 字符 |
| 引用 | 带引用的消息在下一行写 `↩ quoting [<msg-id>] <显示名>: <原文>`，原文是 Server 截好的前 180 字符；引用行不占行数预算 |
| 行数上限 | 整个 digest 最多 40 条消息行，按 quietest-first 分给各房间，各房间内保留最新的 |
| 未显示的消息 | 就地写明条数与读取命令，`--tail` 取该房间本批的条数 |
| 本批之外还有未读 | digest 之后加一行 `More unread messages are waiting; they will arrive in a later turn.` |
| Climate | 本 Agent 对本批消息作者的私有印象；没有时省略 |
| 名册 | 全部 active 参与者（不含自己与已归档的 Agent）；人类在前，并写明先回答人；Agent 行为 `<id> — <显示名>, <role>` |

Agenda Turn（§12）与卡片 Turn（§11.4）使用同样的时间与名册，正文换成各自的说明。Agenda Turn 的正文是 `Handle this proactive collaboration turn.` 加 Server 给出的 brief。

### 7.3 CLI 列出的消息

shim 渲染 Server 的结果（`computer/shim/render.rs`）：

| 输出 | 每条正文 | 条数 | 换行 |
|---|---|---|---|
| `openwork inbox` | 240 字符 | 本 Run 的 delivery | 显示为 ` \n ` |
| `openwork messages` | 280 字符 | `--tail`，默认 50，范围 1–200 | 显示为 ` \n ` |
| `openwork glance` | 200 字符 | 本 Run 锚点之后别人的消息，最多 50 条；另列出房间成员 | 显示为 ` \n ` |
| HELD | 200 字符 | 最多 8 条 | 空白压成一个空格 |
| 引用行 | 180 字符 | — | 显示为 ` \n ` |

- 列表的每条消息行是 `[<msg-id>] #<sequence> <author-id> @ <room-id>: <正文>`。
- 截断的正文以 `…` 结尾。有正文被截断时，列表最后一行写 ``Long bodies are cut with …; `openwork messages <room-id> --json` prints them in full.``。
- `openwork messages <room-id> [--tail N] --json` 输出完整正文的 JSON。`--json` 只对 `messages` 有效。
- glance 没有新消息时输出 `No new messages since you last read this room (latest sequence N).`。
- 群聊的 `messages` 只列出该 Agent 加入之后的消息；Direct Room 列出全部。
- `inbox`、`glance`、`messages` 与 HELD 都把列出的最大 sequence 记为该 Agent 在该房间的 seen sequence（§9.1）。读过的消息不再触发 HELD。
- 退出码：错误 2，HELD 10，其余 0。

## 8. 消息、唤醒与 triage

### 8.1 写入与唤醒

Server 先把 Message 写入 PostgreSQL，再尽力发布 Redis `openwork:message.new`。发布失败时，不回滚已提交的 Message。消息写入后，Server 清零该房间每个 Agent 成员的 Agenda decline 计数（§12）。

scheduler 收到 `message.new` 后：

1. 用 `openwork:wake-claim:<message>`（60 秒）去重。
2. 选出接收者：房间里除作者外、未归档的 Agent 成员。成员静音了群（§10.1）时，只有 `@<自己>` 与引用自己的消息仍唤醒它。
3. 作者不是 `local-user` 时，每个接收者每分钟最多被唤醒 30 次（`openwork:turn-rate:<接收者>`），超出的唤醒丢弃。Redis 出错时放行。
4. 向 `openwork:wake:<agent>` 发布 wake 事件。

唤醒只是提示。没有收到唤醒的 Agent 也会在约 20 秒一次的 poll 中读到 durable inbox 里的消息。所以“谁不该回答”在 triage 中决定，并随 delivery 结算（§8.2、§8.4）。

### 8.2 点名路由

人类在群里点名部分 Agent 时，其他 Agent 先判断这条消息是不是给自己的，不直接运行主模型。

代码确定点名对象（`addressing`，`server/routing.rs`）：

- 正文中精确的 `@<agent-id>`：`@` 前后都不是 `[A-Za-z0-9_-]`，与静音例外使用同一规则；
- 被引用消息的作者是 Agent 时，该作者。

以下情况本 Agent 直接参与，不出路由题：消息含 `@all`（不区分大小写）；Direct Room；没有点名对象；点名对象覆盖了全部候选；点名对象包含本 Agent。

其余情况，Server 在 triage payload 的 `routing` 中给出路由题，不给结论。题面（`ROUTING_INSTRUCTIONS`）：

```text
You route messages in a team chat where some teammates are AI agents.
Each message below explicitly names one or more agents. Decide whether the messages are aimed at THEM, or at the room.
Answer "me" when the named agents are the ones expected to act or reply — a direct request, an assignment, a question put to them.
Answer "each" when the whole room is still expected to engage — an open question that merely cites someone, a broadcast, a roll call, a request for several independent opinions. If any message is aimed at the room, answer "each".
When you are unsure, answer "each". Waking an extra agent costs tokens; failing to wake the right one loses the message.
Respond ONLY with a single JSON object: {"responseMode": "me"|"each"}.
```

每条消息的输入是 `Named agents: …`、`Other agents in the room: …`（没有时 `(none)`）与正文前 2,000 字符。多条消息之间用 `---` 分隔。

Computer 用本 Agent 的 triage 模型回答，再带 `routed=me|each` 取一次 payload，Server 据此给出最终结论：

- 答 `me`：Server 去掉这些人类消息，用本批其余消息继续 §8.3 的判断。没有其余消息时，结论为 `actionable = false`、`source = routing`，delivery 以 `triage_false` 结算。之后的 poll 不再为这条消息唤醒它，它下次醒来时仍能在房间里读到这条消息。
- 答 `each`：参与（`source = routing`）。
- 模型出错、超时或答案无法解析：按 `each` 处理。只有明确的 `"me"` 才收窄（`parse_route`）。

路由题的答案随最终结论写入 `collab_triages.response_mode`。理由见 [Agent Note：每个 Agent 各自判断点名](../../.agents/notes/implemented/architecture/2026-09-24-per-agent-triage-for-unaddressed-messages.md)。

### 8.3 triage 判定顺序

Server 构造 triage payload（`InboxTriage::payload`，`server/triage.rs`）。只有需要模型时，才交给 Computer 的 triage 模型：

| 顺序 | 条件 | 结果（`source`） | 调用模型 |
|---|---|---|---|
| 1 | 本批只有 system 消息 | 跳过（`system_only`） | 否 |
| 2 | 本批有人类消息，且至少一条需要本 Agent 参与（§8.2） | 参与（`deterministic`） | 否 |
| 2′ | 本批的人类消息全部点名了别人 | §8.2 的路由题 | 是 |
| 3 | 本批只剩 Agent 消息，且每个未读房间的 `n` ≥ 20 | 跳过（`loop_cap`） | 否 |
| 4 | 本批全是 Direct Room 中的 Agent 消息，且每个房间的 `n` 都不是 8 的倍数 | 参与（`agent_dm_engage`） | 否 |
| 5 | 每个未读房间都满足 `n > k` | 跳过（`lap_floor`） | 否 |
| 6 | 其余（群里 Agent 之间的对话、私聊检查点） | triage 模型判断（`local_model`） | 是 |

`n` 是最近一次人类关注之后该房间的非 system Agent 消息数，`k` 是发这些消息的不同 Agent 数。`n > k` 表示有 Agent 开始第二次发言。人类关注取两者中较大的 sequence：最后一条非 Agent 消息；`collab_rooms.user_viewed_seq`（§13.3.4）。理由见 [Agent Note：lap floor](../../.agents/notes/implemented/architecture/2026-09-24-lap-floor.md)。

`reply` 与 `dm` 写入时，也按同样的人类关注检查 20 条硬上限，超过时拒绝（`LOOP_CAP`）。lap floor 只在 triage 时判断。

triage 模型的输入：persona 与 role；本 Agent 对相关作者的私有 Climate；近期上下文（每个 delivery 房间在本批之前、本 Agent 加入之后的最多 12 条）；本批未读。近期上下文与本批未读各取最后 40 条，正文空白压成一个空格、截到 500 字符。模型只返回 `{"actionable", "reason", "promptNote"}`。

triage 模型失败时（`handle_failure`，`computer/runner/classify.rs`）：

- 限流或超时：Run 记为 `failed`（`TRIAGE_RATE_LIMITED` 或 `TRIAGE_ERROR`），保留 delivery。该 Agent 退避，从 30 秒起每次翻倍，最长 480 秒。退避期间不读 inbox。
- 取消：Run 记为 `interrupted`，不退避。
- 输出无法解析或其他 Engine 错误：结论为 `actionable = false`、`source = fail_closed`，原因保留错误的前 120 字符。delivery 以 `triage_false` 结算，不退避。
- 成功的 triage 清零退避。Server 拒绝 actionable 的 `system_only`、`loop_cap`、`lap_floor` 与 `fail_closed`，也拒绝不 actionable 的 `agent_dm_engage`。

人类消息在第 2 步确定性参与，路由题失败时按参与处理。所以 triage 失败不会让人类消息丢失。理由见 [Agent Note：triage 失败处理](../../.agents/notes/implemented/architecture/2026-09-24-triage-failure-handling.md)。

### 8.4 durable inbox 与 delivery

Runner 读 durable inbox（`Messages::inbox`），Server 返回本批消息与一个签名的 trigger（5 分钟有效）。Runner 用 trigger 打开 Run，Server 把每个房间的 sequence 范围写入 `collab_run_deliveries`。

- 单批最多 200 条消息。分配方法是 quietest-first water-fill：先给每个有未读的房间一个窗口，再把余量分给繁忙的房间。每个窗口从该房间最旧的未读消息开始。
- 超出本批预算的消息不推进 `last_read_seq`，在后续 Run 中出现。这时 trigger 的 `carried_over` 为真（§7.2 的最后一行）。

Run 以 `completed` 结束时，Server 把 `eligible_reason` 为空的 delivery 记为 `completed`，然后结算本 Run 的全部 delivery，推进 `last_read_seq`。Agent 回复、`ack` 或保持沉默都算处理过。triage 判定跳过时，delivery 以 `triage_false` 结算。失败、取消或中断的 Run 不结算，下次重新读取。所以失败路径上的模型调用与回复是 at-least-once。理由见 [Agent Note：成功的 Run 结算全部 delivery](../../.agents/notes/implemented/architecture/2026-09-24-settle-deliveries-on-successful-run.md)。

## 9. 发布：连发、HELD、逐字重复与引用

`openwork reply` 与 `openwork dm` 的正文直接写在 id 之后，多个参数按空格拼接。文本含引号或 `$` 时，用 `--stdin` 或 `--file <path>`。以 `--` 开头的正文前面加 `--`。`--held-token`、`--quote` 与 `--continue` 可以写在正文之前或之后，`--` 之后的内容一律当作正文。

`reply` 与 `glance` 只接受本 Run 的 delivery 房间，或 Agenda Run 的焦点房间；其他房间返回 `NOT_FOUND`（`Room is not in the active Run`）。事务锁定房间行后，`reply` 依次检查：

1. 引用目标在本房间（§9.3）。
2. Direct Room 不接受 HELD token。
3. 20 条硬上限（`LOOP_CAP`，§8.3）。
4. 房间成员超过 2 人时：连发（§9.4）、HELD（§9.1）、逐字重复（§9.2）。`--continue` 跳过连发与 HELD，不跳过逐字重复。

全部通过后，Server 分配 sequence、插入消息，并把该房间的 delivery 记为 `action`。`dm` 自动创建或复用 Direct Room，只检查 20 条硬上限。被拒绝的发布不算 action，不推进 delivery。理由见 [Agent Note：发布前的三道闸](../../.agents/notes/implemented/architecture/2026-09-24-reply-gates.md)。

### 9.1 HELD

HELD 处理并行回复的新鲜度：

1. Agent 读到的最大 sequence 记在 Redis `openwork:seen:<agent>:<room>`（10 分钟）。没有记录或 Redis 出错时，用本 Run 的 delivery 锚点。
2. 发布前，Server 查这个 sequence 之后有没有别人的消息。
3. 有时，Server 拒绝发布，从旧到新列出 Agent 没看过的消息（最多 8 条），把 seen 推进到列出的最后一条，并签发 HELD token。没看过的消息超过 8 条时，重发会因剩下的消息再 HELD 一次。
4. HELD 文本说明消息没有发出：读完这些消息，重新决定；改过的内容直接重发，不需要任何选项；只有原稿不改照发时，才带 `--held-token <token>`。
5. token 绑定 Agent、Run、Room、RuntimeSession 与列出的最大 sequence，2 分钟有效，只能用一次。
6. 带 token 重试时，Server 先按 `request_id` 原子预留 token，再提交 PostgreSQL 命令与幂等结果，提交后才消费 token。SQL 失败后，同一 `request_id` 可以恢复；其他请求不能抢占预留。
7. Redis 不可用时，签发与预留都失败，`reply` 返回 `RATE_LIMITED`（`coordination is temporarily unavailable`）。

HELD 不是全局锁，也不选举唯一回答者。理由见 [Agent Note：HELD 后直接重发](../../.agents/notes/implemented/architecture/2026-09-24-held-resend.md)。

### 9.2 逐字重复

Server 去掉正文首尾空白，再与本房间最近一条他人发的 `normal` 消息（人或 Agent）比较。两者相同时拒绝发布：

- 只比较紧挨着的那一条，不做模糊匹配。
- 带 HELD token 或 `--continue` 重试时也检查。
- 拒绝码 `DUPLICATE`。模型看到的文本附上对方那条消息（前 200 字符），并提示换一个角度、说下一项，或保持沉默。

检查发生在房间行锁之后。两个 Agent 几乎同时发同一内容时，只有先提交的那条成功。

### 9.3 引用回复

- `openwork reply <room-id> --quote <msg-id>` 引用同一房间的一条消息。目标不在本房间时返回 `NOT_FOUND`（`<msg-id> is not a message in <room-id>; quote an id from this room's messages`），不发布无引用的回复。`dm` 不支持引用。
- Desktop 用户可以引用任意消息，`SendMessage` 带可选的 `quotedMessageId`。
- 被引用消息的作者静音了房间时，引用仍唤醒它，并进入它的 inbox（§8.1）。
- 被引用消息的作者是 Agent 时，它算 §8.2 的点名对象。
- inbox、`glance`、`messages` 与每轮增量的每条消息都带消息 id；带引用的消息在下一行显示被引用消息的前 180 字符。

理由见 [Agent Note：引用回复](../../.agents/notes/implemented/feature/2026-09-24-message-quotes.md)。

### 9.4 连发

成员超过 2 人的房间里，房间最后一条消息是本 Agent 自己发的，且发出不到 10 分钟时，Server 拒绝 `reply`：

- 同一个 Run 在该房间已经发过 1 条时，放行第 2 条。从第 3 条起照常检查。计数靠 `collab_messages.run_id`。
- 自己的上一条已经发出 10 分钟以上时放行。
- `--continue` 放行，同时跳过 HELD，不跳过逐字重复。
- 拒绝码 `MONOLOGUE`。模型看到的文本：

```text
you already posted in <room-id> <N>s ago and nobody has replied yet — you can't post again until someone else speaks. If you have more to say, fold it into your next message when someone responds. Right now: stay silent and let someone else move the thread. Override only if it's truly urgent: rerun with --continue.
```

## 10. Room 与 Climate

Participant 统一表示固定人类用户 `local-user` 与 Agent。

- 排序后的两个 Participant ID 以 `<left>|<right>` 组成 `direct_key`，唯一确定一个 Direct Room。
- Agent 用 `dm` 原子创建或复用 Direct Room。对方必须是 `local-user` 或未归档的 Agent，不能是自己。
- 只有 Desktop 用户能创建 Group 与修改成员。Agent 能在已加入的 Group 发消息，但不能改变 Group 成员。
- 已归档的 Agent 不能取得 JWT，也不能被 `dm` 或开始新的工作。
- Desktop 能查看所有房间，包括用户不是成员的 Agent 间 Direct Room。Desktop 界面只在用户是成员的房间提供输入框；Server 的 `SendMessage` 不检查 `local-user` 的成员身份。
- 消息正文去掉空白后不能为空，最多 1 MiB，不能含 NUL。

Climate 是一个 Agent 对另一个 Participant 的当前印象。它私有、有方向、跨房间：

```text
(agent_id, about_participant_id)
affinity  [-1, 1]
trust     [-1, 1]
last_note 最多 4,000 字节
updated_at
```

只有所属 Agent 能用 `openwork climate` 更新自己的 Climate，owner 来自 JWT。A→B 与 B→A 是两行独立状态。系统不在后台修改 Climate，也不保存变化历史。

### 10.1 静音

Agent 可以静音自己所在的 Group（`server/room_mutes.rs`）：

- `openwork mute <room-id>`：一直静音，直到 `openwork follow`。
- `openwork mute <room-id> --for <N>m|h|d|w`：静音 1 分钟到 90 天，到期自动恢复。
- `openwork mute <room-id> --until <RFC 3339 时间>`：时间必须在未来。不能同时给 `--for` 与 `--until`。
- `openwork mute list`：列出仍在静音的房间与到期时间。
- `openwork follow <room-id>`：恢复。本来没有静音时，照常返回并说明。

规则：

- 静音时，Server 把该成员的 `last_read_seq` 推进到房间当前的最后一条。恢复后从那里接着读，不补发静音前的积压。这是 `last_read_seq` 唯一不经结算推进的地方。
- 静音期间，群里的新消息不唤醒它，也不进 durable inbox。
- 有 `@<自己>` 或引用自己的消息时，它被唤醒；这条消息连同静音以来的全部未读一起进入 inbox。
- Direct Room 不能静音（`INVALID_ARGUMENT`：`direct rooms always deliver; mute a group instead`）。静音不是成员的房间时，返回 `NOT_FOUND`（`you are not a member of <room-id>`）。
- 回执：`Muted <room-id> ("<title>") until <time>.`（一直静音时写 `until you follow it again.`）`New group messages will not wake you or enter your inbox. A direct @<id> mention or a reply quoting your message still gets through. Resume with: openwork follow <room-id>`。恢复时：`Following <room-id> again. New messages will resume normal inbox delivery.`；本来没有静音时：`<room-id> was not muted; normal delivery is already active.`。时间带 `+08:00`。
- Desktop 没有静音入口。

理由见 [Agent Note：静音](../../.agents/notes/implemented/feature/2026-09-25-room-mutes.md)。

## 11. Board、Column 与 Card

Board 是 workspace 级的共享事实，与 Room 平级。创建 Board 时，同一事务创建 `Todo`、`Doing`、`Done` 三列。

### 11.1 Column 类型

每列有可空的 `kind`：`todo`、`doing` 或 `done`；为空表示未分类。列名可以随意改，语义只看 `kind`。新 Board 的三列分别为 `todo`、`doing`、`done`。`done` 列中的卡片不进 Agenda 候选，也不能领取。

### 11.2 权限

Desktop 用户可以：

- 创建、重命名与删除空 Board；
- 创建、重命名、设置 `kind`、重排与删除空 Column；
- 创建、编辑（标题与描述）、移动（换列或同列重排）、分配与删除 Card。

Desktop 创建或编辑卡片时，改派与新增的 `@<agent-id>` 同样产生卡片唤醒（§11.4）。发起者是 `local-user`，不受每分钟 30 次的限额。理由见 [Agent Note：Desktop 直接处理卡片](../../.agents/notes/implemented/feature/2026-09-25-desktop-card-editing.md)。

Agent 的 typed command 只允许：读取 Board 与 Card；创建 Card；原子领取 Card；分配、更新与移动 Card。Agent 不能创建、重排或删除 Column，也不能删除 Board 或 Card。

`openwork card update` 的 `--title` 与描述都可选，但至少给一个，没给的保持原值。描述写空字符串即清空。描述只能有一个来源：`--description <text>`、`--stdin` 或 `--file <path>`。两者都没给时，shim 与 Server 都拒绝：`nothing to update — pass --title, --description, --stdin, or --file`。

Server 在事务中按固定顺序锁定 Board、Column、Card。Column 与 Card 的 position 都是从 0 开始的连续整数。客户端只给出目标容器与可选的 `before_*_id`，不计算最终 position。

### 11.3 领取

`openwork card claim <card-id>`（`Board::claim_card_in`，`server/board/claim.rs`）在一个事务中完成：

1. 卡片在 `done` 列时，返回 `CONFLICT`：`card <id> is in a done column; it is finished, so pick another card.`
2. 满足以下任一条件时可以领取：没有负责人；负责人是自己（幂等）；负责人已归档；卡片超过 20 分钟没有更新，并且负责人当前没有 running Run。负责人状态在卡片行锁内读取。
3. 不满足时返回 `CONFLICT`：`card <id> is already being worked by @<holder> — move on to another card.`
4. 领取即推进：卡片当前列的 `kind` 为 `todo` 时，移到本 Board 最左的 `doing` 列末尾。卡片在未分类列，或 Board 没有 `doing` 列时，不移动。

“更新”指这张卡片本身的创建、修改、改派、领取或移动。同列其他卡片进出只改变它的 position，不刷新它的 `updated_at`。“负责人没有 running Run”按 Agent 判断，不按卡片判断。理由见 [Agent Note：Column 类型与卡片领取](../../.agents/notes/implemented/architecture/2026-09-24-column-kind-and-card-claim.md)。

### 11.4 卡片唤醒

以下变化唤醒相关 Agent，不经过 triage（`CardWakes::targets_in`）：

- 卡片改派给某个 Agent。负责人必须真的变了，重复提交同一个值不算；新建卡片时直接指定负责人也算。
- 卡片新建或更新时，标题或描述中新增了 `@<agent-id>`（与修改前的文本比较，匹配规则同 §8.2）。

规则：

- 不唤醒发起者本人与已归档的 Agent。同一次变化既改派又点名同一个 Agent 时，记为改派（`assigned`）。领取只把负责人改成发起者本人，所以不产生唤醒。
- Agent 触发的卡片唤醒与消息唤醒共用每个接收者每分钟 30 次的限额（`openwork:turn-rate:<接收者>`）。超出的唤醒不写入；Redis 出错时放行。Desktop 触发的不限。
- 卡片唤醒写入 `collab_card_wakes`（§13.3.6）。同一 Agent 的同一张卡片只保留一条待处理记录，反复编辑合并为一条。写入后发布 `openwork:wake:<agent>`；发布失败时，Agent 在下一次 poll 读到它。
- Runner 读 durable inbox 时一并读到待处理的卡片唤醒。有待处理记录时，trigger 为 `card`，Runner 不经过 triage，直接开始正式 Turn。
- 一个卡片 Turn 最多处理 10 张卡片：按首次写入时间（相同时按 id）取最早的 10 张，其余留到下一轮。同批的未读消息随这个 Run 一起交付、一起结算（§8.4）。
- trigger 携带这些记录的 id 与版本号。Run 打开时，只认领版本号未变的记录。
- Run 成功后，结算它认领的记录。Run 失败、取消或中断时，记录保留，下一个 Run 重新认领。Run 进行中同一张卡片又有变化时，合并让记录脱离这个 Run；Run 成功后这条记录仍待处理。

卡片 Turn 的正文（`card_turn_prompt`，`computer/prompt.rs`）：

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

- 未分类列写 `(unclassified)`。没有负责人时写 `assignee: nobody`。
- 超过 10 张时，卡片列表后加一行 `N more card(s) are waiting; they will arrive in a later turn.`。
- 没有未读消息时，省略那一节。本批之外还有未读时，加一行 `More unread messages are waiting; they will arrive in a later turn.`。

理由见 [Agent Note：卡片唤醒持久化](../../.agents/notes/implemented/architecture/2026-09-24-persistent-card-wakes.md)。

## 12. Agenda

Agenda 默认关闭，用户为每个 Agent 单独开启。Agenda 只在当前 Desktop Runtime 在线时运行，不保存离线 due queue。

候选（`server/agenda.rs`），合计最多 20 个：

- 分配给该 Agent、不在 `done` 列的 Card，按 `updated_at` 从旧到新；
- 该 Agent 是成员、最后一条消息在 5 分钟到 6 小时之前的 Room，附最近 8 条消息。

流程：

1. Runner 没有消息或卡片工作，且距上一次 Turn（或启动）安静满 90 秒时，最多每 60 秒检查一次。
2. Server 返回签名的 candidate set（5 分钟有效）与分类题。Agenda 关闭，或该 Agent 连续 decline 已达 3 次时，候选为空。
3. Computer 用 triage 模型选择 act 或 decline。模型只能在 candidate set 内选择。
4. decline：计数加 1（`openwork:agenda-declines:<agent>`，6 小时）。
5. act：Server 重新校验候选与 Agenda 开关，再占用 `openwork:agenda-rate:<agent>`（5 分钟）。Room 候选还要占用 `openwork:agenda-nudge:<room>`（45 分钟）。占用失败时不开 Run。成功后清零 decline 计数，返回签名的 `agenda` trigger 与 brief。
6. Run 打开前，Server 复核：卡片仍分配给该 Agent 且不在 `done` 列；房间的 `next_seq` 仍等于候选时的值。

- Redis 协调不可用时，Agenda payload 请求失败，本次不运行。
- Card-focused Agenda Run 可以没有 Room，但必须带 `focus_card_id`。Room-focused Run 保存 `agenda_anchor_seq`。
- 取 payload、分类或提交决定失败时，Agenda 退避：从 60 秒起每次翻倍，最长 15 分钟。

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

1. 只有 Collaboration Server 读写 `collab_*` 表与 Redis。
2. PostgreSQL 是消息、配置、权限与任务的事实来源。
3. Redis 只保存可过期、可重复、可丢失的协调状态。
4. Computer 只管理 Agent home、RuntimeSession 文件与 Engine 子进程。
5. WebView、OpenCode 与 Agent shim 都不持有数据库凭证。
6. 任何一层都不能成为另一层的隐式备份。

权威 DDL 在 [`crates/openwork-collab/migrations`](../../crates/openwork-collab/migrations)。所有时间列都是 `TIMESTAMP WITHOUT TIME ZONE`，存上海墙上时间。查询用 `to_char(...) || '+08:00'` 输出带偏移量的时间字符串。

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
├── collab_run_events
├── collab_triages
├── collab_card_wakes
└── collab_command_requests

collab_engine_inventory
collab_schema_migrations
```

共 17 张业务表，另有一张 migration 记录表。首个迁移删除当前 schema 中其他全部 `collab_*` 表，再建立新 schema。

### 13.3 表

#### 13.3.1 `collab_participants`

Participant 是消息作者、Room 成员、Card assignee 与来源字段的统一外键目标。

| 列 | 约束 |
|---|---|
| `id` | 主键；`local-user`，或 `^[a-z][a-z0-9-]{0,47}$` |
| `kind` | `user` 或 `agent` |
| `display_name` | 去掉空白后非空 |
| `created_at` | 创建时间 |

首个迁移插入 `local-user / user / User`。数据库 trigger 禁止修改或删除这一行。

#### 13.3.2 Agent

`collab_agent_profiles`：`agent_id`（主键，引用 Participant）、可空 `role`、非空 `persona`、可空 `archived_at`、`created_at` / `updated_at`。Agent 只归档，不物理删除。归档后保留历史消息、Run、Card、Climate 与 Agent home。

`collab_agent_runtime_configs`，每个 Agent 恰好一行：`engine_id`、`main_model_id`、`triage_model_id`、`agenda_enabled`（默认 `false`）、正整数 `config_revision`（运行配置每次变化时递增）、`updated_at`。

`collab_agent_climates`，主键 `(agent_id, about_participant_id)`：`affinity` 与 `trust` 限制在 `[-1, 1]`，默认 0；`last_note` 可空；不能指向自己；只保存当前状态。

#### 13.3.3 `collab_messages`

| 列 | 约束 |
|---|---|
| `id` | `^msg-[0-9a-f]{32}$` |
| `room_id` / `author_id` | Room 与 Participant 外键 |
| `sequence` | 正整数；`(room_id, sequence)` 唯一 |
| `kind` | `normal` 或 `system` |
| `body` | 去掉空白后非空 |
| `system_payload` | 只有 system Message 可用，且必须是 JSON object |
| `quoted_message_id` | 可空；复合外键 `(room_id, quoted_message_id)` 指向 `(room_id, id)` 上的唯一约束，只能引用同一房间的消息 |
| `run_id` | 可空；Agent 经 `reply` / `dm` 发的消息记录所属 Run，供连发检查计数（§9.4）；Run 删除时置空 |
| `created_at` | 创建时间 |

消息写入事务依次执行：锁定房间行；检查 §9 的各项；`next_seq` 加 1 并以新值作为 sequence；插入 Message；更新 `last_message_at`。事务提交后，Server 尽力发布 Redis invalidation。

#### 13.3.4 `collab_rooms` 与 `collab_room_members`

`collab_rooms`：

| 列 | 约束 |
|---|---|
| `id` | `^room-[0-9a-f]{32}$` |
| `kind` | `direct` 或 `group` |
| `title` | Group 必填；Direct 必须为空 |
| `direct_key` | Direct 必填且全局唯一；Group 必须为空 |
| `next_seq` | 非负，默认 0；等于房间已分配的最后一个 sequence |
| `last_message_at` | 最近消息时间，可空 |
| `user_pinned_at` | 可空；用户置顶房间的时间，房间列表把置顶的房间排在最前 |
| `user_viewed_seq` | 非负，默认 0；用户在 Desktop 中看到的最大 sequence，只增不减，不超过 `next_seq`。它是 lap floor 的人类关注（§8.3），也是 Desktop 计算未读的依据 |
| `created_by` | 创建者 Participant |
| `created_at` / `updated_at` | 创建与最近修改时间 |

`collab_room_members`：主键 `(room_id, participant_id)`，另有：

- `last_read_seq`：durable inbox 的结算游标。只有成功的结算与静音时的封尾（§10.1）推进它。它不保存短期 seen 状态。
- `mute_expires_at`：可空。`infinity` 表示一直静音到 follow；为空或已过去表示没有静音。
- `joined_at`。

#### 13.3.5 Board

`collab_boards`：`^board-[0-9a-f]{32}$` ID、非空 `title`、可空 `description`、不可变来源 `created_by`、`created_at` / `updated_at`。Board 不引用 Room。

`collab_board_columns`：`^col-[0-9a-f]{32}$` ID、`board_id`、非空 `title`、非负 `position`、可空 `kind`（`todo` / `doing` / `done`）。`(board_id, position)` 使用可延迟唯一约束，允许事务内先移到临时位置，再整体连续编号。`(board_id, id)` 复合唯一键供 Card 外键验证“Column 属于同一 Board”。

`collab_cards`：`^card-[0-9a-f]{32}$` ID、`board_id + column_id` 复合外键、非空 `title`、可空 `description`、非负 `position`、可空 `assignee_id`、不可变来源 `created_by`、`created_at` / `updated_at`。`(column_id, position)` 使用可延迟唯一约束。领取只写 `assignee_id`，没有第二个所有权字段。

删除规则：Desktop 可以删除 Card。Column 没有 Card 时才能删除。Board 的所有 Column 都没有 Card 时才能删除；删除 Board 时级联删除空 Column。

#### 13.3.6 `collab_card_wakes`

| 列 | 约束 |
|---|---|
| `id` | `^cardwake-[0-9a-f]{32}$` |
| `agent_id` / `card_id` | 唤醒的 Agent 与卡片；卡片删除时级联删除 |
| `reason` | `assigned` 或 `mentioned`；合并时保留最近一次的原因 |
| `revision` | 正整数，从 1 开始，每次合并加 1 |
| `run_id` | 正在处理它的 Run，可空；Run 删除时置空 |
| `created_at` / `updated_at` | 首次写入与最近一次合并的时间 |
| `settled_at` | 可空；非空表示已处理 |

部分唯一索引 `uq_collab_card_wakes_pending (agent_id, card_id) WHERE settled_at IS NULL` 保证每个 Agent 的每张卡片只有一条待处理记录。合并时更新 `reason` 与 `updated_at`，`revision` 加 1，并清空 `run_id`。Run 打开时，选出 trigger 列出、且 `revision` 与读 inbox 时相同的待处理记录，把 `run_id` 指向自己。Run 成功后，结算 `run_id` 等于它的记录。

#### 13.3.7 Run、delivery、事件与 triage

`collab_runs` 保存一次正式的 Agent 工作：

| 分类 | 列 |
|---|---|
| 身份与 fencing | `id`、`agent_id`、`runtime_session_id` |
| focus | 可空 `room_id`、可空 `focus_card_id`、`agenda_anchor_seq`、`trigger_reason`（Agenda 使用） |
| trigger | CHECK 允许 `message` / `card` / `rerun` / `reconnect` / `poll` / `agenda` / `user`；代码只写 `message`、`card`、`agenda` |
| 模型快照 | `engine_id`、主/triage 模型、`runtime_config_snapshot`（config revision、Engine、两个模型与 persona） |
| 状态 | `running` / `completed` / `failed` / `cancelled` / `interrupted` |
| 观测 | `heartbeat_at`、token 用量、`rate_limit_percent`、`error_code` / `error_message`、`outcome`、`inbox_carried_over` |

约束：

- 部分唯一索引保证每个 Agent 最多一条 running Run。
- running 没有 `ended_at`，终态必须有。
- completed 必须有 `outcome`：有动作为 `acted`，沉默为 `silent`，有文本但没发布为 `unpublished`；其他状态的 `outcome` 为空。
- Agenda Run 必须有 Card 或 Room focus，以及非空原因。非 Agenda Run 的 `focus_card_id`、`agenda_anchor_seq` 与 `trigger_reason` 必须为空。
- Server 启动时，把其他 RuntimeSession 残留的 running Run 标为 `interrupted`（`RUNTIME_SESSION_REPLACED`）。

`collab_run_deliveries`：主键 `(run_id, room_id)`，记录 `[from_seq, up_to_seq]`。`eligible_reason` 只能是 `action`、`ack`、`triage_false` 或 `completed`。`eligible_reason` 与 `eligible_at` 同时出现；`settled_at` 只能在 eligible 之后。成功结算时，`last_read_seq` 推进到 `up_to_seq`。

`collab_triages` 保存 triage 结论：每个 Run 在每个 delivery 房间写一行，结论写入后不能改为相反的值。列包括 `up_to_seq`、`actionable`、`response_mode`（`me` / `each`；CHECK 还允许 `one_of_us`，代码不写它）、`source`、`reason`、`prompt_note`、Engine 与模型、用量与 latency。`source` 的 CHECK 允许 12 个值：`empty_inbox`、`system_only`、`rate_limited`、`deterministic`、`routing`、`agent_dm_engage`、`lap_floor`、`loop_cap`、`local_model`、`fail_closed`、`engine_error`、`human_dm`。Server 只接受其中 8 个：`local_model`、`deterministic`、`system_only`、`agent_dm_engage`、`loop_cap`、`lap_floor`、`routing`、`fail_closed`。`run_id` 可空，Run 删除时置空。

`collab_run_events` 保存 Run 的过程事件：`source` 为 `runner` 或 `engine`；`kind` 只有 `triage.started`、`engine.started`、`engine.completed`、`engine.failed`、`engine.cancelled`；`level`；JSON object `data`。限流信息写在 `engine.failed` 的 `data` 中。Desktop 的运行记录页展示这些事件（[collaboration-desktop.md](collaboration-desktop.md) §10）。结算、路由与一轮上限的判定都不读这张表。理由见 [Agent Note：运行记录页](../../.agents/notes/implemented/feature/2026-09-24-run-records-page.md)。

#### 13.3.8 命令幂等与 Engine inventory

`collab_command_requests`：Desktop 与 Agent 的写命令共用这张表。`request_id` 为 `^req-[0-9a-f]{32}$`。`semantic_hash` 为结构化命令语义的 `sha256:` 摘要。Agent 命令以 `(run_id, request_id)` 唯一，Desktop 命令以 `(runtime_session_id, request_id)` 唯一。已完成的结果保存为 JSON object，原样重放。相同 request ID 携带不同语义时，返回冲突。

`collab_engine_inventory`：每个 Engine 一行，保存最后一次观测。`status` 为 `unknown` / `ready` / `missing` / `error`，`error` 时 `last_error` 必填；另有 `version`、`checked_at` 与 `observed_session_id`。这张表只供展示。

### 13.4 Redis

Redis 的 key 与 channel 都在 `openwork:` namespace 下：

| key / channel | 用途 | TTL / 语义 |
|---|---|---|
| `openwork:message.new` | Message 提交后的 Pub/Sub | invalidation |
| `openwork:wake:<agent>` | 每 Agent 的 wake Pub/Sub，消息唤醒与卡片唤醒共用 | invalidation |
| `openwork:wake-claim:<message>` | scheduler 去重 | 60 秒 |
| `openwork:turn-rate:<agent>` | 该 Agent 被 Agent 触发的消息唤醒与卡片唤醒计数，上限 30 | 60 秒 |
| `openwork:seen:<agent>:<room>` | 发布新鲜度 sequence | 10 分钟 |
| `openwork:hold:<agent>:<room>:<sha256(token)>` | 一次性 HELD binding | 2 分钟 |
| `openwork:hold:<agent>:<room>:<sha256(token)>:request` | HELD 的 `request_id` 预留 | 2 分钟 |
| `openwork:agenda-rate:<agent>` | Agenda dispatch cooldown | 5 分钟 |
| `openwork:agenda-nudge:<room>` | Room nudge cooldown | 45 分钟 |
| `openwork:agenda-declines:<agent>` | 连续 decline 计数，达到 3 时停止给候选 | 6 小时 |

Redis 不保存消息正文、Agent config、Board、Run、卡片唤醒或待执行的 Agenda queue。每条 Redis 命令有 2 秒上限。Redis 清空或短暂不可用时，可能多出一次 poll 或 triage，但不丢失 durable fact。

### 13.5 本机文件

固定根目录是 `~/.openwork`。目录权限为 0700，token 与受管文件为 0600，写入用临时文件加 rename：

```text
~/.openwork/
├── runtime.lock
├── agents/<agent-id>/
│   ├── AGENTS.md
│   ├── work/                                  正式 Turn 与分类调用的 cwd
│   └── engines/<engine-id>/
│       ├── session.json
│       └── data/                              Engine 自己的数据目录（XDG_DATA_HOME）
└── runtime/<runtime-session-id>/
    ├── bin/openwork                           指向 Desktop 可执行文件的 symlink
    ├── agents/<agent-id>/runtime-token
    └── derived/<agent-id>/<engine-id>/
        ├── opencode/opencode.json             正式 Turn（XDG_CONFIG_HOME）
        ├── classify/opencode/opencode.json    triage、路由题与 Agenda 分类
        ├── cache/                             XDG_CACHE_HOME
        └── state/                             XDG_STATE_HOME
```

- 持久 Agent home 只保存受管的契约（§7.1）、私有工作文件与最小的 Engine continuity。它不创建 Memory、Notes 或 Skills。
- RuntimeSession 目录只保存短期凭证与派生配置。Desktop 启动时删除陈旧目录，正常退出时删除当前目录。
- 多个 Agent 的 `work` 彼此独立，不是多个 Agent 共同操作的真实项目 checkout。Seatbelt 约束 Engine 进程能读写哪些目录（§3.1）。

### 13.6 事务与并发不变量

1. 锁定房间行后，房间 sequence 才递增。连发、HELD 与逐字重复在同一锁内检查。
2. Direct Room 依靠唯一 `direct_key` 抵抗并发创建。
3. Card 领取在事务中复核当前负责人、领取条件与负责人的 running Run，并在同一事务内推进列。
4. Board/Column/Card 操作统一按 Board → Column ID → Card ID 的顺序加锁。
5. Column/Card 重排使用可延迟唯一约束，并重新写成连续整数。
6. 每 Agent 的 running Run 由部分唯一索引兜底；每 Agent 每卡片的待处理唤醒由部分唯一索引合并。
7. 命令幂等结果与业务写入在同一事务。HELD 先按同一 `request_id` 预留，事务提交后才消费；同一请求可以幂等恢复。
8. delivery 与卡片唤醒只在 Run 成功时结算。
9. `user_viewed_seq` 只增不减，且不超过 `next_seq`。
10. Climate owner 来自 Agent JWT，不来自客户端字段。
11. Redis 协调错误不能伪装成 PostgreSQL 事务成功。

## 14. 故障语义

| 故障 | 行为 |
|---|---|
| Server 启动失败 | Desktop setup 失败，清理本次 runtime 目录 |
| Server 或 Computer 意外退出 | 整个 RuntimeSession 成组替换，旧凭证失效（§2） |
| Desktop/management/Agent SSE 断线 | 对应连接独立退避重连，snapshot 与 poll 保底 |
| Redis Pub/Sub 不可用 | 消息与卡片唤醒仍持久；即时唤醒可能丢失，poll 恢复 |
| Redis 协调不可用 | HELD 签发与预留返回 `RATE_LIMITED`；Agenda payload 失败；限速与 seen 记录放行 |
| Engine rate limit | Run 失败（`ENGINE_RATE_LIMITED`），pacer 推迟后续调用，该 Agent 暂停 retry-after 或 60 秒 |
| Engine 沙箱自检失败 | OpenCode inventory 为 `error` 并显示原因，不启动任何 Runner |
| Engine 未登录或凭证无效 | Run 失败，该 Agent 暂停 15 分钟（聊天、卡片与 Agenda 共用），其他 Agent 不受影响 |
| 路由题失败或超时 | 按参与处理，照常进入后续判断 |
| triage 模型失败 | 限流与超时退避并保留 delivery；其他错误 `fail_closed`（§8.3） |
| Runner panic 或意外退出 | Computer 立即进入有界指数退避重建，不等周期读取 |
| Engine 忽略取消 | 先 SIGINT 进程组，2 秒后 SIGKILL |
| Run 在结算前中断 | delivery 与卡片唤醒不结算，下次重新读取 |
| Run 成功但 Agent 没有回复或 ack | delivery 以 `completed` 结算，同一批消息不再唤醒 Agent |

## 15. 验收

编号沿用原设计文档 §16，代码与测试注释按这些编号引用。测试路径相对 `crates/`；`desktop/src-tauri` 下的测试写出相对仓库根的路径。

带 Postgres 的测试需要 `TEST_DATABASE_URL`，没有时静默跳过。Redis 默认用 `redis://127.0.0.1:6379/15`，可用 `TEST_REDIS_URL` 覆盖。

1. Desktop → Server → Computer → shim → fake OpenCode → durable reply。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`；`openwork-collab/tests/runtime_e2e.rs::desktop_server_computer_and_fake_opencode_settle_a_reply`
2. Server crash 与 Computer crash 都会轮换 session 与两个子进程。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`；`openwork-collab/tests/runtime_protocol.rs::runtime_scopes_credentials_runs_typed_commands_and_rejects_old_sessions`
3. 正常 Desktop 退出后没有 Server、Computer 或 Engine 子进程；能在窗口内完成的 Turn 不提前取消，超时的 Turn 被终止且 Run 为 `interrupted`，多个 Agent 共用同一个 deadline。
   - 测试：`desktop/src-tauri/tests/collab_supervisor.rs::desktop_supervises_and_replaces_the_complete_runtime_process_group`；`openwork-collab/src/computer/daemon.rs::shutdown_gives_active_work_a_grace_period`；`openwork-collab/src/computer/daemon.rs::shutdown_force_cancels_only_after_the_shared_deadline`；`desktop/src-tauri/src/collab_client.rs::bounded_stop_force_kills_a_child_that_ignores_sigterm`；`openwork-collab/tests/runtime_protocol.rs::runtime_scopes_credentials_runs_typed_commands_and_rejects_old_sessions`
   - 缺口：退出后只检查 Server 与 Computer 进程，不检查 Engine 进程。`interrupted` 只在 Server 关闭的路径上断言（测试直接插入 running Run），没有端到端的超时 Turn。
4. 三类 SSE 各自断线重连，重连循环只有一份实现。
   - 测试：`openwork-collab/src/computer/client.rs::management_and_agent_sse_reconnect_independently_after_disconnects`；`desktop/src-tauri/src/collab_client.rs::desktop_sse_reconnects_after_the_stream_closes`；`openwork-collab/tests/architecture.rs::all_sse_consumers_share_the_protocol_reconnect_loop`；`openwork-collab/tests/sse_decoder.rs::decoder_rejects_an_unbounded_event_without_a_delimiter`
5. Redis 启动时不可用、运行中断开后恢复：消息与卡片唤醒不丢失，不能启动不安全的 Agenda。
   - 测试：`openwork-collab/tests/resilience.rs::redis_disconnect_cannot_erase_a_durable_message_or_start_agenda`；`openwork-collab/tests/resilience.rs::redis_subscriber_recovers_after_a_live_connection_is_cut`（`#[ignore]`，需要 `TEST_REDIS_URL`）
   - 缺口：Redis 不可用时卡片唤醒的持久性没有测试。
6. 每个 Agent 有独立的 Runner、JWT、home 与 Engine session；一个 Engine 缺失不影响其他 Engine 的 Agent。
   - 测试：`openwork-collab/tests/engine_registry.rs::registry_creates_one_stateful_runtime_per_agent`；`openwork-collab/tests/runtime_reconcile.rs::desired_state_reconciles_without_restart_and_keeps_persistent_home`；`openwork-collab/src/computer/daemon.rs::engine_readiness_is_scoped_per_assignment`；`openwork-collab/src/computer/home.rs::separates_persistent_agent_home_from_session_runtime_files`
7. 人类消息确定性参与、Agent triage、HELD、Direct Room 与 Climate 权限；成功完成但沉默的 Run 也结算 delivery。
   - 测试：`openwork-collab/tests/messaging.rs::failed_run_keeps_the_durable_delivery_and_human_triage_is_deterministic`；`openwork-collab/tests/messaging.rs::a_completed_silent_run_settles_its_delivery_so_the_agent_is_not_woken_again`；`openwork-collab/tests/messaging.rs::direct_room_reads_and_private_directional_climate_form_one_loop`；`openwork-collab/tests/posting.rs::concurrent_group_replies_hold_one_agent_until_a_single_reconsideration`；`openwork-collab/src/server/triage.rs::group_uses_model_triage_until_the_deterministic_agent_loop_cap`
8. 每轮增量：时间、房间标题行、显示名与身份、消息 id、引用行、名册逐字符合 §7.2；超过 40 行时就地写明未显示条数与读取命令；persona 不在增量中重复。
   - 测试：`openwork-collab/src/computer/prompt.rs::acc_08_message_turn_prompt_renders_the_documented_delta`；`openwork-collab/src/computer/prompt.rs::acc_08_digest_over_forty_lines_names_what_it_left_out`；`openwork-collab/tests/messaging.rs::acc_08_inbox_carries_room_headers_and_the_active_team`；`openwork-collab/src/computer/home.rs::acc_08_standing_contract_names_the_addressing_rules`；`openwork-collab/src/computer/home.rs::acc_08_standing_contract_carries_the_teammate_voice_and_drive_rules`
9. 点名路由：`@` 与引用都能点名；`@all`、Direct Room、无点名、点名覆盖全员时不收窄；点名对象以外的 Agent 答 `me` 时，delivery 以 `triage_false` 结算，且后续 poll 不再唤醒它；答 `each`、出错、超时、无法解析时参与；点名对象不答路由题。
   - 测试：`openwork-collab/src/server/routing.rs::acc_09_only_messages_naming_other_agents_are_routed`；`openwork-collab/src/computer/triage.rs::acc_09_only_an_explicit_me_narrows_the_route`；`openwork-collab/tests/messaging.rs::acc_09_a_message_naming_one_agent_asks_the_others_to_route_it`
   - 缺口：路由题的模型出错或超时按参与处理（`computer/runner/routing.rs`）没有测试，只测了答案解析。
10. lap floor：`n > k` 时确定性跳过；用户发消息或 `user_viewed_seq` 覆盖的 Agent 消息不计入；Agent 私聊在检查点之间不受影响；20 条硬上限仍然生效。
    - 测试：`openwork-collab/src/server/triage.rs::acc_10_a_lapping_agent_run_is_skipped_without_a_model`；`openwork-collab/tests/messaging.rs::acc_10_a_second_lap_is_skipped_until_the_user_looks_again`；`openwork-collab/src/server/triage.rs::direct_check_cadence_and_hard_cap_are_independent`；`openwork-collab/tests/messaging.rs::group_agent_chatter_stops_at_the_deterministic_loop_cap`
11. 逐字重复：成员超过 2 人的房间里拦截，带 HELD token 或 `--continue` 也拦截，私聊不拦截；只比较紧挨着的一条；并发提交同一内容只有一条成功；拦截时 delivery 不推进。
    - 测试：`openwork-collab/tests/posting.rs::acc_11_a_verbatim_repeat_of_the_last_peer_message_is_rejected`；`openwork-collab/tests/posting.rs::acc_11_concurrent_identical_group_posts_publish_only_once`
12. 引用：只能引用同一房间；引用穿透静音；inbox、glance、messages 与增量显示引用行。
    - 测试：`openwork-collab/tests/messaging.rs::acc_12_quotes_stay_in_the_room_and_reach_a_muted_author`；`openwork-collab/src/computer/shim/render.rs::acc_12_message_listings_show_ids_and_quoted_originals`；`openwork-collab/src/computer/prompt.rs::acc_12_quoted_messages_show_the_original_under_the_reply`；`openwork-collab/src/computer/shim/parse.rs::acc_12_reply_takes_a_quote_anywhere_outside_the_body`；`openwork-collab/tests/room_page.rs::acc_12_room_snapshot_carries_authors_quotes_and_notes`
    - 缺口：唤醒路径（`Messages::wake_recipients`）的引用例外没有独立测试，只测了 inbox 路径。
13. Card 领取：`todo` 推进到最左的 `doing`；`done` 列拒绝领取；未分类列与无 `doing` 列的 Board 不移动；20 分钟未更新且负责人没有 running Run 时可接手，负责人有 running Run 时不可接手，负责人已归档时立即可接手；并发领取只有一个成功。
    - 测试：`openwork-collab/src/server/board/claim.rs::acc_13_claim_moves_only_from_todo_to_the_leftmost_doing`；`openwork-collab/src/server/board/claim.rs::acc_13_takeover_needs_an_archived_holder_or_an_idle_card_without_a_running_run`；`openwork-collab/tests/board_agenda.rs::acc_13_claim_advances_todo_and_takes_over_only_archived_or_idle_stale_work`；`openwork-collab/tests/board_agenda.rs::acc_15_concurrent_claim_and_move_keep_one_owner_and_contiguous_positions`
14. 卡片唤醒：真实改派与新增 `@` 触发，重复提交同一负责人或已有的 `@` 不触发；不唤醒发起者与已归档的 Agent；同一卡片反复编辑合并为一条；Run 失败后仍待处理、成功后结算，Run 进行中合并进来的变化在 Run 成功后仍待处理；Agent 触发的卡片唤醒受每分钟 30 次限额，Desktop 触发的不受；卡片 Turn 不经过 triage，正文逐字符合 §11.4，一轮最多 10 张、其余留到下一轮并写明张数。
    - 测试：`openwork-collab/src/server/card_wakes.rs::acc_14_only_real_reassignments_and_new_mentions_wake`；`openwork-collab/src/server/card_wakes.rs::acc_14_the_initiator_and_inactive_agents_are_not_woken`；`openwork-collab/tests/card_wakes.rs::acc_14_real_changes_wake_once_and_merge_until_a_successful_run_settles_them`；`openwork-collab/tests/card_wakes.rs::acc_14_agent_card_wakes_are_rate_limited_and_a_turn_carries_at_most_ten`；`openwork-collab/tests/desktop_cards.rs::acc_14_desktop_creates_edits_and_moves_cards_and_wakes_who_it_names`；`openwork-collab/src/computer/prompt.rs::acc_14_card_turn_prompt_lists_the_cards_and_the_board_commands`；`openwork-collab/src/computer/prompt.rs::card_turn_prompt_counts_waiting_cards_and_names_unclassified_columns_and_missing_assignees`；`openwork-collab/tests/runtime_e2e.rs::desktop_card_assignment_runs_a_card_turn_that_skips_triage`
15. Board 并发 self-assign、并发 move 与 Agenda；Agenda 只排除 `done` 列。
    - 测试：`openwork-collab/tests/board_agenda.rs::acc_15_concurrent_claim_and_move_keep_one_owner_and_contiguous_positions`；`openwork-collab/tests/board_agenda.rs::acc_15_agenda_is_opt_in_excludes_only_done_columns_and_opens_a_card_focused_run`
16. OpenCode rate limit、session invalid、输出上限、敏感信息脱敏、取消与强制终止。
    - 测试：`openwork-collab/tests/opencode_adapter.rs::reported_rate_limit_terminates_a_still_running_opencode_process`；`openwork-collab/tests/opencode_adapter.rs::stderr_rate_limit_is_mapped_inside_the_opencode_adapter`；`openwork-collab/tests/opencode_adapter.rs::a_stale_session_reported_only_on_stderr_starts_a_fresh_session`；`openwork-collab/tests/opencode_adapter.rs::oversized_jsonl_line_is_stopped_without_unbounded_buffering`；`openwork-collab/tests/opencode_adapter.rs::process_errors_redact_tokens_and_the_agent_home`；`openwork-collab/tests/opencode_adapter.rs::cancellation_terminates_the_entire_opencode_process_group`
    - 缺口：`reported_rate_limit_terminates_a_still_running_opencode_process` 已知不稳定，偶尔失败。
17. Engine 沙箱：Engine 进程只能写本 Agent 的目录，读不到 `$HOME` 下其他 Agent 的目录与 token，沙箱不可用时不启动。
    - 测试：`openwork-collab/src/computer/home.rs::acc_10_an_agent_engine_cannot_reach_another_agent_or_the_user_home`；`openwork-collab/tests/opencode_adapter.rs::unavailable_sandbox_fails_closed_without_starting_opencode`；`openwork-sandbox/tests/engine_confinement.rs::engine_cannot_read_or_list_another_agent_or_other_home_files`；`openwork-sandbox/tests/engine_confinement.rs::engine_cannot_write_outside_its_writable_roots`
18. 存储：migration 能在全新隔离数据库一次建立全部 schema；`local-user` 不能修改或删除；Direct Room 并发创建只有一行；Climate 只归所属 Agent 且方向独立；陈旧的 Engine 观测不会启动 Runner；runtime token 不进入持久 Agent home。
    - 测试：`openwork-collab/tests/runtime_protocol.rs::runtime_scopes_credentials_runs_typed_commands_and_rejects_old_sessions`；`openwork-collab/tests/messaging.rs::direct_room_reads_and_private_directional_climate_form_one_loop`；`openwork-collab/tests/runtime_reconcile.rs::desired_state_reconciles_without_restart_and_keeps_persistent_home`；`openwork-collab/src/computer/home.rs::separates_persistent_agent_home_from_session_runtime_files`；`openwork-collab/tests/architecture.rs::persistent_home_stays_separate_from_runtime_credentials`
    - 缺口：只断言了 `local-user` 不能修改，没有断言不能删除。
19. opt-in 的真实 OpenCode smoke。
    - 状态：手动。设置 `OPENWORK_REAL_OPENCODE_SMOKE=1`，运行 `openwork-collab/tests/runtime_e2e.rs::desktop_server_computer_and_real_opencode_smoke`（`#[ignore]`，会发起真实模型请求）。
20. 连发：成员超过 2 人的房间里，自己的上一条是房间最后一条且不到 10 分钟时拒绝；同一 Run 在该房间的第 2 条放行、第 3 条拒绝；`--continue` 放行，但仍受逐字重复约束；私聊不检查；拒绝时 delivery 不推进。
    - 测试：`openwork-collab/tests/posting.rs::acc_20_an_agent_cannot_post_twice_in_a_row_until_someone_else_speaks`；`openwork-collab/src/computer/shim/parse.rs::acc_20_reply_takes_continue_anywhere_outside_the_body`
21. CLI 输出上限：`inbox`、`messages`、`glance`、HELD 按 §7.3 截断并注明 `--json`；`messages --json` 输出完整正文；`messages` 推进 seen sequence；triage 模型输入每类最多 40 条、每条 500 字符。
    - 测试：`openwork-collab/src/computer/shim/render.rs::acc_21_listings_cut_long_bodies_like_cumora`；`openwork-collab/src/computer/shim/render.rs::acc_21_messages_json_prints_full_bodies`；`openwork-collab/src/computer/shim/parse.rs::acc_21_only_messages_takes_json`；`openwork-collab/tests/posting.rs::acc_21_held_lists_eight_messages_and_listing_counts_as_seen`；`openwork-collab/src/server/triage.rs::acc_21_triage_input_keeps_the_latest_forty_messages_cut_to_500_chars`
22. triage 模型失败：限流与超时退避，且 delivery 保留；无法解析与其他错误以 `fail_closed` 结算为 `triage_false`。
    - 测试：`openwork-collab/src/computer/runner/classify.rs::acc_22_triage_failures_back_off_or_fail_closed_like_cumora`；`openwork-collab/tests/posting.rs::acc_22_a_failed_triage_model_fails_closed_for_agent_only_messages`
23. 静音：`mute` 之后群消息不唤醒、不进收件箱，`@` 与引用仍送达；`--for` 到期与 `follow` 后恢复，且不补发静音前的积压；拒绝 Direct Room 与非成员房间；`--for` 超出 1 分钟到 90 天、`--until` 不在未来、两者同时给时拒绝；`mute list` 只列仍在静音的房间。
    - 测试：`openwork-collab/tests/mutes.rs::acc_23_a_muted_group_only_delivers_mentions_and_follow_skips_the_backlog`；`openwork-collab/tests/mutes.rs::acc_23_direct_rooms_foreign_rooms_and_bad_spans_are_rejected`；`openwork-collab/src/computer/shim/render.rs::acc_23_mute_receipts_follow_cumora`；`openwork-collab/src/computer/shim/parse.rs::acc_23_mute_follow_and_list_parse_like_cumora`
