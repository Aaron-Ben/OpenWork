# 本机 BYOA 协作运行时

协作模式让多个持久 Agent 通过房间、消息和共享看板协作。它与工作台的 `SessionActor` 运行时彼此独立：协作事实由 Collaboration Server 管理，模型调用由本机 Computer 中的 Engine adapter 执行。

本文是协作运行时与存储的唯一权威：业务语义、协调规则、PostgreSQL/Redis/本机文件的所有权与约束、故障语义和验收。Desktop 的界面、supervisor 与 Tauri command 见 [collaboration-desktop.md](collaboration-desktop.md)；Engine 沙箱的权限背景见 [permissions.md](permissions.md)。

当前产品范围固定为：

- macOS；
- 单个 Desktop 生命周期；
- 单个逻辑 Collaboration Runtime；
- 本机 OpenCode；
- 多个 Agent，每个 Agent 独立选择 `engine_id`、主模型和 triage 模型；
- Room、Message、Climate、Board、Column、Card、Run 和 Agenda。

当前不提供远程 Computer、共享真实项目目录、离线补跑、审批、MCP、Memory、Notes、Skills、Calendar、steer、reaction、convene、投票、文档或 Agent 管理群成员。未来接入 Codex 时新增真实 Engine adapter，不改变 Server 业务模型。

协作机制对照 Cumora BYOA 的已提交源码（`/Volumes/Extreme SSD/Code/cumora/server/src/agents/`）。Cumora 的协作靠五层：谁被唤醒（§8.2）、醒了要不要动用主模型（§8.3）、主模型读房间（§7）、服务端仲裁（§9、§11.3）、防循环（§8.3）。本文在每处注明对应的 Cumora 文件与偏离之处。

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

必须保持：

1. Server 是协作业务事实的唯一写者；
2. Computer 不持有数据库或 Redis 凭证；
3. Server 不创建 Engine 子进程，也不调用模型；所有模型调用都在 Computer 中经 Engine adapter 执行；
4. WebView 只调用 Tauri command，不接触 Runtime URL 或凭证；
5. Server 与 Computer 只通过 loopback HTTP/SSE DTO 通信；
6. 协作 crate 不依赖 `openwork-core`、`openwork-credentials` 或工作台 Provider adapter；
7. `server` 与 `computer` 不直接引用对方的实现类型，只共享 `protocol`。

Server 内的业务 SQL 由领域模块直接拥有，不设集中式 Storage、Repository 或 Manager 转发层：

| 模块 | 拥有 |
|---|---|
| `AgentCommands` | 唯一入口 `execute(claims, command)`：事务编排和把领域结果映射成 protocol result，不直接写业务 SQL |
| `Messages` | 消息校验、sequence 推进、消息插入、glance、reply/HELD、逐字重复拦截、DM 消息流程 |
| `Rooms` | Direct Room 创建/复用、成员读取、用户查看记录 |
| `Runs` | active Run、Run inbox、delivery 与卡片唤醒的结算、session interruption |
| `Board` | Board/Column/Card 结构与 Card 领取、移动、卡片唤醒的写入 |
| `CommandRequests` | Agent 与 Desktop 命令的幂等 reservation/result ledger |

## 2. 生命周期与 RuntimeSession

Desktop setup 执行以下顺序：

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

`runtime.lock` 保证同一状态目录同时只有一个 Desktop supervisor。子进程 bootstrap 和 ready 都有 64 KiB 上限，启动总等待上限为 30 秒。

Desktop 持有两个 `Child` handle，每 250 ms 检查一次。如果 Server 或 Computer 意外退出：

1. 暂停 Desktop command；
2. 停止旧 Computer、全部 Runner 和 Engine；
3. 停止旧 Server；
4. 删除旧 runtime 目录；
5. 生成全新的 RuntimeSession 与凭证；
6. 成组启动新的 Server 与 Computer。

不允许只替换一个子进程后复用旧 session。旧 Agent JWT 与旧 trigger 都绑定旧 `runtime_session_id`，新 Server 会拒绝它们。

正常退出是两阶段有界关闭。每个 Runner 有两个不同的信号：`stop_requested` 停止 SSE、poll、Agenda 和新 Turn，已经进入 Engine 的 Turn 不接收它；`force_cancel` 只在优雅窗口耗尽时发给正在运行的 Engine。

```text
同时向所有 Runner 发出 stop_requested
  → 已在执行的 Turn 自然完成，所有 Runner 共享同一个 15 秒 deadline
  → 到期后向未完成的 Runner 发出 force_cancel，终止 Engine 进程组
  → Computer 退出
  → Server 停止 HTTP，把当前 session 遗留的 running Run 写为 interrupted
  → Server 停止 SSE、Redis tasks 与数据库连接池
  → 删除当前 runtime 目录
```

15 秒是 Agent 可以继续工作的最长窗口，之后只允许有界的进程回收和 Server 状态落盘。PostgreSQL 和 Redis 是本机基础设施，不属于这个子进程组，Desktop 退出不会停止它们。Desktop 自身被强制杀死后的孤儿回收不在当前范围。

## 3. 身份与认证

每次 RuntimeSession 都有三种独立身份：

| 调用者 | 凭证 | 允许访问 |
|---|---|---|
| Desktop | session-scoped Desktop secret | `/desktop/*` |
| Computer | session-scoped Computer secret | `/computer/*` |
| AgentRunner / shim | 30 分钟 Agent JWT | `/agent/*` |

Agent JWT 至少携带 Agent ID、RuntimeSession ID 和过期时间。Computer 只能为当前 session 中未归档的 Agent 获取 JWT。Server 在每次 Agent 请求上重新验证：

- JWT 有效且属于当前 RuntimeSession；
- Agent 仍处于 active 状态；
- 写命令属于当前 active Run；
- request ID 尚未以不同语义使用；
- 目标 Room、Participant、Board 或 Card 满足领域权限。

Desktop、Computer、Agent credential 不能跨 namespace 互换。JWT 负责限制 Server API 中“以哪个 Agent、哪个 RuntimeSession 做什么”；文件边界由 §3.1 的 Engine 沙箱负责。

### 3.1 Engine 沙箱

Engine 进程（OpenCode 及它启动的全部子进程，包括 shim）在 macOS Seatbelt 下运行，规则由 `openwork-sandbox::EngineConfinement` 生成：

| 访问 | 放行 | 其余 |
|---|---|---|
| 写 | 本 Agent 的 `agents/<id>/`、`runtime/<session-id>/derived/<id>/`、临时目录、可写设备 | 拒绝 |
| 读文件内容 | `$HOME` 之外全部可读；`$HOME` 之内只有本 Agent 的 `agents/<id>/`、`derived/<id>/`、本 Agent 的 `runtime-token`、`runtime/<session-id>/bin/`、shim 与 Engine 可执行文件 | `$HOME` 之内其余拒绝，包括其他 Agent 的目录与 token、用户自己的 OpenCode 数据和凭证目录 |
| 网络 | 放行：Engine 自己要连模型服务商 | — |

- `$HOME` 内只拒绝读取**内容**（`file-read-data`，含列目录），不拒绝 `stat`，否则解析 Agent 目录的上级路径会失败。
- 每个 Agent 使用独立的 OpenCode 数据目录 `agents/<id>/engines/opencode/data`（`XDG_DATA_HOME`）。登录信息由 Computer 在沙箱外、每次启动 OpenCode 前读取用户的 `opencode/auth.json`，经 `OPENCODE_AUTH_CONTENT` 传入，因此用户重新登录后下一次 Turn 即生效。登录文件超过 64 KiB 时拒绝启动 OpenCode：环境变量与 argv 共用 macOS 的 1 MiB `ARG_MAX`，不截断。旧 session id 在新数据目录中不存在时，OpenCode 报 `Session not found`，按 session 失效处理，自动开新会话。
- Computer 启动时做一次沙箱自检。不可用时 OpenCode 的 inventory 为 error，任何 Runner 都不启动，不退回无沙箱运行。
- 沙箱挡不住的：模型能看到本 Agent 的 JWT 与 Provider 登录信息，因为它们必须进入同一个进程树；OpenCode 必须是 `$HOME` 之外、或单文件的可执行文件（Homebrew、官方安装脚本），依赖 `$HOME` 下解释器的安装方式（如 nvm 里的 npm 包）无法在沙箱内启动。

## 4. HTTP 与 SSE seam

Server 只绑定操作系统分配的随机 loopback 端口。所有修改都是 HTTP request；实时通道只使用 SSE。

```text
Desktop             1 条 Desktop SSE
Computer            1 条 management SSE
每个 active Runner  1 条 Agent SSE
```

- Desktop SSE 通知 Runtime、Agent config、Engine inventory、Runner status 或消息可能变化；
- management SSE 只促使 Computer 重新获取完整 desired Agent snapshot。新 Agent 还没有 Runner 和 JWT，所以它不能被每 Agent 连接取代；
- Agent SSE 只通知对应 Agent 可能有新工作。每条 Agent SSE 在网络上独立，Server 内部也按 Agent ID 使用独立 wake channel，Alpha 的事件不会先广播给所有 Runner 再过滤；
- SSE decoder 和重连循环只有一份实现，位于 `protocol::sse`。三类连接都使用 1 秒起步、最多 30 秒的指数退避，对单个未完成事件设置 1 MiB 上限；Desktop 不引用 `computer::*` 的实现模块；
- Computer 仍每 60 秒获取完整 desired snapshot；Agent 仍约每 20 秒重新读取 durable inbox；
- SSE 和 Redis Pub/Sub 只传 invalidation，不传业务正文。

事件可以重复或丢失。正确性来自 PostgreSQL 中的 canonical state 和周期重读，不建立事件重放日志。

## 5. Agent desired state 与 reconcile

Server 保存每个 Agent 的：

- Participant identity；
- profile：显示名、role、persona、归档时间；
- runtime config：`engine_id`、主模型、triage 模型、Agenda 开关、`config_revision`。

`EngineId` 是 protocol 中的强类型值，Server 只校验格式，不维护 Engine allowlist。`EngineRegistry` 是 Computer 内唯一的 Adapter 注册表；Computer 对每个 Adapter 独立 probe 并按 `engine_id` 上报 inventory，一个 Engine 缺失不影响使用其他 Engine 的 Agent。当前生产只注册 OpenCode，加入 Codex 时只新增并注册真实 Adapter，不预留占位实现。

Computer 启动时获取全量 snapshot；management invalidation 和 60 秒周期 fallback 也进入同一个幂等 `reconcile`：

```text
desired agents + current Engine readiness
  → stop removed or changed Runner
  → prepare persistent home and session runtime files
  → create per-Agent Engine runtime
  → start missing Runner
  → heartbeat observed Runner state to Server
```

heartbeat 上报每个 Runner 的当前状态：`running`、`paused`（附原因与恢复时间，来自 §6 的退避）或 `error`（附最后一次错误）。Server 只在内存中保存当前 session 的这份状态，供 Desktop 显示。

Engine、主模型、triage 模型、persona 或 config revision 变化都会重建对应 Runner。一个 Agent 的 home 或 Engine 初始化失败只把该 Runner 标记为 error，不阻塞其他 Agent。归档停止 Runner 并保留历史、home 与 Engine continuity；恢复后用新的 config revision 重建。

Runner 异常退出后，Computer 不等待下一次 60 秒全量 reconcile：它立即按 1、2、4 秒指数退避重建该 Agent 的 Runner，最长退避 30 秒；Runner 连续稳定 60 秒后清零失败次数。desired state 已删除、归档或配置已变化时，旧重启计划随之取消。management、heartbeat、roster 或 inventory 后台任务意外停止则视为 daemon 故障，而不是留下一个表面在线但不再协调的 Computer。

PostgreSQL 中的 Engine inventory 只是最后一次观测。当前 session 能否启动 Runner，必须由 Computer 本次实时 probe 的内存结果确认。

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

生产 `EngineRegistry` 当前只注册 `OpenCodeAdapter`。OpenCode 每个 Turn 启动一次 `opencode run --session <id>`。派生配置把本 Agent 的主模型与判断模型写成 `provider.<p>.models.<m>.status = "active"`：OpenCode 刷新模型目录后会删除标为 deprecated 的模型（opencode `provider/provider.ts`），配置里的状态覆盖目录里的状态，目录中没有的模型也由这个条目创建，所以用户选的模型不会在第一次运行后失效。服务商真正下线某个模型时，错误来自服务商 API。命令、JSONL、session 恢复、错误映射、输出上限、取消与进程组终止全部封装在 adapter 内。Runner 只看到通用 `EngineError`，其中包括 missing、unauthenticated、rate-limited、session-invalid、process、protocol、cancelled、timeout 和 output-limit。

正式 Turn 默认没有“5 分钟无输出”或总墙钟超时；长时间无输出本身不表示 Engine 已失效。总 Turn 超时只有在对应 Engine runtime 显式配置时才启用。用户停止 Agent 或退出 Desktop 仍会沿取消/有界关闭路径终止 Engine 进程组。classifier 等短请求继续拥有自己的固定超时。

每个 `AgentRunner` 是一个 actor：

- 同 Agent 永不并发运行两个正式 Turn；
- 不同 Agent 可以并行；
- main 模型最多 2 个并发，triage 最多 4 个并发；
- busy 时的多次 wake 合并为一个 `rerun_requested`；
- Turn 结束后重新读取 durable inbox，不在内存堆积消息正文；
- rate limit 进入结构化 pacer，并按 Engine 提供的 retry-after 或本地 60 秒退避恢复；
- Engine 未登录或凭证无效时，该 Agent 暂停 15 分钟再拉起 Engine（Cumora `daemon.ts` 的 `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`）：失败 Run 不推进 delivery，否则每次 poll 都会重复失败；
- Engine/model/persona fingerprint 不一致时不恢复旧 Engine session。

## 7. 每轮 Turn 的输入

Cumora 不做中央编排：“这条该谁回答”由每个 Agent 的主模型读房间决定，服务端只做事后仲裁。所以主模型每轮看到什么，直接决定协作质量。输入分两部分，照搬 Cumora BYOA 的 `standingPrompt` 与 `chatDelta`（`computer/daemon.ts`）。

### 7.1 固定契约

`agents/<id>/AGENTS.md` 由 Computer 写入，经 OpenCode 派生配置的 `instructions` 进入系统提示词（§13.5）。它包含 persona 与代码拥有的协作契约，每个 Agent 固定不变，不含时间、路径或运行时状态：

- 协作动作一律用 `openwork` CLI，assistant 文本本身不会发布；发消息写 `openwork reply <room-id> <text>` 或 `openwork dm <participant-id> <text>`，含引号或 `$` 的文本用 `--stdin`（Cumora `standingPrompt` 的 `postingMechanicsText` 同样写明发消息方式）；
- glance-and-yield 五条规则（Cumora `glance-protocol.ts` 的 `GLANCE_YIELD_RULES`）：人类按名字或角色点名某人时，不是你就不插话；按实际已发布的消息回复，不按想象中的排队位置；乐观发布，遇到 HELD 重看后再决定；不重复同伴已经说过的，任务完成就停；不认领聊天轮次，认领只用于共享交付物（Card）；
- 点名同伴用 `@<id>`，不用显示名；
- 回复某条特定消息时加 `--quote <msg-id>`（§9.3）；
- 查看用法用 `openwork --help`，只看一个命令用 `openwork <command> --help`；
- 谈到某张卡片时写出它的 id（`card-…`）。Desktop 把消息里的卡片 id 渲染成卡片链接；这是房间与看板之间唯一的连接，Server 不会把看板事件写进房间（Cumora 的做法相同：`src/components/CardLink.tsx`）。

### 7.2 每轮增量

每轮 Turn 的 prompt 只包含动态部分，不再重复 persona：

```text
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
| 时间 | 当前时间，RFC 3339，`+08:00` | Cumora 用 UTC；本仓库时间统一东八区 |
| 房间标题行 | `# <room-id> [<direct\|group>] "<title>"`，Direct 没有标题 | `memory-scope.ts` 的 `conversationHeader` |
| 消息行 | `[<msg-id>] <显示名> (<user\|agent>): <正文>`；正文空白压成一个空格，截断到 600 字 | `snapshotUnread` |
| 引用 | 带引用的消息下一行 `↩ quoting [<msg-id>] <显示名>: <原文前 180 字>` | `cli.ts` 的 inbox 渲染 |
| 行数上限 | 整个 digest 最多 40 条消息行，按 quietest-first 分给各房间，各房间内保留最新的 | `renderInboxDigest` 的 `DIGEST_MAX_MESSAGE_LINES` |
| 未显示的消息 | 就地写明条数与读取命令，不静默省略 | 同上。本批最多 200 条并在成功后整批结算（§8.4），没显示的消息必须让模型知道 |
| 本批之外还有未读 | digest 之后一行 `More unread messages are waiting; they will arrive in a later turn.` | 本批超过 200 条时（§8.4），这些消息不在本批，不会被结算 |
| Climate | 本 Agent 对本批消息作者的私有印象 | §10 |
| 名册 | 全部 active Agent（不含自己）与 `local-user`；人类在前并注明先回答人；每行 `<id> — <显示名>, <role>` | `personas.ts` 的 `rosterSection` |

Agenda Turn（§12）与卡片 Turn（§11.4）使用同样的时间与名册，正文换成各自的说明。

## 8. 消息、唤醒与 triage

### 8.1 写入与唤醒

Message 先写 PostgreSQL，再尽力发布 Redis invalidation。Redis 发布失败不会回滚已提交消息。

Scheduler 按消息 ID 在 Redis 去重后，唤醒房间内除作者外的 active Agent：

- 成员 mute 了房间时，只有私聊、`@<自己>`、或引用了自己的消息仍会唤醒它；
- 作者是 Agent 时，每个接收者每分钟最多被 Agent 触发 30 次，超出的唤醒丢弃；人类消息不限；
- 唤醒只是提示。没有被唤醒的 Agent 也会在约 20 秒一次的 poll 中读到持久收件箱里的消息，所以“谁不该回答”必须在 triage 里决定并随 delivery 结算（§8.2），只收窄唤醒无效。

### 8.2 点名路由

对照 Cumora `routing.ts` 与 `scheduler.ts` 的 `wake()`。人类在群里点名了部分 Agent 时，其他 Agent 先判断这条消息是不是给自己的，而不是直接跑主模型。

**点名对象**由代码确定，不由模型挑选：

- 正文中精确的 `@<agent-id>`（与 mute 例外使用同一匹配规则：`@` 前后不是 `[A-Za-z0-9_-]`）；
- 被引用消息的作者是 Agent 时，该作者。

以下情况**不收窄**：消息含 `@all`；Direct Room；没有点名对象；点名对象已覆盖全部接收者。

**判断**在每个接收者自己的 triage 中进行（§8.3 第 2 步）。Server 不调用模型（§1 第 3 条），所以不能像 Cumora 那样每条消息只调一次；每个未被点名的 Agent 各用自己的 triage 模型回答同一道题：

```text
The message explicitly names one or more agents. Decide whether it is aimed at THEM, or at the room.
Answer "me" when the named agents are the ones expected to act or reply.
Answer "each" when the whole room is still expected to engage.
When you are unsure, answer "each".
```

- 答 `me`：本 Agent 不参与，triage 记 `actionable = false`、`source = routing`，delivery 以 `triage_false` 结算，之后的 poll 不会再为这条消息唤醒它；下次醒来时它仍能在房间里读到这条消息；
- 答 `each`、模型出错、超时或答案无法解析：按参与处理（fail-open）。漏掉该回答的人不会留下任何痕迹，多跑一次只多花 token。

### 8.3 triage 判定顺序

triage payload 由 Server 构造，只有需要模型时才交给 Computer 的 triage 模型。判定顺序对照 Cumora `triage-core.ts` 的 `buildTriageRequest`：

| 顺序 | 条件 | 结果 | 调用模型 |
|---|---|---|---|
| 1 | 本批只有 system 消息 | 跳过（`system_only`） | 否 |
| 2 | 本批有人类消息：本 Agent 被点名，或至少一条人类消息不满足 §8.2 的收窄条件 | 参与（`deterministic`） | 否 |
| 2′ | 本批的人类消息全部点名了别人 | §8.2 的路由判断（`routing`）。答“给全员”即参与；答“给被点名的人”时去掉这些人类消息，用本批其余消息继续第 3–6 步，没有其余消息则跳过 | 是 |
| 3 | 本批全是 Agent 之间的 Direct Room 消息，且不在第 8、16… 条的检查点 | 参与（`agent_dm_engage`） | 否 |
| 4 | 本批每个未读房间都已越过 lap floor | 跳过（`lap_floor`） | 否 |
| 5 | 本批每个未读房间自最近一次人类关注后都已有 20 条 Agent 消息 | 跳过（`loop_cap`） | 否 |
| 6 | 其余（群里 Agent 之间的对话、私聊检查点） | triage 模型判断 | 是 |

**lap floor**（Cumora `triage-core.ts` 的 `pastFloor`）：对每个房间，从最近一次人类关注之后，统计 Agent 消息数 `n` 与发这些消息的不同 Agent 数 `k`。`n > k` 表示有人开始第二次发言，一整轮已经结束。它按参与的人数自动伸缩，没有固定数字。

**人类关注**有两种：`local-user` 在该房间发消息；用户在 Desktop 中看到了该房间的消息（`collab_rooms.user_viewed_seq`，§13.3.4）。sequence 不超过 `user_viewed_seq` 的 Agent 消息视为被人看过，不计入 `n`。因此用户在旁观时，Agent 之间的活动不会一轮就停。

**20 条硬上限**保留为兜底。Cumora 的注释记录它“删过两次，每次都回归”。Cumora 另有两档把 lap floor 放宽到 20 的规则，这里都不采用：房间认领档在 Cumora 中没有写入方，是死代码；“租户内任何人 10 分钟内读过任何房间”一档范围过粗。

triage 失败不会把人类消息丢掉：人类消息在第 2 步确定性参与，路由判断失败时 fail-open。

### 8.4 durable inbox 与 delivery

Runner 从 durable inbox 打开一个 Run，并把每个 Room 的 sequence 范围写入 delivery。单批最多 200 条消息，使用与 Cumora 相同的 quietest-first water-fill：先让每个有未读的 Room 获得自己的窗口，再把余量交给繁忙 Room；每个窗口从该 Room 最旧的未读消息开始。超过本批预算的消息不推进 `last_read_seq`，会在后续 Run 继续出现。

成功完成的 Run 结算它携带的全部 delivery：Agent 回复、`ack` 或保持沉默都算已处理，沉默记为 `completed`。这与 Cumora daemon 在成功 Turn 后自行 `ackSeen` 相同；否则被点名的是别人、选择沉默的 Agent 会在每次 poll 被同一条消息重新唤醒。triage 判定跳过时 delivery 以 `triage_false` 结算。失败、取消或中断保留未结算范围，下次重新读取，因此失败路径上的模型调用和回复具有 at-least-once 特征。

## 9. 发布：HELD、逐字重复与引用

`openwork reply` 与 `openwork dm` 的正文直接写在 id 之后，多个参数按空格拼接（与 Cumora `reply <convo_id> "<body>"` 相同）；含引号或 `$` 的文本用 `--stdin` / `--file <path>`，以 `--` 开头的文本前面加 `--`。`--held-token` 必须写在正文之前。两者共用 `Messages` 的同一段写入事务。事务锁定 Room 行后，按顺序检查 HELD（§9.1，仅群聊）与逐字重复（§9.2），都通过才分配 sequence 并插入。

### 9.1 HELD

HELD 解决并行回复的新鲜度问题：

1. inbox 读取时记录该 Agent 对 Room 的 seen sequence；
2. 发布前 Server 比较当前 sequence；
3. Room 已变化时拒绝发布并签发短期 HELD token；
4. Agent `glance` 最新消息；
5. 使用绑定 Agent、Run、Room、session 和 sequence 的一次性 token 重试；
6. Server 先按 `request_id` 原子预留 HELD，再提交 PostgreSQL 命令与幂等结果，提交成功后才最终消费 token。同一 `request_id` 可在 SQL 失败后继续恢复，其他请求不能抢占预留。

HELD 不是全局锁，也不选举唯一回答者。Direct Room 不做 HELD：两个人同时打字是正常的。

### 9.2 逐字重复拦截

对照 Cumora `cli.ts` 的 VERBATIM-DUP 闸。要发布的正文去掉首尾空白后，与本房间最近一条作者不是自己的 `normal` 消息（人或 Agent）完全相同时，拒绝发布：

- 只比较紧挨着的那一条；不做模糊匹配；
- 群聊和私聊都拦；带 HELD token 重试时也拦。Cumora 记录过一次 Agent 用放行令牌硬发重复内容的事故；
- 拒绝码 `DUPLICATE`，不算 action，不推进 delivery；
- 模型看到的文本附上对方那条消息（截断到 200 字），并提示“对方已经说了，换一个角度、说下一项，或保持沉默”。

检查在锁住 Room 行之后进行，所以两个 Agent 相隔很短各发同一内容时，只有先提交的那条成功。

### 9.3 引用回复

- `openwork reply <room-id> --quote <msg-id>` 引用同一房间的一条消息；目标不在本房间时报错，并告诉模型怎么改，不静默发布无引用的回复。`dm` 不支持引用；
- Desktop 用户也可以对任意消息引用回复，`send_message` 带可选的 `quotedMessageId`；
- 被引用消息的作者即使 mute 了房间，也会被唤醒（§8.1）；
- 被引用消息的作者是 Agent 时，它算 §8.2 的点名对象；
- inbox、`glance`、`messages` 与每轮增量（§7.2）在回复下一行显示被引用的原文。

## 10. Room 与 Climate

Participant 统一表示固定人类用户 `local-user` 和 Agent。

- Direct Room 由排序后的两个 Participant ID 唯一确定；
- Agent 可以原子创建或复用 Direct Room；
- Group 只能由 Desktop 用户创建并修改成员；
- Agent 可以在已有 Group 发送消息，但不能改变 Group audience；
- 已归档 Agent 不能新建 Direct Room、接收 JWT 或进入新工作；
- Desktop 可以查看所有房间，包括用户不是成员的 Agent 之间的 Direct Room，但只能在自己是成员的房间发言。

Climate 是某 Agent 对另一个 Participant 的私有、有方向、跨 Room 当前印象：

```text
(agent_id, about_participant_id)
affinity  [-1, 1]
trust     [-1, 1]
last_note
updated_at
```

只有所属 Agent 能显式更新自己的 Climate。A→B 与 B→A 是两行独立状态；系统不会后台修改，也不保存变化历史。

## 11. Board、Column 与 Card

Board 是 workspace 级共享事实，与 Room 平级。一个 Board 原子创建 `Todo`、`Doing`、`Done` 三列。

### 11.1 Column 类型

每列有可空的 `kind`：`todo`、`doing`、`done`，为空表示未分类。列名可以随意改，语义只看 `kind`（Cumora `board-columns.ts`）。新 Board 的三列分别为 `todo`、`doing`、`done`；`done` 是终态，Agenda 与领取都排除它。

### 11.2 权限

Desktop 用户拥有结构：

- 创建、重命名和删除空 Board；
- 创建、重命名、设置 `kind`、重排和删除空 Column；
- 分配或物理删除 Card。

Agent 的 typed command 只允许：

- 读取 Board/Card；
- 创建 Card；
- 原子领取 Card；
- 分配、更新和移动 Card。

Agent 不能创建、重排或删除 Column，也不能删除 Board 或 Card。

Server 在事务中按固定顺序锁定 Board、Column、Card；Column 与 Card 的 position 都是从 0 开始的连续整数。客户端只表达目标容器和可选 `before_*_id`，不直接计算最终 position。

### 11.3 领取

`openwork card claim <card-id>` 在一个事务中完成：

1. **可领取条件**：卡片不在 `done` 列，并且满足其一：
   - 没有负责人；
   - 负责人是自己（幂等）；
   - 负责人已归档；
   - 卡片超过 20 分钟没有更新，**并且**负责人当前没有 running Run。
2. 不满足时返回 `CONFLICT`，说明当前负责人并提示去做别的卡片；
3. **领取即推进**：卡片当前列的 `kind` 为 `todo` 时，移到本 Board 最左边的 `doing` 列；在 `done`、在未分类列、或 Board 没有 `doing` 列时不动。只前进，不后退。

20 分钟取自 Cumora `cli.ts` 的 `card claim`。“负责人没有 running Run”是 OpenWork 的补充：编码任务的一个 Turn 常常超过 20 分钟且中途不更新卡片，只看时间会在原负责人还在工作时把卡片交给别人，造成重复劳动。由 Desktop 用户指派的卡片同样适用本规则。

### 11.4 卡片唤醒

对照 Cumora `kanban-wake.ts`。以下变化直接唤醒相关 Agent，不经过 triage：

- 卡片被 Desktop 或 Agent **改派**给某个 Agent（负责人真的变了，重复提交同一个值不算）；
- 卡片新建或更新时，标题或描述中**新增**了 `@<agent-id>`（与修改前的文本比较）。

发起者本人和已归档的 Agent 不唤醒。Agent 触发的卡片唤醒与消息唤醒共用每分钟 30 次的限额（§8.1），防止两个 Agent 互相改派形成循环。

Cumora 的卡片唤醒是尽力而为的；这里改为持久：

- 唤醒写入 `collab_card_wakes`（§13.3.6），同一 Agent 同一张卡片只保留一条待处理记录，反复编辑合并为一次；
- Runner 从持久收件箱连同消息一起读到待处理的卡片唤醒；有待处理卡片时直接开始正式 Turn；一个 Run 处理该 Agent 当时全部待处理的卡片；
- Run 成功后结算这些卡片唤醒；失败、取消或中断则保留，下次重试；
- SSE 只负责尽快叫醒 Agent。

卡片 Turn 的正文照 Cumora `manualBriefDelta`：说明这是有人直接交给你的工作，收件箱为空也要处理，不是合适的人就明确说出来；列出每张卡片的标题、id、Board 与所在列，以及 `openwork card claim/move/update` 的用法；附带随唤醒到达的未读消息、时间与名册（§7.2）。

## 12. Agenda

Agenda 默认关闭，用户按 Agent 开启。它只在当前 Desktop Runtime 在线时运行，不保存离线 due queue。

候选来源：

- 分配给该 Agent 且不在 `done` 列的 Card；
- 最近 5 分钟至 6 小时内停滞的 Room。

Runner 启动后等待 90 秒 quiet window，每 60 秒检查一次。Server 先用 Redis cooldown/dedupe 协调，再返回签名 candidate set；triage 模型只能在集合内选择，Server 在正式 Run 前再次校验候选和 Agenda 开关。

Redis 协调不可用时 Agenda 关闭本次尝试。Card-focused Agenda Run 可以没有 Room，但必须带 `focus_card_id`；Room-focused Run 保存 Room sequence anchor。

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

权威 DDL 位于 [`crates/openwork-collab/migrations`](../crates/openwork-collab/migrations)。所有时间都保存为不带时区的上海本地时间，见 [.claude/rules/database.md](../.claude/rules/database.md)。

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
| `id` | 主键；人类用户固定为 `local-user`，Agent 使用 Server 由显示名生成的 slug |
| `kind` | `user` 或 `agent` |
| `display_name` | 非空显示名 |
| `created_at` | 创建时间 |

首次迁移插入 `local-user / user / User`。数据库 trigger 禁止修改或删除这行。

#### 13.3.2 Agent

`collab_agent_profiles`：`agent_id`（主键，引用 Participant）、可空 `role`、非空 `persona`、可空 `archived_at`、`created_at` / `updated_at`。归档而非物理删除，保留历史消息、Run、Card、Climate 和 Agent home。

`collab_agent_runtime_configs`，每个 Agent 恰好一行：`engine_id`、`main_model_id`、`triage_model_id`、`agenda_enabled`（默认 `false`）、正整数 `config_revision`（每次运行配置变化递增）、`updated_at`。profile 描述“这个 Agent 是谁”，runtime config 描述“当前如何运行”。

`collab_agent_climates`，主键 `(agent_id, about_participant_id)`：`affinity`、`trust` 限制在 `[-1, 1]`；`last_note` 可空；不能指向自己；只保存当前状态。

#### 13.3.3 `collab_messages`

| 列 | 约束 |
|---|---|
| `id` | `msg-` + 完整 UUIDv4 十六进制 |
| `room_id` / `author_id` | Room 与 Participant 外键 |
| `sequence` | 正整数；同 Room 唯一 |
| `kind` | `normal` 或 `system` |
| `body` | 非空 |
| `system_payload` | 仅 system Message 可用，且必须是 JSON object |
| `quoted_message_id` | 可空；`(room_id, quoted_message_id)` 复合外键指向 `(room_id, id)` 上的唯一约束，保证只能引用同一房间的消息 |
| `created_at` | 创建时间 |

消息写入事务锁定 Room 行，检查 HELD 与逐字重复（§9），增加 `next_seq`，插入 Message，更新 `last_message_at`。Redis invalidation 在事务提交后尽力发布。

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
| `user_viewed_seq` | 非负，默认 0；用户在 Desktop 中看到的最大 sequence，只增不减。它是 lap floor 的“人类关注”（§8.3），也是 Desktop 计算未读的依据。放在 Room 而不是成员表上，因为用户可以查看自己不是成员的 Agent 之间的房间 |
| `created_by` | 创建者 Participant，不可空 |

Direct Room 的 key 由两个 Participant ID 排序后组成，因此并发首次 DM 仍只产生一间 Room。

`collab_room_members`：主键 `(room_id, participant_id)`，另存 `last_read_seq`（durable inbox 的 settlement 游标，只能由成功 settlement 推进，不能拿来保存短期 seen 状态）、`muted`、`joined_at`。

#### 13.3.5 Board

`collab_boards`：`board-` 前缀 ID、非空 `title`、可空 `description`、不可变来源 `created_by`、`created_at` / `updated_at`。不引用 Room。创建 Board 与三个默认 Column 在同一事务中完成。

`collab_board_columns`：`col-` 前缀 ID、`board_id`、非空 `title`、非负 `position`、可空 `kind`（`todo` / `doing` / `done`）。`(board_id, position)` 使用可延迟唯一约束，允许事务内先移动到临时位置再整体连续编号；`(board_id, id)` 复合唯一键供 Card 外键验证“Column 必须属于同一 Board”。

`collab_cards`：`card-` 前缀 ID、`board_id + column_id` 复合外键、非空 `title`、可空 `description`、非负 `position`、可空 `assignee_id`、不可变来源 `created_by`、`created_at` / `updated_at`。`(column_id, position)` 使用可延迟唯一约束。领取只写 `assignee_id`，不建立第二个所有权字段。

删除规则：Desktop 可删除 Card；Column 只有没有 Card 时才能删除；Board 只有所有 Column 都没有 Card 时才能删除，删除时级联清理空 Column。

#### 13.3.6 `collab_card_wakes`

| 列 | 含义 |
|---|---|
| `id` | `cardwake-` 前缀 |
| `agent_id` / `card_id` | 被唤醒的 Agent 与卡片；卡片删除时级联删除 |
| `reason` | `assigned` 或 `mentioned`；合并时保留最近一次的原因 |
| `run_id` | 最近一次携带它的 Run，可空 |
| `created_at` / `updated_at` | 首次写入与最近一次合并的时间 |
| `settled_at` | 可空；非空表示已处理 |

部分唯一索引 `(agent_id, card_id) WHERE settled_at IS NULL` 保证每个 Agent 每张卡片只有一条待处理记录；合并只更新 `reason` 与 `updated_at`。Run 打开时把该 Agent 全部待处理记录的 `run_id` 指向自己；Run 成功后结算 `run_id` 等于它的记录；失败、取消和中断不结算，下一个 Run 会重新指向它们。

#### 13.3.7 Run、delivery、事件与 triage

`collab_runs` 保存一次正式 Agent 工作：

| 分类 | 列 |
|---|---|
| 身份与 fencing | `id`、`agent_id`、`runtime_session_id` |
| focus | 可空 `room_id`、可空 `focus_card_id`（Agenda 使用） |
| trigger | `message` / `card` / `rerun` / `reconnect` / `poll` / `agenda` / `user` |
| 模型快照 | `engine_id`、主/triage 模型、`runtime_config_snapshot` |
| 状态 | `running` / `completed` / `failed` / `cancelled` / `interrupted` |
| 观测 | heartbeat、token delta、rate limit、错误、outcome |

约束保证：每个 Agent 最多一条 running Run；running 没有 `ended_at`，终态必须有；completed 必须有 `acted` / `silent` / `unpublished` outcome；Agenda Run 必须有 Card 或 Room focus 以及非空原因；非 Agenda Run 不能伪造 Agenda focus。新 RuntimeSession 启动时把其他 session 残留的 running Run 标记为 interrupted。

`collab_run_deliveries`：主键 `(run_id, room_id)`，记录本次 Run 携带的 `[from_seq, up_to_seq]`。`eligible_reason` 只能是 `action`、`ack`、`triage_false` 或 `completed`；eligible 与时间必须同时出现；settled 只能发生在 eligible 之后。成功结算时 Server 依据 delivery 最大 sequence 推进对应成员的 `last_read_seq`。

`collab_triages`：classifier 或确定性短路的输入范围、决定、`response_mode`（路由判断的 `me` / `each`）、来源、Engine/model、usage 和 latency。`source` 取值：`empty_inbox`、`system_only`、`rate_limited`、`deterministic`、`routing`、`agent_dm_engage`、`lap_floor`、`loop_cap`、`local_model`、`engine_error`、`human_dm`。`run_id` 可空以保留已结束 Run 之外的决策；`runtime_session_id` 防止跨 session 混用。

Run 与 triage 只供运行时内部使用：结算、当前状态、路由与一轮上限的判定。协作模式不提供运行记录，也不保存 Runner 或 Engine 的过程事件；Desktop 只投影当前状态和房间里的说明行，见 [collaboration-desktop.md](collaboration-desktop.md)。

#### 13.3.8 命令幂等与 Engine inventory

`collab_command_requests`：Desktop 和 Agent 写命令共用。`request_id` 使用 `req-` 前缀；`semantic_hash` 对结构化命令语义计算；Agent 命令以 `(run_id, request_id)` 唯一，Desktop 命令以 `(runtime_session_id, request_id)` 唯一；已完成结果保存为 JSON object 并可原样重放；相同 request ID 携带不同语义时冲突。

`collab_engine_inventory`：每个 Engine 一行最后观测，`status` 为 `unknown` / `ready` / `missing` / `error`，另有 `version`、`checked_at`、`last_error` 与产生该观测的 `observed_session_id`。只供展示；启动 Runner 还必须有 Computer 当前 session 的实时 probe 结果。

### 13.4 Redis

Redis key/channel 都在 `openwork:` namespace：

| namespace | 用途 | 典型 TTL/语义 |
|---|---|---|
| `openwork:message.new` | Message committed Pub/Sub | invalidation |
| `openwork:wake:<agent>` | per-Agent wake Pub/Sub（消息与卡片唤醒共用） | invalidation |
| `openwork:wake-claim:<message>` | scheduler dedupe | 60 秒 |
| `openwork:turn-rate:<agent>` | Agent 触发的消息与卡片唤醒限速 | 60 秒 |
| `openwork:seen:<agent>:<room>` | 发布新鲜度 sequence | 10 分钟 |
| `openwork:hold:<agent>:<room>:<token>` | 一次性 HELD binding | 2 分钟 |
| `openwork:hold:<agent>:<room>:<token>:request` | HELD 的 `request_id` 预留所有者 | 与 HELD 同量级短 TTL |
| `openwork:agenda-rate:<agent>` | Agenda dispatch cooldown | 5 分钟 |
| `openwork:agenda-nudge:<room>` | Room nudge cooldown | 45 分钟 |
| `openwork:agenda-declines:<agent>` | 连续 decline 计数 | 6 小时 |

Redis 不保存消息正文、Agent config、Board、Run、卡片唤醒或待执行 Agenda queue。Redis 清空或短暂不可用可能造成一次额外 poll/triage，不能造成 durable fact 丢失。

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

持久 Agent home 只保存受管 persona 与协作契约（§7.1）、私有工作文件和最小 Engine continuity，不创建 Memory、Notes 或 Skills。RuntimeSession 目录只保存短期凭证与派生配置：启动清除陈旧目录，正常退出清除当前目录。

OpenCode 以 `OPENCODE_DISABLE_PROJECT_CONFIG=1` 运行，不会自动读取 cwd 上方的 `AGENTS.md`。正式 Turn 的派生配置用 `instructions` 引用 `agents/<agent-id>/AGENTS.md` 的绝对路径；分类调用使用单独的配置目录，不加载 persona。

多个 Agent 的 `work` 彼此独立；它不是多个 Agent 共同操作同一个真实项目 checkout。Engine 进程能读写哪些目录由 Seatbelt 约束（§3.1）。

### 13.6 事务与并发不变量

1. Room sequence 在锁定 Room 行后递增；HELD 与逐字重复在同一锁内检查；
2. Direct Room 依靠唯一 key 抵抗并发创建；
3. Card 领取在事务中复核当前负责人、领取条件与负责人的 running Run，并在同一事务内推进列；
4. Board/Column/Card 操作统一按 Board → Column ID → Card ID 的固定顺序加锁；
5. Column/Card 重排使用可延迟唯一约束并重新写成连续整数；
6. 每 Agent running Run 依靠部分唯一索引兜底；每 Agent 每卡片待处理唤醒依靠部分唯一索引合并；
7. 命令幂等结果与业务写入位于同一事务；HELD 先按同一 `request_id` 预留，事务提交后才消费，同一请求可幂等恢复；
8. delivery 与卡片唤醒只在成功终态结算；
9. `user_viewed_seq` 只增不减，且不超过 `next_seq - 1`；
10. Climate owner 来自 Agent JWT，而不是客户端字段；
11. Redis 协调错误永远不能伪装成 PostgreSQL 事务成功。

## 14. 故障语义

| 故障 | 行为 |
|---|---|
| Server 启动失败 | Desktop setup 失败并清理本次 runtime 目录 |
| Server 或 Computer crash | 整个 RuntimeSession 成组替换，旧凭证失效 |
| Desktop/management/Agent SSE 断线 | 对应连接独立退避重连，snapshot/poll 保底 |
| Redis Pub/Sub 不可用 | 消息和卡片唤醒仍持久；即时 wake 可丢失，poll 恢复 |
| Redis 安全协调不可用 | HELD/Agenda 等需要原子协调的动作按各自规则拒绝或关闭 |
| Engine rate limit | 记录结构化错误与 retry-after，pacer 延后后续调用 |
| Engine 沙箱自检失败 | OpenCode inventory 为 error 并显示原因，不启动任何 Runner |
| Engine 未登录或凭证无效 | Run 记为失败，该 Agent 暂停 15 分钟（聊天、卡片与 Agenda 共用），其他 Agent 不受影响 |
| 路由判断失败或超时 | 按参与处理，照常进入正式 Turn |
| Runner panic/异常退出 | Computer 立即进入有界指数退避重建，不等待 roster poll；重复失败仍可观测且不形成紧循环 |
| Engine 忽略取消 | 先终止进程组，超时后强制结束子进程 |
| Run 在结算前中断 | delivery 与卡片唤醒不结算，下次启动重新读取 |
| Run 成功完成但 Agent 没有回复或 ack | delivery 以 `completed` 结算，不再因同一批消息重新唤醒 |

## 15. 演进约束

- 如果将来需要全局唯一的模型决策（例如 Cumora 的 one-of-us 选主：没点名时只派一个 Agent），不能让每个接收者各自判断。届时由 Server 通过新的任务通道把决策派给 Computer 统一执行一次，而不是让 Server 自己调用模型或持有凭证（§1 第 3、6 条）。
- steer（Turn 进行中插入新消息）需要先把 OpenCode adapter 从每 Turn 一个 `opencode run` 改为每 Agent 一个常驻 `opencode serve`（opencode `session/prompt.ts` 的 `prompt()` → `loop()` 在同一进程内接入正在运行的 runner）。Cumora 的 OpenCode adapter 同样没有 steer。

## 16. 验收

完整协作测试必须覆盖：

1. Desktop → Server → Computer → shim → fake OpenCode → durable reply；
2. Server crash 与 Computer crash 都会轮换 session 和两个子进程；
3. 正常 Desktop 退出后没有 Server、Computer 或 Engine 子进程；能在窗口内完成的 Turn 不被提前取消，超时的 Turn 被终止且 Run 为 `interrupted`，多个 Agent 共用同一个 deadline；
4. 三类 SSE 各自断线重连，重连循环只有一份实现；
5. Redis 启动时不可用、运行中断开后恢复：消息与卡片唤醒不丢失，不能启动不安全 Agenda；
6. 每个 Agent 独立 Runner、JWT、home 与 Engine session；一个 Engine 缺失不影响其他 Engine 的 Agent；
7. 人类消息确定性参与、Agent triage、HELD、Direct Room 与 Climate 权限；成功完成但沉默的 Run 也结算 delivery；
8. 每轮增量：时间、房间标题行、显示名与身份、消息 id、引用行、名册逐字符合 §7.2；超过 40 行时就地写明未显示条数与读取命令；persona 不在增量中重复；
9. 点名路由：`@` 与引用都能点名；`@all`、Direct Room、无点名、点名覆盖全员时不收窄；未被点名的 Agent 答 `me` 时 delivery 以 `triage_false` 结算且后续 poll 不再唤醒，答 `each`、出错、超时、无法解析时进入正式 Turn；被点名者不调用路由判断；
10. lap floor：`n > k` 时确定性跳过；用户发消息或 `user_viewed_seq` 覆盖的 Agent 消息不计入；Agent 私聊在检查点之间不受影响；20 条硬上限仍然生效；
11. 逐字重复：群聊、私聊、带 HELD token 都被拦；只比较紧挨着的一条；并发提交同一内容只有一条成功；被拦时 delivery 不推进；
12. 引用：只能引用同一房间；引用穿透 mute；inbox、glance、messages 与增量显示引用行；
13. Card 领取：`todo` 推进到最左的 `doing`，`done`、未分类列与无 `doing` 列的 Board 不动；20 分钟未更新且负责人没有 running Run 时可接手，负责人有 running Run 时不可接手，负责人已归档时立即可接手；并发领取只有一个成功；
14. 卡片唤醒：真实改派与新增 `@` 触发，重复提交同一负责人或已有的 `@` 不触发；发起者不被唤醒；同一卡片反复编辑合并为一条；Run 失败后仍待处理、成功后结算；Agent 触发的卡片唤醒受每分钟 30 次限额；
15. Board 并发 self-assign、并发 move 与 Agenda；Column `kind` 替换原终态标记后 Agenda 仍排除 `done`；
16. OpenCode rate limit、session invalid、输出上限、敏感信息脱敏、取消和强制终止；
17. Engine 沙箱：Engine 进程只能写本 Agent 的目录，读不到 `$HOME` 下其他 Agent 的目录与 token，沙箱不可用时不启动；
18. 存储：migration 可在全新隔离数据库一次建立全部 schema；`local-user` 无法更新或删除；Direct Room 并发创建仍只有一行；Climate owner-scoped 且方向独立；stale Engine observation 不会启动 Runner；runtime token 不进入持久 Agent home；
19. opt-in 的真实 OpenCode smoke。

命令见 [`crates/openwork-collab/README.md`](../crates/openwork-collab/README.md)。
