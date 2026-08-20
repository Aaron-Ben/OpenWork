# 协作模式数据模型

`collab_*` 全部表的 DDL 与约束理由。**这是协作模式 schema 的唯一权威副本**——[collaboration.md](collaboration.md) 描述语义与边界，本篇描述落盘形状；正文里出现的任何列名以本篇为准。

工作台的表在 [data-model.md](data-model.md)，两套 schema **互不引用、互不阻塞**。

## 0. 硬约束

- 迁移目录 `crates/openwork-collab/migrations/`，**独立的版本表**，绝不与 `crates/openwork-core/migrations/` 混排；
- 时间口径完全沿用 [.claude/rules/database.md](../.claude/rules/database.md)：所有时间列 `TIMESTAMP WITHOUT TIME ZONE`，默认值 `AT TIME ZONE 'Asia/Shanghai'`，时长一律 `*_ms` 整数，出库序列化带 `+08:00`；
- **daemon 是唯一写者**（[collaboration.md §2](collaboration.md)）。下面所有"原子操作"都假定单写者，但仍用数据库约束兜底——写者只有一个不代表它不会写错。

## 1. 身份

### 1.1 为什么拆成两张表

消息作者、卡片指派、reaction 发起者**既可能是 Agent 也可能是用户**。写成 `(author_kind, author_id)` 多态列就拿不到外键，一个拼错的 id 要到渲染时才暴露。

因此：`collab_participants` 是全库唯一的引用目标，`collab_agents` 是它的 1:1 子类型，只放 Agent 专有列。创建一个 Agent 是同一事务里的两次插入。

```sql
CREATE TABLE collab_participants (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at   TIMESTAMP WITHOUT TIME ZONE NOT NULL
                 DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_participants_kind_valid
        CHECK (kind IN ('user', 'agent')),
    -- id 同时是 Agent 的 home 目录名与 OpenCode agent 名，
    -- 因此必须是文件系统与配置文件都安全的形状。与 sessions.task_name 同风格。
    CONSTRAINT collab_participants_id_format
        CHECK (id ~ '^[a-z][a-z0-9_]{0,47}$'),
    CONSTRAINT collab_participants_name_not_blank
        CHECK (btrim(display_name) <> '')
);
```

本机只有一个人类用户，迁移里种一行 `('user', 'user', ...)`。**不做多用户**——那会把整套 schema 变成多租户形状。

```sql
CREATE TABLE collab_agents (
    id                  TEXT PRIMARY KEY
                        REFERENCES collab_participants(id) ON DELETE CASCADE,
    role                TEXT,
    bio                 TEXT,
    system_prompt       TEXT NOT NULL,
    provider_id         TEXT NOT NULL,
    model_id            TEXT NOT NULL,
    -- 当前的 OpenCode session。放库里而不是 home 里：home 是 Agent 自己的
    -- 可写空间，放进去它自己就能改。见 collaboration.md §3.1。
    opencode_session_id TEXT,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    scanner_enabled     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);
```

`system_prompt` 可以是空字符串；共享人格底座依然会被渲染进 `AGENTS.md`，所以空人格不会退化成没有协作约束的引擎默认助手（[collaboration.md §3.4](collaboration.md)）。`scanner_enabled` 默认关闭，只能由用户显式开启。

**Agent 只停用不删除。** `enabled = FALSE` 让它退出所有唤醒候选，而历史消息与身份完好。真要删除时，下面所有指向 participants 的外键都是 `RESTRICT` 或 `SET NULL`，数据库会拦住会造成"消息没有作者"的删除。

## 2. 房间与消息

```sql
CREATE TABLE collab_rooms (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,
    title           TEXT,
    -- DM 的成员 id 排序后拼接。没有它，两个 Agent 同时 dm 同一个人
    -- 会建出两个房间——"由成员集合唯一确定"必须有一个显式的键来保证。
    direct_key      TEXT,
    next_seq        BIGINT NOT NULL DEFAULT 0,
    last_message_at TIMESTAMP WITHOUT TIME ZONE,
    created_at      TIMESTAMP WITHOUT TIME ZONE NOT NULL
                    DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at      TIMESTAMP WITHOUT TIME ZONE NOT NULL
                    DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_rooms_kind_valid CHECK (kind IN ('group', 'direct')),
    CONSTRAINT collab_rooms_shape_valid CHECK (
        (kind = 'direct' AND direct_key IS NOT NULL AND title IS NULL) OR
        (kind = 'group'  AND direct_key IS NULL     AND title IS NOT NULL)
    ),
    CONSTRAINT collab_rooms_next_seq_nonneg CHECK (next_seq >= 0)
);

CREATE UNIQUE INDEX uq_collab_rooms_direct
    ON collab_rooms(direct_key) WHERE direct_key IS NOT NULL;
```

```sql
CREATE TABLE collab_room_members (
    room_id        TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    -- inbox 查询的起点。seen 游标绝不能共用这一列，理由见 §6。
    last_read_seq  BIGINT NOT NULL DEFAULT 0,
    muted          BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at      TIMESTAMP WITHOUT TIME ZONE NOT NULL
                   DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (room_id, participant_id),
    CONSTRAINT collab_room_members_seq_nonneg CHECK (last_read_seq >= 0)
);
```

```sql
CREATE TABLE collab_messages (
    id             TEXT PRIMARY KEY,
    room_id        TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    sequence       BIGINT NOT NULL,
    author_id      TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    kind           TEXT NOT NULL DEFAULT 'normal',
    body           TEXT NOT NULL,
    -- 结构化的系统消息载荷（看板事件、成员变动）。让界面能渲染成可点击的
    -- 引用，而不是去正则解析正文。
    system_payload JSONB,
    created_at     TIMESTAMP WITHOUT TIME ZONE NOT NULL
                   DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_messages_kind_valid CHECK (kind IN ('normal', 'system')),
    CONSTRAINT collab_messages_seq_positive CHECK (sequence > 0),
    CONSTRAINT collab_messages_body_not_blank CHECK (btrim(body) <> ''),
    CONSTRAINT collab_messages_payload_only_system
        CHECK (system_payload IS NULL OR kind = 'system'),
    CONSTRAINT collab_messages_payload_is_object
        CHECK (system_payload IS NULL OR jsonb_typeof(system_payload) = 'object')
);

-- 这条唯一索引是 sequence 分配正确性的最后一道防线。
CREATE UNIQUE INDEX uq_collab_messages_room_seq ON collab_messages(room_id, sequence);
-- 房间视图从新往旧翻页。
CREATE INDEX idx_collab_messages_room_seq_desc ON collab_messages(room_id, sequence DESC);
```

```sql
CREATE TABLE collab_reactions (
    message_id TEXT NOT NULL REFERENCES collab_messages(id) ON DELETE CASCADE,
    actor_id   TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    emoji      TEXT NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (message_id, actor_id, emoji)
);
```

## 3. 看板

三层：房间 → 看板 → 列 → 卡片。

**为什么保留"看板"这一层。** 两层（列直接挂房间）现在少一张表，V1 也够用。但那样房间就**兼任**了看板，将来想在同一个房间里分出"Bug"和"路线图"两块时，要给每张卡加 `board_id` 并回填——一次涉及全部历史数据的迁移。`AGENTS.md` 要求"不接受只在当下有效、以后要替换的权宜之计"，一张空表比一次全量回填便宜得多。

```sql
CREATE TABLE collab_boards (
    id         TEXT PRIMARY KEY,
    room_id    TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_boards_title_not_blank CHECK (btrim(title) <> '')
);
CREATE INDEX idx_collab_boards_room ON collab_boards(room_id);

CREATE TABLE collab_board_columns (
    id       TEXT PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    title    TEXT NOT NULL,
    position INTEGER NOT NULL,
    -- 显式的完成标记。cumora 用正则猜列名是不是 done（DONE_COLUMN_PATTERNS），
    -- 那是内容分类，属于不该用正则做的判断。agenda 靠这一列决定哪些卡还算未完成。
    is_done  BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT collab_board_columns_title_not_blank CHECK (btrim(title) <> '')
);
CREATE UNIQUE INDEX uq_collab_board_columns_position
    ON collab_board_columns(board_id, position);

CREATE TABLE collab_cards (
    id          TEXT PRIMARY KEY,
    board_id    TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    column_id   TEXT NOT NULL REFERENCES collab_board_columns(id) ON DELETE RESTRICT,
    title       TEXT NOT NULL,
    description TEXT,
    position    INTEGER NOT NULL,
    assignee_id TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    claimed_by  TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    claimed_at  TIMESTAMP WITHOUT TIME ZONE,
    created_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_cards_title_not_blank CHECK (btrim(title) <> ''),
    CONSTRAINT collab_cards_claim_consistent CHECK (
        (claimed_by IS     NULL AND claimed_at IS     NULL) OR
        (claimed_by IS NOT NULL AND claimed_at IS NOT NULL)
    )
);
CREATE INDEX idx_collab_cards_column ON collab_cards(column_id, position);
CREATE INDEX idx_collab_cards_assignee
    ON collab_cards(assignee_id) WHERE assignee_id IS NOT NULL;
```

## 4. 设置

```sql
CREATE TABLE collab_settings (
    id                 TEXT PRIMARY KEY DEFAULT 'singleton',
    triage_provider_id TEXT,
    triage_model_id    TEXT,
    opencode_base_url  TEXT,
    updated_at         TIMESTAMP WITHOUT TIME ZONE NOT NULL
                       DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_settings_singleton CHECK (id = 'singleton'),
    -- provider 与 model 同生共死：只填一半的 triage 配置无法使用，
    -- 让数据库直接拒绝，而不是运行时报一个含糊的错。
    CONSTRAINT collab_settings_triage_pair CHECK (
        (triage_provider_id IS     NULL AND triage_model_id IS     NULL) OR
        (triage_provider_id IS NOT NULL AND triage_model_id IS NOT NULL)
    )
);
```

单行表而不是 KV 表：配置项是有类型的，KV 会把类型检查推到运行时。

## 5. 观测

三张平表，**不是 Trace**——没有 Span 树、没有父子指针、没有完整度派生（[collaboration.md §12](collaboration.md)）。

```sql
CREATE TABLE collab_runs (
    id                  TEXT PRIMARY KEY,
    agent_id            TEXT NOT NULL REFERENCES collab_agents(id) ON DELETE CASCADE,
    room_id             TEXT REFERENCES collab_rooms(id) ON DELETE SET NULL,
    trigger             TEXT NOT NULL,
    status              TEXT NOT NULL,
    started_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    ended_at            TIMESTAMP WITHOUT TIME ZONE,
    provider_id         TEXT,
    model_id            TEXT,
    input_tokens        BIGINT,
    cached_input_tokens BIGINT,
    output_tokens       BIGINT,
    rate_limit_percent  DOUBLE PRECISION,
    error_code          TEXT,
    error_message       TEXT,
    outcome             TEXT,
    CONSTRAINT collab_runs_trigger_valid CHECK (
        trigger IN ('message', 'rerun', 'idle', 'agenda', 'scanner', 'user')
    ),
    CONSTRAINT collab_runs_status_valid CHECK (
        status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    CONSTRAINT collab_runs_outcome_valid CHECK (
        outcome IS NULL OR outcome IN ('acted', 'silent', 'unpublished', 'unknown')
    ),
    -- outcome 只在 completed 上有意义：running 还没有结论；
    -- failed / cancelled / interrupted 的那一轮没跑完，判不出它是主动闭嘴还是被打断。
    -- 这条约束同时让"跑完了却没派生 outcome"成为不可能状态。
    CONSTRAINT collab_runs_outcome_scope CHECK (
        (status =  'completed' AND outcome IS NOT NULL) OR
        (status <> 'completed' AND outcome IS     NULL)
    ),
    -- .claude/rules/database.md §1.3 的硬要求：终态必有 ended_at，运行中必无。
    CONSTRAINT collab_runs_terminal_time_valid CHECK (
        (status =  'running' AND ended_at IS     NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT collab_runs_end_after_start CHECK (
        ended_at IS NULL OR ended_at >= started_at
    )
);
CREATE INDEX idx_collab_runs_agent_started ON collab_runs(agent_id, started_at DESC);
```

**usage 落差量不落累计值。** 引擎报的是会话累计，每轮取差；直接存累计会让"这一轮花了多少"永远算不出来。

**`outcome` 由 daemon 派生，不由 Agent 上报。** 语义与理由见 [collaboration.md §8.3](collaboration.md)：调过 `reply` / `react` / `card` 是 `acted`；未发布但调过 `ack` 是 `silent`；一个结算工具都没调时，正文近乎为空是 `silent`，吐了大段正文才是 `unpublished`。判据是 daemon 的一手事实——它既是全部 MCP 调用的接收方，也在消费事件流里的 assistant 正文——所以这一列不花任何 token。

`unknown` 只用于缺少 outcome 证据的旧完成轮次；把它们编成 `acted` 或 `silent` 是凭空造事实。新写入的行不允许 `unknown`。

```sql
CREATE TABLE collab_triages (
    id            TEXT PRIMARY KEY,
    agent_id      TEXT NOT NULL REFERENCES collab_agents(id) ON DELETE CASCADE,
    room_id       TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    up_to_seq     BIGINT NOT NULL,
    actionable    BOOLEAN NOT NULL,
    response_mode TEXT,
    source        TEXT NOT NULL,
    reason        TEXT,
    prompt_note   TEXT,
    provider_id   TEXT,
    model_id      TEXT,
    input_tokens  BIGINT,
    output_tokens BIGINT,
    latency_ms    BIGINT,
    created_at    TIMESTAMP WITHOUT TIME ZONE NOT NULL
                  DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_triages_mode_valid CHECK (
        response_mode IS NULL OR response_mode IN ('me', 'each', 'one_of_us')
    ),
    -- source 区分"模型判的"与"服务端在调模型之前就短路的"，
    -- 后者是 collaboration.md §11.2 三道防死循环的可观测证据。
    CONSTRAINT collab_triages_source_valid CHECK (source IN (
        'empty_inbox', 'system_only', 'rate_limited', 'loop_cap',
        'support_model', 'fail_open', 'fail_closed', 'human_dm', 'dm_agent_engage'
    ))
);
CREATE INDEX idx_collab_triages_created ON collab_triages(created_at);
```

```sql
CREATE TABLE collab_events (
    id         TEXT PRIMARY KEY,
    run_id     TEXT REFERENCES collab_runs(id) ON DELETE CASCADE,
    agent_id   TEXT REFERENCES collab_agents(id) ON DELETE CASCADE,
    room_id    TEXT REFERENCES collab_rooms(id) ON DELETE SET NULL,
    kind       TEXT NOT NULL,
    payload    JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_events_payload_is_object CHECK (jsonb_typeof(payload) = 'object')
);
CREATE INDEX idx_collab_events_created ON collab_events(created_at);
CREATE INDEX idx_collab_events_run ON collab_events(run_id) WHERE run_id IS NOT NULL;
```

`kind` **不加 CHECK**：事件种类会随 OpenCode 的事件流演进，把它写死成约束会让每加一种事件都要一次迁移。观测表的容错优先于取值收敛。

## 6. 不建表的东西

| 状态 | 放哪 | 为什么不落库 |
|---|---|---|
| seen 游标 | daemon 内存，TTL 10 分钟 | 协调信号不是正确性不变量，fail-open；**且绝不能与 `last_read_seq` 共用**——那一列是 inbox 的 SELECT 游标，一旦被 seen 推进，下次 inbox 就返回空，daemon 挂成 silent-busy（cumora `a6e69aa` 的真实事故） |
| HELD token | daemon 内存，TTL 120 秒 | 只在"HELD → 重读 → 重发"这一口气里有意义，长 TTL 会变成未来的绕过弹药 |
| 停滞推动的 claim 与 decline 计数 | daemon 内存 | 窗口级状态，重启后最多多推一次 |
| Agent 的 MCP token | daemon 内存 | 与 daemon 进程同生共死，不需要 TTL 或刷新 |
| 未读数 | 查询 | `last_read_seq` 与房间最高 `sequence` 的差，不是存储 |
| 待审批 | OpenCode + daemon 内存 | 单条 `GET /global/event` 维护跨 Agent 的待决集合；`GET /permission` 是 instance 范围的，只用于按 Agent 查询 |
| Agent 记忆 | `<home>/memory/MEMORY.md` | 文件，Agent 自己维护 |

## 7. 写入顺序与原子操作

### 7.1 发一条消息

**一个事务**，顺序不可换：

```sql
-- 1. 取行锁并分配 sequence（这一步同时把该房间的所有插入串行化）
UPDATE collab_rooms SET next_seq = next_seq + 1, updated_at = ...
 WHERE id = $1 RETURNING next_seq;
-- 2. HELD 新鲜度预检（房间成员 > 2 时才做）与逐字去重
-- 3. INSERT INTO collab_messages ...
-- 4. UPDATE collab_rooms SET last_message_at = ...
```

第 1 步的行锁是第 2 步能安全"先查后写"的**唯一**理由：单写者进程内可能有多个并发请求，没有这把锁，去重预检就是一个竞态。

### 7.2 认领一张卡

一次原子 UPDATE，不是先查后写：

```sql
UPDATE collab_cards
   SET claimed_by = $1, claimed_at = $2, updated_at = $2
 WHERE id = $3 AND claimed_by IS NULL
 RETURNING id;
```

返回 0 行 = 队友已认领，Agent 换一件事做。

### 7.3 认领什么时候释放

**按会话状态派生，不设 TTL。** TTL 到期时无法区分"Agent 死了"和"Agent 在做一件耗时长的事"，而后者被抢走会导致两边重复做同一件工作。

| 时机 | 动作 | 留痕 |
|---|---|---|
| daemon 启动 | 释放**全部**认领——此时所有引擎会话必然已断 | 只记日志 |
| 周期检查 | `GET /session/status` 显示认领者的会话已不在运行，且超过宽限期 → 释放 | 房间 `system` 消息 |

**宽限期不是可有可无的。** 会话状态有观测延迟，没有宽限期会把一个刚要开工的 Agent 的认领判成死的，**卡片被抢走后两个 Agent 会同时做同一件事**——而那正是 claim 要消除的东西。具体秒数属于运维参数，记在 crate 的 README。

| 用户操作 | 看板 UI 上强制取消认领 | 房间 `system` 消息 |

释放走同样的条件更新（`WHERE claimed_by = $1`）。

**留痕分两档是有意的。** 启动时的批量释放是例行事件，一次可能涉及 N 张卡，发成房间消息就是刷屏；而运行中因会话死亡的释放是意外，用户需要知道"这张卡为什么又没人认领了"，那属于房间里该被看见的事。

三种释放都同时发一个内存态的变更事件供 Desktop 实时刷新。**`collab_events` 的持久化记录要到 P6**（§9）——在那之前不要引用这张还不存在的表。

### 7.4 启动修正

daemon 启动时，除释放认领外还要把所有 `status = 'running'` 的 `collab_runs` 改成 `interrupted` 并补 `ended_at`——进程已经换了，那些 run 不可能再有人去结束它们。这与工作台的 `mark_running_interrupted()` 同一原则：修复记录的可理解性，不恢复执行。

## 8. 保留期

`collab_events` 与 `collab_triages` 会持续增长——主动性开着的话，一个没有人的夜晚也在写 triage 记录。**daemon 必须自带 GC worker**（P6），按天数保留、小批量删除、每批设 `statement_timeout`，与工作台 `db-gc` 同形态但独立运行。

`collab_messages` **不清理**。它是房间的历史，删了就没了。

## 9. 分期落地

| 期 | 建表 |
|---|---|
| **P1** | `collab_participants` `collab_agents` `collab_rooms` `collab_room_members` `collab_messages` `collab_settings` `collab_runs` |
| **P3** | `collab_triages` `collab_reactions` |
| **P4** | `collab_boards` `collab_board_columns` `collab_cards` |
| **P5** | 不建新表；`collab_agents` 加 `scanner_enabled` 列 |
| **P6** | `collab_events` |
| **横切验收** | 不建新表；`collab_runs` 加 `outcome` 列（回填 + 两条 CHECK） |

每个迁移必须能在**空库**上从头跑通，不依赖任何手工修复过的状态。
