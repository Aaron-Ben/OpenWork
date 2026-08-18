# Cumora BYOA 实现详解

**这是外部参考资料，不是 OpenWork 的设计文档。** 它描述 Cumora 的 BYOA（Bring Your Own Agent）链路——用户机器上的常驻 daemon 如何驱动本地 Claude Code / Codex 作为多个常驻 Agent 的大脑——用于给 OpenWork 的[协作模式](../collaboration.md)提供可对照的先例。**本篇的任何结论都不构成对 OpenWork 的约束**；落到 OpenWork 的决定写在 [collaboration.md](../collaboration.md)，以那篇为准。

> 基于 `cumora/server/src` 源码通读整理。**已通读**：`agents/computer/daemon.ts`、`agents/computer/engine.ts`、`agents/triage-core.ts`、`agents/seen-boundary.ts`、`agents/glance-protocol.ts`、`agents/agenda.ts`。**按需检索**：`agents/scheduler.ts`、`agents/idle.ts`、`agents/scanner.ts`、`agents/computer/registry.ts`、`agents/runtime/server.ts`、`agents/cli.ts`。**未读**：Cloud 侧的 `turn.ts` / `runtime/orchestrator.ts` / `runtime/pod-agent.ts`（K8s Pod 路径，与 BYOA 无关）。行号为阅读时的实际位置，上游演进后会漂移。

---

## 0. 一句话定位

BYOA 不是"把 Agent 部署到用户机器上"，而是**把大脑换成用户自己付费的本地 CLI，其余全部留在服务器**。

daemon 只通过 HTTPS 访问 `/api` 与 `/runtime`，**不持有数据库或 Redis 凭证**。它做四件事：保持 SSE 唤醒连接、在本地跑 triage、按需 spawn 引擎、把引擎的世界动作经 shim 转发回服务器。所有业务事实（消息、seen 游标、HELD、看板、认领）都在服务器一侧判定。

这个切分决定了整套代码的形状：**daemon 的复杂度几乎全部是"进程与配额管理"，一点业务逻辑都没有。**

---

## 1. 模块地图（只画 BYOA 链路）

```text
server/src/agents/
├── computer/
│   ├── daemon.ts        2601 行  BYOA 主进程：配对、AgentRunner、唤醒、triage、限流、安装
│   ├── engine.ts        1534 行  本地引擎适配：claude / codex，探测 · session · classify · doctor
│   └── registry.ts       479 行  服务器侧 Computer 注册表：配对码、device token、心跳、host 解析
│
├── triage-core.ts        (纯)   Cloud 与 BYOA 共用的 triage 协议核心，零副作用依赖
├── glance-protocol.ts    (纯)   五条协作规则，Cloud 与 BYOA 逐字共用
├── skype-emoticons.ts    (纯)   表情短码提示片段
├── agent-voice.ts        (纯)   全局说话规则
├── seen-boundary.ts             seen 游标 + HELD token（Redis）
├── agenda.ts             556 行  卡片 / 日历 / 停滞会话 → 小模型判断有没有真活
├── idle.ts               217 行  空闲轮转唤醒
├── scanner.ts            209 行  跨会话环境扫描
├── scheduler.ts          892 行  消息 → 候选 Agent → wake（BYOA 与 Cloud 在此分叉）
├── cli.ts               6007 行  世界动作总线；shim 最终打到这里
│
└── runtime/
    ├── server.ts         665 行  挂在 /runtime 的控制面，Cloud Pod 与 BYOA daemon 共用
    ├── jwt.ts                    Agent runtime JWT 签发与校验
    ├── cli-argv.ts               身份绑定：剥离 --as，注入 JWT sub
    ├── wake-bus.ts       285 行  Redis pub/sub ↔ 每 Agent SSE
    └── sse-parse.ts              无副作用 SSE 解析，daemon 与 pod-agent 共用
```

**纯模块（标 `(纯)`）是刻意的**：它们零副作用依赖，因此能被打包进 standalone 的 daemon 二进制，保证 Cloud 与 BYOA 的协作行为逐字一致。`triage-core.ts` 的头注释把这条写死了——Cloud 走 `build → 云端小模型 → parse`，BYOA 走 `服务器 build（它有 DB）→ 本地小模型 → 同一个 parse`。

---

## 2. 进程与信任边界

```text
用户机器                                          Cumora 服务器
┌──────────────────────────────────┐             ┌────────────────────┐
│ daemon (node)                    │  HTTPS/SSE  │ Express            │
│  ├─ AgentRunner × N              │◄───────────►│  /api  /runtime    │
│  │   ├─ EngineSession (claude)   │             │                    │
│  │   └─ home ~/.cumora/agents/<id>/            │  PostgreSQL        │
│  │        └─ bin/cumora  (shim)  │             │  Redis             │
│  └─ triage（本地小模型 one-shot） │             └────────────────────┘
└──────────────────────────────────┘
        │                                              ▲
        └── 引擎在 bash 里跑 shim ──► POST /runtime/cli ┘
```

| 边界 | 谁在里面 | 拿得到什么 |
|---|---|---|
| daemon 进程 | 用户机器 | device token；每 Agent 的 runtime JWT（2h TTL） |
| 引擎子进程 | 用户机器 | shim + `CUMORA_AGENT_RUNTIME_URL/TOKEN`；cwd 是自己的 home |
| 服务器 | Cumora | 全部业务事实与判定 |

**daemon 从不判定业务。** 它甚至不知道"这条消息该不该回"——那是 triage 的裁决，而 triage 的**请求是服务器构造的**（`/inbox-triage/payload`），daemon 只负责在本地跑模型并把结果回报。

---

## 3. 配对与身份

### 3.1 Computer 注册表（`computer/registry.ts`）

`ComputerKind = 'cloud' | 'local' | 'vps'`（`registry.ts:22`）。`cloud` 是每公司的逻辑默认 host，不是真实设备；`isByoaKind()` 判定其余两种。

| 机制 | 细节 |
|---|---|
| 配对 | 持久化 pair token，公司级或 computer 级；`kind <> 'cloud'` 才能配对 |
| 可配对引擎 | `PAIRABLE_ENGINES = {claude, codex}`（`registry.ts:48`） |
| device token | 原始值**不明文持久化**，库里存 `credential_hash` |
| 心跳 | daemon 每 30s；`sweepOfflineComputers` 把心跳过期的置 `offline` 并广播 |
| 状态广播 | 只在 `offline→online` 的**转换**上广播，稳定心跳不刷事件 |
| 撤销 | `revoked_at` + 清空 `credential_hash`，其 Agent 随之离线；cloud 不可撤销 |
| Agent token | `AGENT_TOKEN_TTL_SECONDS = 2h`（`registry.ts:109`），daemon 提前刷新 |

`resolveAgentHost(agentId)` 返回 cloud 还是具体 computer，`scheduler.ts` 据此决定走 Pod 还是 BYOA wake。

### 3.2 身份绑定（`runtime/cli-argv.ts`）

`/runtime/cli` 收到 argv 后：**删除全部 `--as` / `--as=...`，再把 JWT `sub` 对应的 `--as` 放到最前面**。

原注释点明目的：即使 Cloud Pod 或 BYOA 本地进程**被提示注入**，也不能通过 CLI 参数操作另一个 Agent 的身份。这是把"身份"从可被模型影响的输入里彻底拿走，而不是靠校验。

---

## 4. 引擎适配层（`computer/engine.ts`）

### 4.1 接口

```ts
type EngineId = 'claude' | 'codex'
ENGINE_IDS: EngineId[] = ['claude', 'codex']   // 也是默认探测顺序

interface EngineRunArgs {
  home: string          // Agent 的隔离 home，直接作为引擎 cwd
  prompt: string        // 每次唤醒的触发 prompt
  env: NodeJS.ProcessEnv// 含 shim 接线
  model?: string        // 大脑模型 → --model
  fastModel?: string    // 小脑模型；Claude 走 ANTHROPIC_SMALL_FAST_MODEL，Codex 无此旋钮
  resumeSessionId?: string  // 续上一次 session，让 Agent 记得自己做到哪
  onLog / onHopUsage / signal
}

interface EngineUsage {   // 原样透传 Anthropic 字段名，本模块不引入定价
  input_tokens / output_tokens
  cache_read_input_tokens / cache_creation_input_tokens
}
```

`resumeSessionId` 的注释解释了它为什么承重：BYOA Agent 靠它**记得自己在一个进行中的任务里的位置**（例子是接龙计数——它知道自己已经说过 "2"），而不是每次从冻结的 inbox 快照重新推导。

### 4.2 二进制发现与跨平台

- 在 PATH 上找 `claude` / `codex`，读版本与登录状态；
- **Windows 上它们通常是 `.cmd` shim，Node 无法直接 spawn**（`engine.ts:46`），因此 `.cmd`/`.bat` 走 `shell: true` 并**用 stdin 喂 prompt**；
- 找不到就交给 shell 解析，同样 stdin 喂 prompt。

### 4.3 Codex 适配：app-server JSON-RPC

传输是 `codex app-server --listen stdio://`（`engine.ts:960`），注释说明选它的理由是**Codex 原生的上下文管理与自动压缩**，而 `codex exec` 每次都重付冷启动且不保留上下文。

```text
initialize { clientInfo, capabilities: { experimentalApi: true } }
  → initialized
  → thread/start | thread/resume
       params: { cwd: home, approvalPolicy: 'never',
                 sandbox: 'danger-full-access',
                 experimentalRawEvents: true,
                 developerInstructions: <standing prompt>,   // 可选
                 model: <model> }                            // 可选
  → turn/start  每次唤醒
  → turn/steer  { threadId, expectedTurnId, input:[{type:'text',text}] }
  → turn/completed  → resolve 本轮 pending
```

| 事实 | 处理 |
|---|---|
| `thread/resume` 失败 | 日志 `thread/resume failed — starting a fresh thread`，用**不带 threadId 的同一份 params** 重开 |
| usage 是**线程累计** | `cum` / `turnStart` 两个快照相减得到本轮（`engine.ts` `CodexSession`） |
| `experimentalRawEvents` | 发无载荷 `rawResponseItem/*` ping，当**存活信号**，日志里抑制 |
| item 事件 | `commandExecution`（带命令原文）、`agentMessage`（带文本）、`contextCompaction`（原生压缩起止） |
| 账号限流 | ≥90% 时打警告："turns will start failing when it reaches 100%" |
| `steerGate` | 一个 turn 内只允许一次 steer 注入 |
| **app-server 需要 git 仓库** | `codex exec` 有 `--skip-git-repo-check`，app-server **没有** |

`ensureGitRepoForCodex(home)`：

```ts
execFileSync('git', ['init'], { cwd: home })
execFileSync('git', ['-c','user.name=cumora','-c','user.email=cumora@local',
                     '-c','commit.gpgsign=false',
                     'commit','--allow-empty','-m','cumora init'], { cwd: home })
```

注释写明：**只 init + 一次空提交，绝不 `git add`**，否则 home 下 operator 的 token 与文件会被 stage。失败是 best-effort——app-server 可能拒绝，`startSession` 回落一次性 `exec`。

### 4.4 Claude 适配

`--dangerously-skip-permissions`，stream-json 流，持久 session + `--resume`，system prompt 走文件，`MAX_THINKING_TOKENS` 默认置 `'0'`，`fastModel` 走 `ANTHROPIC_SMALL_FAST_MODEL`。

关键差异（`daemon.ts:1969` 的注释）：**`claude -p` 没有 mid-turn interrupt**。所以 Claude 路径收到 `steer` 事件时只能靠"续上下文的补跑 turn"来消化，而 Codex 有原生 `turn/steer`。

### 4.5 四种能力与降级

`主 turn` / `一次性 classify` / `probe` / `doctor`。持久 session 不可用（自定义参数覆盖、opt-out env、Windows 上的 codex、git init 失败）时**塌缩成一次性 `codex exec` / `claude -p`**。

`doctor` 用 `mkdtemp` 建全新临时目录，并**每个引擎一个独立子目录**——因为 codex 可能在里面 `git init`（`engine.ts:1513`）。

---

## 5. daemon 主循环

### 5.1 常量表

这张表是本篇最有复用价值的部分：每个数字背后都有一次真实故障。

| 常量 | 值 | 作用 |
|---|---|---|
| `HEARTBEAT_MS` | 30s | Computer 心跳 |
| `AGENT_POLL_MS` | 60s | 刷新本机被分配的 Agent 列表 |
| `RUN_HEARTBEAT_MS` | 60s | 长 turn 期间 bump `agent_run.updated_at` |
| `INBOX_POLL_MS` | 20s | **SSE 之外的兜底**：不忙时定期 drain inbox |
| `WAKE_DEBOUNCE_MS` | 2.5s | 合并突发消息，一次 turn 处理一批 |
| `TOKEN_REFRESH_SKEW_MS` | 5min | 提前于 2h TTL 刷新 JWT |
| `AGENDA_QUIET_MS` | 90s | Agent 必须安静这么久才考虑 agenda 主动性 |
| `AGENDA_CHECK_MS` | 60s | agenda 检查自身节流，防 20s 轮询打爆 |
| `MIN_SPAWN_INTERVAL_MS` | 500ms | spawn 最小间隔，遇错**指数退避** |
| `BIG_BRAIN_CONCURRENCY` | 6 | 同机并发主推理上限 |
| `TRIAGE_CONCURRENCY` | 8 | 同机并发 triage 上限（注释记录：曾是 4，7–8 Agent 广播唤醒时排到第二波超时） |
| `TRIAGE_TIMEOUT_MS` | 30s | triage 超时即 abort |
| `ENGINE_BACKOFF_AFTER_RATE_LIMIT_MS` | 60s | 命中限流后整机退避 |
| `SHUTDOWN_GRACE_MS` | 15s | 优雅退出宽限；超时**转为等空闲**而不是打断工作 |
| `GROUP_STEER_MIN_INTERVAL_MS` | 8s | 群聊 steer 节流 |
| `MAX_LOG_BYTES` / `LOG_ROTATE_MS` | 20MB / 5min | 日志轮转 |
| `UPDATE_CHECK_MS` | 6h | 查 npm 有无新版 |

`BigBrainSemaphore` 是 FIFO 计数信号量；`AdaptivePacer` 做确定性间隔——成功若干次后收敛回基线，**每次出错翻倍**。

TRIAGE_CONCURRENCY 的注释保留了症状原文，值得整段照抄进任何类似系统的设计评审：并发起本地 CLI 会触发账号限流，然后"每个 Agent 的 triage 停 30 秒"，日志表现为 `local triage RATE-LIMITED (timed out) — process exited with code 143`，实测耗时区间 `triage 4983-7772ms`。

### 5.2 唤醒路径

```text
streamLoop()  ── SSE /runtime/wake-stream，指数退避重连（1s → 30s 封顶）
   │   连上后立刻 kickTurn('reconnect-catchup')   ← 补偿订阅前到达的消息
   │   事件：wake | steer | ready(keepalive)
   ▼
scheduleWake(reason, convo)
   │   WAKE_DEBOUNCE_MS = 2.5s 合并
   ▼
runTurn(reason)
   ├─ busy? → pendingRerun = true，直接返回
   ├─ snapshotUnread()      → seen map + inbox digest + hasReal
   ├─ inboxTriage()         → 本地小模型裁决
   │     actionable=false → 记账 + ackSeen，不唤醒大脑
   ├─ ensureEngineSession() → thread/resume 或新建
   ├─ turnPrompt(session, chatDelta(...))  → send()
   └─ 结束：ackSeen · 落 run · 若 pendingRerun 再来一轮
```

SSE 事件里带 `at`（服务器发布时间），daemon 算出 `deliveryLatency=Nms` 打进日志——注释说这是"收消息慢"时**第一个该看的地方**，并提醒它含时钟偏差，只能当趋势读。

### 5.3 忙时

- `pendingRerun` 是**布尔而非队列**：忙时到达的任意多次 wake 合并成恰好一次补跑；
- `steer` 事件在 Claude 上无法真正注入，靠续上下文的补跑消化；Codex 上走原生 `turn/steer`；
- 群聊 steer 有 `GROUP_STEER_MIN_INTERVAL_MS` 与 `lastGroupSteeredMsgId` 双重节流。

### 5.4 引擎 session 的生命周期

session id 存在 `~/.cumora/sessions/`，**在 Agent home 之外**（`daemon.ts:39`），理由写在注释里：引擎的 cwd 就是 home，放里面会被它自己覆盖。跨 daemon 重启存活。

三类错误触发重建（`mustResetSession` / `isContextOverflow` / `isPoisonedTranscript`）：

```ts
CONTEXT_OVERFLOW_RE = /context window|context length|context_length_exceeded|maximum context|
                       reached its context|prompt is too long|input is too long|too many tokens/i
POISONED_BODY_RE    = /no (?:low|high) surrogate|unpaired surrogate|lone surrogate|
                       surrogate in string|request body is not valid json/i
```

第二条对应 `text-safety.ts` 想解决的问题：一个被截断的 emoji 进了持久 session，会让该 session **永久**无法通过严格 JSON 解析——只能整个重建。

---

## 6. shim（`daemon.ts:522`）

写到 `<home>/bin/cumora`，是一段**内嵌在 daemon 源码里的 Node 脚本字符串**。核心 40 行，做四件事：

1. **token 优先读文件而非 env**。注释说明原因：持久引擎进程只 spawn 一次，**env 里的 token 会在刷新后变陈旧**，文件永远是最新的；env 作为一次性 / codex 路径的回落。
2. **`--file` / `--stdin` 绕开 shell**。这条注释是整个 shim 里最有价值的：一条内联写的 reply，在 shim 运行**之前**就已经被 bash 改坏了——反引号与 `$(...)` 被当命令执行并塌成空、引号被吃掉。所以正文从文件或管道读，在本地读成**一个参数**，以 JSON 传输，永不被 shell 二次解析。
3. `POST /cli`，body 是 `{ argv }`，Bearer 是 runtime JWT。
4. 把 `data.text` 打到 stdout，用 `data.exitCode` 退出；任何失败退 70。

对应地，standing prompt 里有一整段教模型：**任何含反引号 / 代码 / `$` / 多行的消息，先写文件再 `--file`**，短纯文本才允许内联单引号。

---

## 7. triage

### 7.1 纯核（`triage-core.ts`）

头注释写死了 AI-NATIVE 原则：

> 这里的每一个**决定**都由小模型做出，绝不由正则做出。**没有任何正则去分类消息内容**（"这是问候吗？""这是 @all 吗？""这是在叫我吗？"）——那些判断属于小脑。唯一的非模型短路是"inbox 为空"，那是**计数**不是分类。剩下的正则只用来**解析模型自己的 JSON 答案**（剥 ``` 围栏、从截断输出里抢救字段）——它们读取决定，不做决定。

### 7.2 裁决

```ts
type ResponseMode = 'me' | 'each' | 'one-of-us'

interface InboxTriageVerdict {
  actionable: boolean
  reason: string
  promptNote: string
  responseMode?: ResponseMode
  source: 'empty-inbox' | 'system-only' | 'rate-limited' | 'loop-cap'
        | 'support-model' | 'support-model-local' | 'fail-open'
        | 'human-dm' | 'human-group' | 'dm-agent-engage' | 'calendar-due'
}
```

模型被要求只回一个 JSON 对象：
`{"actionable": boolean, "responseMode": "me"|"each"|"one-of-us"|null, "reason": string, "promptNote": string}`

**`responseMode` 是粗粒度提示，两个消费者都不据它行动**——Cloud turn 与 BYOA daemon 都让大脑读房间自己决定谁回、怎么回。注释明确它只是"gate 的推理留痕 + 一个稳定的线上字段"。

`source` 里有一半取值是**服务器在调用模型之前的短路**：`empty-inbox`、`system-only`、`rate-limited`、`loop-cap`、`human-dm`、`dm-agent-engage`。

### 7.3 非对称失败

| 场景 | 策略 | 日志原文 |
|---|---|---|
| 有人类在等 | fail **open** | `local triage failed: … — fail open` |
| 纯 Agent（无人类未读） | fail **CLOSED** | `fail CLOSED (agent-only, no human waiting)` |
| 限流 / 超时 | **不唤醒大脑** + 退避 | `local triage RATE-LIMITED — backing off, NOT waking the big brain` |
| 输出无法解析 | 同上二分 | `local triage unparseable … fail CLOSED/open` |

fail-closed 分支的注释解释了理由：每个 triage 错误都唤醒大脑，会**放大它本要抑制的那个循环**。

限流还叠了 `triageBackoffUntil` 与 `triageTroubleStreak`——连续命中才升级退避，防止一个坏掉的 triage 被每次轮询反复捶打。

### 7.4 Agent↔Agent DM

```ts
const DM_AGENT_TRIAGE_EVERY = 8
```

注释：两个 Agent 的 DM 里**默认参与**（"the agents should talk — never silence the exchange"），只是不为每条消息付 triage 成本；小脑按 sequence 每 8 条跑一次，**当死循环探测器**——若发现是没有进展的来回就终止。既省钱又不会让 Agent 变哑。

---

## 8. 发言竞争

### 8.1 seen 游标（`seen-boundary.ts`）

Redis，key `cumora:seen:<agentId>:<convoId>`，`TTL_SECONDS = 600`，用 Lua 做**原子单调 SET**（新值大于当前才写并刷 TTL），保证两个并发写者收敛到较大值、永不回退。

头注释记了两条教训：

> **为什么用 Redis 而不是 `conversation_reads.last_read_at`**：`a6e69aa` 试过放 `conversation_reads`，结果打断了 `loadInbox`——同一行的 `last_read_at` 正是 loadInbox 的 SELECT 游标，把它 bump 到 NOW() 会让下一次 loadInbox 返回空，**daemon 挂成 silent-busy**。**任何与 inbox 游标共享状态的东西在结构上就是不安全的。**

> **为什么 fail-open**：这是协调信号，不是正确性不变量。Redis 挂掉时最坏结果是一次重复发言（本就是要*减少*而非*消除*的 bug），绝不能是 daemon 卡死或消息丢失。旧设计 fail-closed，同步 DB 争用能把一个 turn 永久拖住。

### 8.2 HELD（`cli.ts` `cmdReply` preflight）

`reply` 落库前做新鲜度判定，三种 HOLD：

1. **seq 预检**：有 peer 消息的 sequence 超过本 Agent 的 seen 基线 → HELD，回执列出未被展示过的新消息；
2. **逐字重复**：草稿与该会话最近一条 peer 消息**逐字相同** → HELD；
3. **`--send-anyway` 的 token 校验**：token 承认的是 HELD 当时展示的房间状态（`heldUpToSeq`）；房间若已越过它，`--send-anyway` **无效**，返回 "your --send-anyway acknowledged an EARLIER hold, but the room has moved since"。

```ts
HOLD_TTL_SECONDS = 120
```

注释：一次 HELD 确认只在"与 HELD 同一口气"里有意义——HELD → 重读 → 重跑是几秒的事，不是几分钟。**长 TTL 会把让出的 hold 变成未来的绕过弹药。**

preflight **只在成员数 > 2 时生效**（`preflightApplies = !monologueBypass && cv[0].members.length > 2`）——1v1 没有抢答问题。ack（主动让出）会清掉未使用的 HELD token。

### 8.3 glance 五条（`glance-protocol.ts`）

头注释是这套设计的钥匙：

> Agent 只看到**已发布**的消息流 + 一个私有的 per-(agent, convo) seen 游标——**没有** composing / claim 顺序 / "谁排在你前面"的名册（见 `cmdGlance`，它现在只返回消息流）。所以 Agent 只能对"真正已发布的最新内容"行动：它发出真实的下一项并**去抢**，服务器的新鲜度闸序列化冲突，HOLD 掉输家并把新消息给它重读。这让"按位置占槽"（我是第 3 个认领 → 我发 3）**在结构上不可表达**，正是这一点让那堵按场景堆的 prompt 规则墙塌缩成下面五条。**不要让它长回去：当 Agent 判断出错时，先问服务器的 gate 是不是正确的修法。**

五条依次是：读清人类点名的是谁；从真实已发布状态回复而非从排位推理；乐观发送、服务器是安全网；别重复队友、任务做完就停；**绝不认领聊天轮次或游戏槽位**——认领只属于队友可能重复做的真实工作（一份文档、一张看板卡）。

### 8.4 worklog claim

Redis HASH，key `cumora:worklog:<scopeKey>`，field 由 `(taskType, subject)` 构成。`inproc-client.ts:737` 的注释区分了两者：**seen/HELD 信号"我正在撰写回复"，worklog claim 信号"我正在做这件工作"**；两个 Agent 要做同一件事时都先查 worklog，谁拿到谁做。

---

## 9. 主动性

### 9.1 三层

| 模块 | 触发 | 信号源 |
|---|---|---|
| `idle.ts` | 定时 | 挑一个安静且可用的 Agent，**轮转**避免总叫同一个 |
| `agenda.ts` | idle 唤醒**之前** | 分配/@ 它的非 done 列看板卡 + 当前时段日历事件 + 停滞会话 |
| `scanner.ts` | 定时 | 显式具备 `background.scan` 能力的 Agent；近期活动快照 + fingerprint 冷却 |

`agenda.ts` 头注释解释了它为什么必须存在：

> 朴素的 idle 唤醒把一个泛泛的"有什么值得做的吗？"丢给大脑。那次调用**又贵、而且 Agent 没有议程上下文**——它得先自己查 inbox / 群 / 联系人才能判断要不要行动，**常常烧掉一整轮推理只为得出"没有，没什么可做"**。

于是先用小脑分类器判两件事：(a) 真有值得唤醒大脑的东西吗？(b) 若有，先聚焦哪个。有则带**聚焦 brief** 唤醒，让大脑把这一轮花在**执行**而非**决定要不要执行**上；无则整轮跳过，大脑一个 token 都不烧。

### 9.2 停滞推动的常量与三约束

```ts
STALL_MIN_MS               = 5min      // 最后一条消息距今超过这个才算停滞
STALL_MAX_MS               = 6h        // 超过这个就不算"进行中"了
NUDGE_COOLDOWN_MS          = 45min     // 同一会话的推动冷却，跨所有 Agent
NUDGE_COOLDOWN_FALLBACK_MS = 5min
CALENDAR_LOOKAHEAD_MS      = 30min
CALENDAR_LOOKBEHIND_MS     = 15min
decline cap                = 3
```

三条约束，缺一不可：

1. **冷却键只按会话，不带最后消息 id**。原注释："a nudge changes the last message, so a per-message key would re-arm 5min later and nag."
2. **Redis NX claim** 保证一次停滞只有**一个** Agent 去推。
3. **decline 计数上限 3**，会话一有新消息就 `resetStallNudgeDeclines` 清零——防止"某个 Agent 的小脑说了不用推"就永久封杀这个停滞，也防止已经放弃的话题被无限戳。

`scanner_helper.ts` 的主动拉群：有人类参与的群 `PULL_COOLDOWN_HOURS = 6`，纯 Agent 群可更积极。`scanner.ts` 要求会话窗口内至少 `SCANNER_MIN_MESSAGES = 8` 条、`SCANNER_WINDOW_HOURS = 24`。

### 9.3 BYOA 侧的 agenda sweep

`maybeAgendaTurn()` 双重门控：`lastTurnEndedAt` 距今 ≥ `AGENDA_QUIET_MS`(90s) **且** `lastAgendaCheckAt` 距今 ≥ `AGENDA_CHECK_MS`(60s)。后者是防 20 秒的 inbox 轮询把 agenda 检查也带成 20 秒一次。

---

## 10. `/runtime` 控制面（BYOA 用到的）

`runtime/server.ts` 全部走 `withAgent`——校验 Bearer JWT 并**从 token 固定 agentId/companyId**，端点不接受调用方指定身份。

| 组 | 端点 |
|---|---|
| 唤醒 | `GET /wake-stream`（SSE） |
| 动作 | `POST /cli` ← shim 唯一入口 |
| 上下文 | `/persona` `/inbox` `/context` `/memory/query` `/climate` `/skills` `/faces` `/system-prompt` `/roster` |
| 判断 | `GET /inbox-triage/payload`（服务器构造 triage 请求）`POST /triage`（回报裁决） |
| 主动性 | `GET /agenda` |
| 状态 | `/status` `/status/heartbeat` `/typing` `/busy/heartbeat` `/busy/clear` `/thinking/mark|unmark|peek` |
| 协作 | `/worklog/claim|release|peek` `/conversation/mark-read` |
| 观测 | `/runs` `/runs/:id/heartbeat` `/runs/:id/finish` `/events` `/llm-calls` `/notices` |

**`/inbox-triage/payload` 这个切分很关键**：服务器有 DB，所以它构造 triage 请求；daemon 有本地模型，所以它执行。两边共用 `triage-core.ts` 的同一份 prompt 与 parser。

---

## 11. standing prompt 的形状

`standingPrompt()`（`daemon.ts:1321`）的注释是一份**反膨胀声明**，值得整段引用：

> 恢复到 5/28 基线的**形状**：一份最小的、只讲必要机制的 prompt，加上 `GLANCE_YIELD_RULES` 和少数核心段落——**不是**一堵 `── XXX ──` 分节墙（那正是用户点名的膨胀：`AGENT_VOICE_RULES` 预热、MORE CUMORA COMMANDS、HELD REPLY 讲解、CONTEXT COMPACTION、WORKING A BOARD CARD——5/28 协作完美时这些一个都不存在，可见协作并不需要它们）。Agent 需要时自己用 `cumora <cmd> --help` 去发现别的 CLI 面。

留下来的六块：身份一句话 → 协作协议（`GLANCE_YIELD_RULES` 逐字）→ 发消息的 shell 安全规矩 → 表情短码 → 记忆（`memory/MEMORY.md`，"说'记住了'不会持久化"）→ 推进与收尾（含用日历给未来的自己排检查点）→ 隐私（"stay inside your home directory"）。

每轮的动态部分单独走 `chatDelta()` / `agendaDelta()`，注释说明理由：**保持小，让持久 session 的 transcript 增长得慢，好让原生压缩跟得上。**

---

## 12. 观测与成本

- `HopReporter`：250ms 窗口或攒够 10 条就 flush，最多缓冲 500 条，把每个 hop 的 usage 批量报给 `/llm-calls`；
- `cost.ts` 归一化 OpenAI / Claude / 本地引擎三种 usage 形状，区分 input / cached input / cache creation / output，未知模型标记为**估算**而不是伪装成精确值；
- `llm-ledger` 的目标是"每次出站模型请求对应一条 `llm_calls`"，并记录 **daemon 版本**（管理端有"BYOA 版本分布"）；
- 日志里把 home 路径替换成 `<agent home>`、`homedir()` 替换成 `~` 再上报（`daemon.ts:1004`）。

---

## 13. 可移植性评估

**本节是判断，不是事实，且不构成对 OpenWork 的约束。** OpenWork 的实际取舍见 [collaboration.md](../collaboration.md)。

> 本节按 OpenWork **改用 OpenCode 作为引擎**之后重判过。cumora 自身的实现（上面各节）不受影响。

| Cumora 的东西 | 它为什么存在 | 换成单机本地产品还成立吗 |
|---|---|---|
| daemon 与服务器分离 | 服务器在云上，算力/额度在用户机器 | **不成立**——单机没有这条鸿沟 |
| SSE wake-stream + 重连 + 20s 兜底 probe | 跨公网的长连接会断 | **不成立**——同机可用进程内 channel |
| Redis（seen / HELD / worklog / wake bus） | 服务器多副本 | **不成立**——单实例放内存即可 |
| runtime JWT + `/runtime/*` 全套 | 跨信任边界 | 大幅简化——但**身份绑定要保留** |
| 配对码 / device token / 心跳 / 撤销 | 用户要把设备接进公司 | **不成立** |
| 本地小模型跑 triage | daemon 拿不到服务器凭证 | **不成立**——本地产品可以直接用廉价 API，省掉中性 cwd、并发信号量、限流退避、trouble streak |
| 每 Agent 一个常驻引擎进程 | 一个 Codex/Claude 进程一个 cwd | **不成立**——OpenCode 逐请求按 `x-opencode-directory` 选实例，一个 server 服务全部 Agent，整套进程生命周期管理随之消失 |
| 自己实现审批与沙箱边界 | 引擎只给「全开/全关」两档 | **不成立**——OpenCode 自带 `permission + pattern → allow/deny/ask` 规则集与外发的审批协议，OpenWork 只做呈现与回传 |
| `install/uninstall/self-update/doctor/logs` | 要教普通用户装后台服务 | 取决于产品形态 |
| **triage 的非对称失败** | 行为正确性 | **成立**，直接抄 |
| **seen 与 inbox 游标必须分离** | 一次真实事故 | **成立**，是结构性约束 |
| **HELD + 乐观发送** | 让"按位置占槽"不可表达 | **成立**，且是整套协调的地基 |
| **glance 五条 + 反膨胀声明** | 一次真实的 prompt 膨胀回退 | **成立** |
| **停滞推动的三约束** | 防止 nag 与永久封杀 | **成立** |
| **shim 的 `--file` / `--stdin`** | bash 在 shim 之前就改坏了正文 | **成立但已规避**：OpenWork 改用 MCP，参数走 JSON 不经 shell，这类问题整类消失。任何仍走"模型在 shell 里调 CLI"的设计都会踩 |
| **session id 存 home 之外** | 引擎 cwd 就是 home | **成立**：OpenWork 把它放 `collab_agents.opencode_session_id`，比放 home 外的文件更彻底 |
| **`git init` 但绝不 `git add`** | Codex app-server 要求 cwd 是 git 仓库 | **不适用**：OpenWork 改用 OpenCode，没有这条硬要求，home 里也就没有 `.git/` |
| Cloud Pod / K8s / FUSE / PVC / 多租户 tier | SaaS | **不成立** |

---

## 附：文件索引

| 文件 | 行数 | 一句话 |
|---|---|---|
| `agents/computer/daemon.ts` | 2601 | BYOA 主进程；`AgentRunner` 是每 Agent 的运行时 |
| `agents/computer/engine.ts` | 1534 | claude / codex 适配；`CodexSession` 是 app-server JSON-RPC 客户端 |
| `agents/computer/registry.ts` | 479 | 服务器侧 Computer 注册表与 host 解析 |
| `agents/triage-core.ts` | — | 纯 triage 协议核心，Cloud/BYOA 共用 |
| `agents/glance-protocol.ts` | — | 五条协作规则 + 反膨胀声明 |
| `agents/seen-boundary.ts` | — | seen 游标（Lua 单调）+ HELD token |
| `agents/agenda.ts` | 556 | 卡片/日历/停滞 → 小模型 gate |
| `agents/idle.ts` | 217 | 空闲轮转唤醒 |
| `agents/scanner.ts` / `scanner_helper.ts` | 209 / 149 | 环境扫描与主动拉群 |
| `agents/scheduler.ts` | 892 | 消息 → 候选 → wake；BYOA 在此跳过服务端 triage |
| `agents/cli.ts` | 6007 | 世界动作总线；`cmdReply` 的 preflight 在此 |
| `agents/runtime/server.ts` | 665 | `/runtime` 控制面 |
| `agents/runtime/cli-argv.ts` | — | 剥离 `--as`，注入 JWT sub |
| `agents/runtime/wake-bus.ts` | 285 | Redis pub/sub ↔ SSE |
| `agents/runtime/sse-parse.ts` | — | 无副作用 SSE 解析 |
