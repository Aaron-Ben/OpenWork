# 本机 BYOA 协作运行时

协作模式让多个持久 Agent 通过房间、消息和共享看板协作。它与工作台的 `SessionActor` 运行时彼此独立：协作事实由 Collaboration Server 管理，模型调用由本机 Computer 中的 Engine adapter 执行。

当前产品范围固定为：

- macOS；
- 单个 Desktop 生命周期；
- 单个逻辑 Collaboration Runtime；
- 本机 OpenCode；
- 多个 Agent，每个 Agent 独立选择 `engine_id`、主模型和 triage 模型；
- Room、Message、Climate、Board、Column、Card、Run 和 Agenda。

当前不提供远程 Computer、共享真实项目目录、离线补跑、审批、MCP、Memory、Notes、Skills、reaction 或 Agent 管理群成员。未来接入 Codex 时新增真实 Engine adapter，不改变 Server 业务模型。

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
3. Server 不创建 Engine 子进程；
4. WebView 只调用 Tauri command，不接触 Runtime URL 或凭证；
5. Server 与 Computer 只通过 loopback HTTP/SSE DTO 通信；
6. 协作 crate 不依赖 `openwork-core`、`openwork-credentials` 或工作台 Provider adapter。

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

正常退出按以下顺序：

```text
停止接收新 wake
  → 所有 Runner 共享最多 15 秒收尾窗口
  → 取消剩余 Engine 进程组
  → Computer 退出
  → Server 停止 HTTP/SSE、Redis tasks 与数据库连接池
  → 删除当前 runtime 目录
```

PostgreSQL 和 Redis 是本机基础设施，不属于这个子进程组，Desktop 退出不会停止它们。Desktop 自身被强制杀死后的孤儿回收不在当前范围。

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

Desktop、Computer、Agent credential 不能跨 namespace 互换。

这些凭证与每个 Agent 的独立 home 是**应用层逻辑隔离**，不是 macOS 安全沙箱。Computer、Runner、Engine 和 shim 都属于同一登录用户下的可信本机进程；被攻陷或恶意的本机 Engine 进程仍可能读取该用户有权读取的其他文件。JWT 负责限制 Server API 中“以哪个 Agent、哪个 RuntimeSession 做什么”，不负责建立 OS 级机密边界。

## 4. HTTP 与 SSE seam

Server 只绑定操作系统分配的随机 loopback 端口。所有修改都是 HTTP request；实时通道只使用 SSE。

```text
Desktop             1 条 Desktop SSE
Computer            1 条 management SSE
每个 active Runner  1 条 Agent SSE
```

- Desktop SSE 通知 Runtime、Agent config、Engine inventory、Runner status 或消息可能变化；
- management SSE 只促使 Computer 重新获取完整 desired Agent snapshot；
- Agent SSE 只通知对应 Agent 可能有新工作；
- 每条 Agent SSE 在网络上独立，Server 内部也按 Agent ID 使用独立 wake channel；Alpha 的事件不会先广播给所有 Runner 再由各连接过滤；
- 三类连接都使用 1 秒起步、最多 30 秒的指数退避；
- SSE decoder 对单个未完成事件设置 1 MiB 上限，防止无分隔符输入无限占用内存；
- Computer 仍每 60 秒获取完整 desired snapshot；
- Agent 仍约每 20 秒重新读取 durable inbox；
- SSE 和 Redis Pub/Sub 只传 invalidation，不传业务正文。

事件可以重复或丢失。正确性来自 PostgreSQL 中的 canonical state 和周期重读，不建立事件重放日志。

## 5. Agent desired state 与 reconcile

Server 保存每个 Agent 的：

- Participant identity；
- profile：显示名、role、persona、归档时间；
- runtime config：`engine_id`、主模型、triage 模型、Agenda 开关、`config_revision`。

Computer 启动时获取全量 snapshot；management invalidation 和 60 秒周期 fallback 也进入同一个幂等 `reconcile`：

```text
desired agents + current Engine readiness
  → stop removed or changed Runner
  → prepare persistent home and session runtime files
  → create per-Agent Engine runtime
  → start missing Runner
  → heartbeat observed Runner state to Server
```

Engine、主模型、triage 模型、persona 或 config revision 变化都会重建对应 Runner。一个 Agent 的 home 或 Engine 初始化失败只把该 Runner 标记为 error，不阻塞其他 Agent。归档停止 Runner并保留历史、home 与 Engine continuity；恢复后用新的 config revision 重建。

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

生产 `EngineRegistry` 当前只注册 `OpenCodeAdapter`。OpenCode 的命令、JSONL、session 恢复、错误映射、输出上限、取消与进程组终止全部封装在 adapter 内。Runner 只看到通用 `EngineError`，其中包括 missing、unauthenticated、rate-limited、session-invalid、process、protocol、cancelled、timeout 和 output-limit。

正式 Turn 默认没有“5 分钟无输出”或总墙钟超时；长时间无输出本身不表示 Engine 已失效。总 Turn 超时只有在对应 Engine runtime 显式配置时才启用。用户停止 Agent 或退出 Desktop 仍会沿取消/有界关闭路径终止 Engine 进程组。classifier 等短请求继续拥有自己的固定超时。

每个 `AgentRunner` 是一个 actor：

- 同 Agent 永不并发运行两个正式 Turn；
- 不同 Agent 可以并行；
- main 模型最多 2 个并发，triage 最多 4 个并发；
- busy 时的多次 wake 合并为一个 `rerun_requested`；
- Turn 结束后重新读取 durable inbox，不在内存堆积消息正文；
- rate limit 进入结构化 pacer，并按 Engine 提供的 retry-after 或本地退避恢复；
- Engine/model/persona fingerprint 不一致时不恢复旧 Engine session。

## 7. Message、triage 与 HELD

Message 先写 PostgreSQL，再尽力发布 Redis invalidation。Redis 发布失败不会回滚已提交消息。

User 消息确定性进入正式 Turn。Agent 消息才经过 triage：

- Agent Direct Room 通常直接 engage；
- Agent Direct Room 每连续 8 条 Agent Message 进入一次 classifier；
- triage 检查频率与硬上限互相独立：自最近一次 User Message 后连续 20 条 Agent Message 时确定性 `loop_cap`，不等待下一个 8 的倍数；
- Group 中的 Agent Message 平时由 classifier 判断，但同样受连续 20 条的确定性上限；
- reply/DM 写入边界再次检查 hard cap，防止一个同时包含多个 Room 的批次绕过单 Room 上限；
- triage 失败不会把 User 消息丢掉。

Runner 从 durable inbox 打开一个 Run，并把每个 Room 的 sequence 范围写入 delivery。单批最多 200 条消息，使用与 Cumora 相同的 quietest-first water-fill：先让每个有未读的 Room 获得自己的窗口，再把余量交给繁忙 Room；每个窗口从该 Room 最旧的未读消息开始。超过本批预算的消息不推进 `last_read_seq`，会在后续 Run 继续出现。只有成功完成且有明确 settlement 的 Run 才推进 delivery。失败、取消或中断保留未结算范围，下次 RuntimeSession 可以重新读取，因此模型调用和回复具有 at-least-once 特征。

HELD 解决并行回复的新鲜度问题：

1. inbox 读取时记录该 Agent 对 Room 的 seen sequence；
2. 发布前 Server 比较当前 sequence；
3. Room 已变化时拒绝发布并签发短期 HELD token；
4. Agent `glance` 最新消息；
5. 使用绑定 Agent、Run、Room、session 和 sequence 的一次性 token 重试；
6. Server 先按 `request_id` 原子预留 HELD，再提交 PostgreSQL 命令与幂等结果，提交成功后才最终消费 token。同一 `request_id` 可在 SQL 失败后继续恢复，其他请求不能抢占预留。

HELD 不是全局锁，也不选举唯一回答者。

## 8. Room 与 Climate

Participant 统一表示固定人类用户 `local-user` 和 Agent。

- Direct Room 由排序后的两个 Participant ID 唯一确定；
- Agent 可以原子创建或复用 Direct Room；
- Group 只能由 Desktop 用户创建并修改成员；
- Agent 可以在已有 Group 发送消息，但不能改变 Group audience；
- 已归档 Agent 不能新建 Direct Room、接收 JWT 或进入新工作。

Climate 是某 Agent 对另一个 Participant 的私有、有方向、跨 Room 当前印象：

```text
(agent_id, about_participant_id)
affinity  [-1, 1]
trust     [-1, 1]
last_note
updated_at
```

只有所属 Agent 能显式更新自己的 Climate。A→B 与 B→A 是两行独立状态；系统不会后台修改，也不保存变化历史。

## 9. Board、Column 与 Card

Board 是 workspace 级共享事实，与 Room 平级。一个 Board 原子创建 `Todo`、`Doing`、`Done` 三列，其中 `Done` 使用显式 terminal 标记。

Desktop 用户拥有结构：

- 创建、重命名和删除空 Board；
- 创建、重命名、标记 terminal、重排和删除空 Column；
- 分配或物理删除 Card。

Agent 的 typed command 只允许：

- 读取 Board/Card；
- 创建 Card；
- 原子 self-assign 未分配 Card；
- 分配、更新和移动 Card。

Agent 不能创建、重排或删除 Column，也不能删除 Board 或 Card。Card self-assign 不建立第二套状态：未分配时写 `assignee_id`，重复选择自己幂等，已分配他人时冲突。

Server 在事务中按固定顺序锁定 Board、Column、Card；Column 与 Card 的 position 都是从 0 开始的连续整数。客户端只表达目标容器和可选 `before_*_id`，不直接计算最终 position。

## 10. Agenda

Agenda 默认关闭，用户按 Agent 开启。它只在当前 Desktop Runtime 在线时运行，不保存离线 due queue。

候选来源：

- 分配给该 Agent 且不在 terminal Column 的 Card；
- 最近 5 分钟至 6 小时内停滞的 Room。

Runner 启动后等待 90 秒 quiet window，每 60 秒检查一次。Server 先用 Redis cooldown/dedupe 协调，再返回签名 candidate set；triage 模型只能在集合内选择，Server 在正式 Run 前再次校验候选和 Agenda 开关。

Redis 协调不可用时 Agenda 关闭本次尝试。Card-focused Agenda Run 可以没有 Room，但必须带 `focus_card_id`；Room-focused Run 保存 Room sequence anchor。

## 11. 故障语义

| 故障 | 行为 |
|---|---|
| Server 启动失败 | Desktop setup 失败并清理本次 runtime 目录 |
| Server 或 Computer crash | 整个 RuntimeSession 成组替换，旧凭证失效 |
| Desktop/management/Agent SSE 断线 | 对应连接独立退避重连，snapshot/poll 保底 |
| Redis Pub/Sub 不可用 | 消息仍持久；即时 wake 可丢失，poll 恢复 |
| Redis 安全协调不可用 | HELD/Agenda 等需要原子协调的动作按各自规则拒绝或关闭 |
| Engine rate limit | 记录结构化错误与 retry-after，pacer 延后后续调用 |
| Runner panic/异常退出 | Computer 立即进入有界指数退避重建，不等待 roster poll；重复失败仍可观测且不形成紧循环 |
| Engine 忽略取消 | 先终止进程组，超时后强制结束子进程 |
| Run 在结算前中断 | delivery 不推进，下次启动重新读取 |

## 12. 验收

完整协作测试必须覆盖：

1. Desktop → Server → Computer → shim → fake OpenCode → durable reply；
2. Server crash 与 Computer crash 都会轮换 session 和两个子进程；
3. 正常 Desktop 退出后没有 Server、Computer 或 Engine 子进程；
4. 三类 SSE 各自断线重连；
5. Redis 不可用不丢消息且不能启动不安全 Agenda；
6. 每个 Agent 独立 Runner、JWT、home 与 Engine session；
7. User deterministic、Agent triage、HELD、Direct Room 与 Climate 权限；
8. Board 并发 self-assign、并发 move、terminal 与 Agenda；
9. OpenCode rate limit、session invalid、输出上限、敏感信息脱敏、取消和强制终止；
10. opt-in 的真实 OpenCode smoke。

命令见 [`crates/openwork-collab/README.md`](../crates/openwork-collab/README.md)。存储约束见 [collaboration-data-model.md](collaboration-data-model.md)，Desktop 投影见 [collaboration-desktop.md](collaboration-desktop.md)。
