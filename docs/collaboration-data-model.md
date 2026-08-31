# 协作模式数据模型

本文约束 Collaboration Server 的 PostgreSQL schema、Redis 协调状态和本机文件所有权。权威 DDL 位于 [`crates/openwork-collab/migrations`](../crates/openwork-collab/migrations)。

## 1. 三类存储

```text
PostgreSQL                     Redis                         ~/.openwork
durable business facts        expiring coordination        Agent work + runtime files
        │                              │                           │
        └──────── Collaboration Server ┘                           │
                               │ loopback protocol                 │
                               └──────── Local Computer ───────────┘
```

所有权规则：

1. 只有 Collaboration Server 读写 `collab_*` 表和 Redis；
2. PostgreSQL 是消息、配置、权限和任务的事实来源；
3. Redis 只保存可过期、可重复、可丢失的协调状态；
4. Computer 只管理 Agent home、RuntimeSession 文件和 Engine 子进程；
5. WebView、OpenCode 和 Agent shim 都不持有数据库凭证。

## 2. PostgreSQL 关系

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
└── collab_command_requests

collab_runs
├── collab_run_deliveries
├── collab_triages
└── collab_command_requests

collab_engine_inventory
collab_schema_migrations
```

当前有 15 张业务表，另有一张 migration 记录表。所有时间都保存为不带时区的上海本地时间，与仓库数据库规范一致。

## 3. Participant 与 Agent

### 3.1 `collab_participants`

Participant 是消息作者、Room 成员、Card assignee 和来源字段的统一外键目标。

| 列 | 含义 |
|---|---|
| `id` | 主键；人类用户固定为 `local-user`，Agent 使用 Server 生成的 slug |
| `kind` | `user` 或 `agent` |
| `display_name` | 非空显示名 |
| `created_at` | 创建时间 |

首次迁移插入 `local-user / user / User`。数据库 trigger 禁止修改或删除这行。

### 3.2 `collab_agent_profiles`

| 列 | 含义 |
|---|---|
| `agent_id` | 主键，同时引用 Agent Participant |
| `role` | 可空展示职责 |
| `persona` | 用户可编辑且非空的人格部分 |
| `archived_at` | 可空；非空表示归档 |
| `created_at` / `updated_at` | 生命周期时间 |

归档而非物理删除保留历史消息、Run、Card、Climate 和 Agent home。

### 3.3 `collab_agent_runtime_configs`

每个 Agent 恰好一行：

| 列 | 含义 |
|---|---|
| `engine_id` | Engine adapter ID；当前生产值为 `opencode` |
| `main_model_id` | 正式 Turn 模型 |
| `triage_model_id` | triage/Agenda classifier 模型 |
| `agenda_enabled` | 默认 `false` |
| `config_revision` | 正整数；每次运行配置变化递增 |
| `updated_at` | 更新时间 |

profile 与 runtime config 分离：前者描述“这个 Agent 是谁”，后者描述“当前如何运行”。

### 3.4 `collab_agent_climates`

主键为 `(agent_id, about_participant_id)`：

- `affinity`、`trust` 都限制在 `[-1, 1]`；
- `last_note` 可空；
- 不能指向自己；
- 只保存当前状态；
- A→B 与 B→A 完全独立。

## 4. Room 与 Message

### 4.1 `collab_rooms`

| 列 | 约束 |
|---|---|
| `id` | `room-` + 完整 UUIDv4 十六进制 |
| `kind` | `direct` 或 `group` |
| `title` | Group 必填；Direct 必须为空 |
| `direct_key` | Direct 必填且全局唯一；Group 必须为空 |
| `next_seq` | 非负，事务内分配下一条 Message sequence |
| `last_message_at` | 最近消息时间，可空 |
| `created_by` | 创建者 Participant，不可空 |

Direct Room 的 key 由两个 Participant ID 排序后组成，因此并发首次 DM 仍只产生一间 Room。

### 4.2 `collab_room_members`

主键为 `(room_id, participant_id)`，另存：

- `last_read_seq`：durable inbox 的 settlement 游标；
- `muted`：成员级 wake 过滤；
- `joined_at`。

`last_read_seq` 只能由成功 settlement 推进，不能拿来保存短期 seen 状态。

### 4.3 `collab_messages`

| 列 | 约束 |
|---|---|
| `id` | `msg-` + 完整 UUIDv4 十六进制 |
| `room_id` / `author_id` | Room 与 Participant 外键 |
| `sequence` | 正整数；同 Room 唯一 |
| `kind` | `normal` 或 `system` |
| `body` | 非空 |
| `system_payload` | 仅 system Message 可用，且必须是 JSON object |

消息写入事务锁定 Room 行，增加 `next_seq`，插入 Message，更新 `last_message_at`。Redis invalidation 在事务提交后尽力发布。

## 5. Board、Column 与 Card

### 5.1 `collab_boards`

Board 不引用 Room，也不保存只有一个合法值的 workspace 外键。

- `id`：`board-` 前缀；
- `title`：非空；
- `description`：可空；
- `created_by`：不可变来源 Participant；
- `created_at` / `updated_at`。

创建 Board 与三个默认 Column 在同一事务中完成。

### 5.2 `collab_board_columns`

- `id`：`col-` 前缀；
- `board_id`：Board 外键；
- `title`：非空；
- `position`：非负；
- `is_terminal`：明确表达终态。

`(board_id, position)` 使用可延迟唯一约束，允许事务内先移动到临时位置再整体连续编号。`(board_id, id)` 复合唯一键供 Card 外键验证“Column 必须属于同一 Board”。

### 5.3 `collab_cards`

- `id`：`card-` 前缀；
- `board_id + column_id`：复合外键；
- `title`：非空；
- `description`：可空；
- `position`：非负；
- `assignee_id`：可空 Participant；
- `created_by`：不可变来源 Participant；
- `created_at` / `updated_at`。

`(column_id, position)` 同样使用可延迟唯一约束。Card self-assign 只原子写 `assignee_id`，不会建立第二个所有权字段。

删除规则：

- Desktop 可删除 Card；
- Column 只有没有 Card 时才能删除；
- Board 只有所有 Column 都没有 Card 时才能删除，删除时级联清理空 Column。

## 6. Run、delivery 与 triage

### 6.1 `collab_runs`

Run 保存一次正式 Agent 工作：

| 分类 | 列 |
|---|---|
| 身份与 fencing | `id`、`agent_id`、`runtime_session_id` |
| focus | 可空 `room_id`、可空 `focus_card_id` |
| trigger | `message` / `rerun` / `reconnect` / `poll` / `agenda` / `user` |
| 模型快照 | `engine_id`、主/triage 模型、`runtime_config_snapshot` |
| 状态 | `running` / `completed` / `failed` / `cancelled` / `interrupted` |
| 观测 | heartbeat、token delta、rate limit、错误、outcome |

约束保证：

- 每个 Agent 最多一条 running Run；
- running 没有 `ended_at`，终态必须有；
- completed 必须有 `acted` / `silent` / `unpublished` outcome；
- Agenda Run 必须有 Card 或 Room focus 以及非空原因；
- 非 Agenda Run 不能伪造 Agenda focus。

新 RuntimeSession 启动时把其他 session 残留的 running Run 标记为 interrupted。

### 6.2 `collab_run_deliveries`

主键为 `(run_id, room_id)`，记录本次 Run 携带的 `[from_seq, up_to_seq]`。

- `eligible_reason` 只能是 `action`、`ack` 或 `triage_false`；
- eligible 与时间必须同时出现；
- settled 只能发生在 eligible 之后；
- 失败、取消和中断不结算。

成功结算时 Server 依据 delivery 最大 sequence 推进对应成员的 `last_read_seq`。

### 6.3 `collab_triages`

Triage 记录 classifier 或确定性短路的输入范围、决定、来源、Engine/model、usage 和 latency。`run_id` 可空以保留已结束 Run 之外的决策；`runtime_session_id` 防止跨 session 混用。

## 7. 命令幂等与 Engine inventory

### 7.1 `collab_command_requests`

Desktop 和 Agent 写命令共用一张幂等表：

- `request_id` 使用 `req-` 前缀；
- `semantic_hash` 对结构化命令语义计算；
- Agent 命令以 `(run_id, request_id)` 唯一；
- Desktop 命令以 `(runtime_session_id, request_id)` 唯一；
- 已完成结果保存为 JSON object 并可原样重放；
- 相同 request ID 携带不同语义时冲突。

### 7.2 `collab_engine_inventory`

每个 Engine 一行最后观测：

- `status`：`unknown` / `ready` / `missing` / `error`；
- `version`、`checked_at`、`last_error`；
- `observed_session_id`：产生该观测的 RuntimeSession。

这张表只供展示 last-known observation。启动 Runner 还必须有 Computer 当前 session 的实时 probe 结果。

## 8. Redis

Redis key/channel 都在 `openwork:` namespace：

| namespace | 用途 | 典型 TTL/语义 |
|---|---|---|
| `openwork:message.new` | Message committed Pub/Sub | invalidation |
| `openwork:wake:<agent>` | per-Agent wake Pub/Sub | invalidation |
| `openwork:wake-claim:<message>` | scheduler dedupe | 60 秒 |
| `openwork:turn-rate:<agent>` | Agent-authored wake 限速 | 60 秒 |
| `openwork:seen:<agent>:<room>` | 发布新鲜度 sequence | 10 分钟 |
| `openwork:hold:<agent>:<room>:<token>` | 一次性 HELD binding | 2 分钟 |
| `openwork:agenda-rate:<agent>` | Agenda dispatch cooldown | 5 分钟 |
| `openwork:agenda-nudge:<room>` | Room nudge cooldown | 45 分钟 |
| `openwork:agenda-declines:<agent>` | 连续 decline 计数 | 6 小时 |

Redis 不保存消息正文、Agent config、Board、Run 或待执行 Agenda queue。Redis 清空或短暂不可用可能造成一次额外 poll/triage，不能造成 durable fact 丢失。

## 9. 本机文件

固定根目录为 `~/.openwork`：

```text
~/.openwork/
├── runtime.lock
├── agents/<agent-id>/
│   ├── AGENTS.md
│   ├── work/
│   └── engines/<engine-id>/session.json
└── runtime/<runtime-session-id>/
    ├── bin/openwork
    ├── agents/<agent-id>/runtime-token
    └── derived/<agent-id>/<engine-id>/
```

持久 Agent home 只保存受管 persona/协作契约、私有工作文件和最小 Engine continuity。RuntimeSession 目录只保存短期凭证与派生配置：启动清除陈旧目录，正常退出清除当前目录。

多个 Agent 的 `work` 彼此独立；它不是多个 Agent 共同操作同一个真实项目 checkout，也不是 OS 安全沙箱。

## 10. 事务与并发不变量

1. Room sequence 在锁定 Room 行后递增；
2. Direct Room 依靠唯一 key 抵抗并发创建；
3. Card self-assign 在事务中复核当前 assignee；
4. Board/Column/Card 操作统一按 Board → Column ID → Card ID 的固定顺序加锁；
5. Column/Card 重排使用可延迟唯一约束并重新写成连续整数；
6. 每 Agent running Run 依靠部分唯一索引兜底；
7. 命令幂等结果与业务写入位于同一事务；
8. delivery 只在成功终态按明确效果结算；
9. Climate owner 来自 Agent JWT，而不是客户端字段；
10. Redis 协调错误永远不能伪装成 PostgreSQL 事务成功。

## 11. 验收

- migration 可在全新隔离数据库一次建立全部 schema；
- `local-user` 无法更新或删除；
- 退役表和退役字段不存在；
- 两个 Agent 并发 self-assign 同一 Card 时只有一个成功；
- 并发 Card move 后 position 连续且无重复；
- Direct Room 并发创建仍只有一行；
- Climate owner-scoped 且方向独立；
- 失败/中断 Run 不推进 delivery；
- Redis 不可用时 Message 仍可从 inbox 读取；
- stale Engine observation 不会启动 Runner；
- runtime token 不进入持久 Agent home。
