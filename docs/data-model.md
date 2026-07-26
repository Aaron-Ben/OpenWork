# 数据模型

PostgreSQL 单一持久化。`crates/openwork-core/migrations/` 是 schema 的**唯一事实来源**，本文解释形状和理由。

时间字段的写法、Rust 侧类型和迁移规范见 [.claude/rules/database.md](../.claude/rules/database.md)。

## 1. 表

| 表 | 职责 | 是业务真相吗 |
|---|---|---|
| `_sqlx_migrations` | SQLx 版本与校验和 | 基础设施 |
| `provider_credentials` | Provider 元数据与加密凭证 | 是（敏感） |
| `models` | 可选择的模型端点 | 是 |
| `sessions` | Session 元数据 | 是 |
| `turns` | 一次用户运行的状态与汇总 | 是 |
| `messages` | 完整原始消息 | 是 |
| `conversation_compactions` | 压缩投影元数据 | 是 |
| `trace_spans` | 质量追踪的骨架 | **否，best-effort** |
| `trace_payloads` | 去重后的正文 | 否，可重建不了但可丢 |
| `trace_span_payloads` | Span 与正文的挂载 | 否 |
| `trace_annotations` | **人对一次运行的判断** | **是** |

```text
Provider Credential ← Model.credential_ref
Model ← Session.default_model_id, Turn.model_id, TraceSpan.model_id
Session
├── Turn ── Message ← TraceSpan.response_message_id
├── ConversationCompaction
├── TraceSpan ── child TraceSpan
│   └── TraceSpanPayload ── TracePayload（按哈希去重，跨 Session 共享）
└── TraceAnnotation
```

`trace_annotations` 是 Trace 家族里**唯一的业务真相**：其余三张丢了只是排查变难，标注丢了是用户的输入丢了。由此推出保留策略的例外，见 [trace.md](trace.md) §14。

**不建立**：明文凭证、`recorded_events`、`steps`、`tool_runs`、`approvals`、`runtime_states`、`session_updates`、`turn_recovery_checkpoints`、`trace_span_events`、评测集与自动打分表。

## 2. 四条设计原则

### 一、Message 只增不改不删

压缩**不删除任何消息**，只新增一条 `conversation_compactions`，用序号划出"哪一段被摘要替换了"。模型看到投影，库里原文始终在。

这是 rewind 和原文回读的前提。**任何"压缩时顺便清理旧消息"的优化都会同时废掉这两个能力。**

### 二、状态机由数据库守

```sql
CHECK ((status = 'running' AND ended_at IS NULL) OR
       (status <> 'running' AND ended_at IS NOT NULL))

CREATE UNIQUE INDEX uq_turns_one_running_per_session
    ON turns(session_id) WHERE status = 'running';
```

这两条必须和启动时的清扫（§7）配成闭环：崩溃遗留的 `running` 行若不清扫，那个部分唯一索引会让该 Session **再也插不进新 Turn**。**约束和清扫是一对，只加其一比都不加更糟。**

### 三、跨实体引用用复合外键

```sql
FOREIGN KEY (turn_id, session_id) REFERENCES turns(id, session_id)
```

而不是单列。这样消息、Span、压缩记录都不可能挂到别的 Session 的 Turn 上。代价是被引用侧要有 `(id, session_id)` 唯一约束，值得。

**反面：不要为了外键而造冗余索引。** 若某外键需要三列（`id, session_id, sequence`），被引用表就得建一个包含主键的唯一索引——而任何包含主键的列组合天然唯一，该索引提供零额外约束能力，却要在高频写入表上一直维护。这种情况应降级成单列外键。

### 四、活动状态不落库

pending permission、当前 phase、草稿只在 `SessionActor` 内存里。数据库只保存**已完成的事实**。因此不恢复未完成的 Turn。

### 五、同一份内容只有一个权威副本

`messages` 是内容的业务真相，只增不改不删，永远比 Trace 完整。Trace **不复制它已有的内容**，只留指针：

```sql
trace_spans.response_message_id  → messages(id)          -- 成功调用的响应
attributes.checkpointId          → conversation_compactions(id)  -- 成功的摘要
（Tool Call 的参数与结果由 (turn_id, provider_call_id) 定位，不需要新列）
```

理由不是省空间，是**避免同一份内容存在两个可能不一致的版本**——一旦不一致，没人知道该信哪个。

Trace 只保存 `messages` 回答不了的东西：**组装后的请求**（投影后的 Conversation + System Context + 工具定义）和**失败调用的响应**。前者是"模型实际看到了什么"的唯一答案，压缩之后它和原始消息不再相同；后者从未产生 Message。

详见 [trace.md](trace.md) §6。

## 3. 业务实体与 Trace 的分工

| | 业务实体 | Trace 三表 |
|---|---|---|
| 回答 | 发生了什么 | 模型看到了什么、说了什么、烧了多少 token |
| 写入 | 事务 + 外键 | 有界队列，可丢 |
| 丢失后果 | 数据丢失 | 排查变难 |
| 生命周期 | 业务决定 | 跟随 Session 级联删除 + 保留策略 |

**唯一的交叉点**是刻意的冗余：**三个计数器**（`model_call_count` / `model_submission_count` / `tool_call_count`）由业务写入路径维护，与 Trace 写入路径完全独立，两者一致才判定 Trace 完整。若把 captured 改成从 Span 派生，对账永远相等，完整度检测失去意义。

`trace_annotations` 不在这张表的两侧——它是长在 Trace 上的业务真相，见 §1。

## 4. ID 与时间

- ID 是应用生成的稳定 `TEXT`，允许带前缀；全部校验 `btrim(id) <> ''`；
- 时间列一律 `TIMESTAMP WITHOUT TIME ZONE`，存**东八区墙上时间**，默认值 `CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'`；
- API 返回补 `+08:00`，**绝不能补 `Z`**——库里已是东八区，标成 UTC 会让前端再换算一次，最终偏 16 小时且不报错；
- `created_at` 写入后不修改，更新业务字段时必须同时更新 `updated_at`；
- Turn 与 Message 的顺序用 `sequence` 表达并由唯一约束兜底；**Trace Span 不参与这套编号**，按 `started_at` 排序（见 §6）。

## 5. 业务表

### provider_credentials / models

```sql
CREATE TABLE provider_credentials (
    provider_id        TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    provider_kind      TEXT NOT NULL,
    base_url           TEXT NOT NULL,
    api_key_encrypted  TEXT NOT NULL,
    enabled            BOOLEAN NOT NULL DEFAULT TRUE,
    config             JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at / updated_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT provider_credentials_kind_valid
        CHECK (provider_kind IN ('openai','anthropic','deepseek','kimi','qwen','glm')),
    -- 其余：各字段非空白、config 必须是 object
);
```

密文用 `provider_id` 作为 AAD。Repository **不返回密文字段**，只返回公开 Profile 或调用边界解密后的零化凭证类型。

`models` 一行是一个可直接选择的端点。`provider_kind` 使用**与上表相同**的 CHECK——同一概念不该在两张表约束强度不一致。

`credential_ref` 是**多态引用**：`provider:<provider_id>` 指向 `provider_credentials`，其余值是环境变量名。因此**不能建外键**，解析方是 `ProviderCredentialResolver`。

`config` 只保存低频 Provider 选项。禁止保存：API Key 明文、完整请求/响应、Session/Turn 状态、能由代码默认值表达的字段。

### sessions

```sql
CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,
    title               TEXT,
    working_directory   TEXT NOT NULL,
    default_model_id    TEXT REFERENCES models(id) ON DELETE SET NULL,
    status              TEXT NOT NULL DEFAULT 'active',   -- active | archived
    created_at / updated_at / last_turn_at
);
```

`working_directory` 是工具执行根目录，可以不是 Git 仓库。**不为它建立 Workspace 记录、Trust 状态或 Git 元数据。**

Session 不保存 `runtime_state`、pending permission 或当前 Tool Call。

### turns

```sql
CREATE TABLE turns (
    id                      TEXT PRIMARY KEY,
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    client_request_id       TEXT NOT NULL,
    sequence                BIGINT NOT NULL,
    model_id                TEXT REFERENCES models(id) ON DELETE SET NULL,
    resolved_provider_kind  TEXT NOT NULL,
    resolved_model_name     TEXT NOT NULL,
    app_version             TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_call_count        INTEGER NOT NULL DEFAULT 0,
    model_submission_count  INTEGER NOT NULL DEFAULT 0,
    tool_call_count         INTEGER NOT NULL DEFAULT 0,
    input_tokens / output_tokens / cached_input_tokens / reasoning_tokens  BIGINT,
    error_code / error_message  TEXT,
    started_at / ended_at / updated_at,

    CONSTRAINT turns_sequence_positive CHECK (sequence > 0),
    CONSTRAINT turns_app_version_not_blank CHECK (btrim(app_version) <> ''),
    CONSTRAINT turns_status_valid
        CHECK (status IN ('running','completed','failed','cancelled','interrupted')),
    CONSTRAINT turns_model_submission_covers_logical_calls
        CHECK (model_submission_count >= model_call_count),
    CONSTRAINT turns_terminal_time_valid CHECK (
        (status = 'running' AND ended_at IS NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)),
    CONSTRAINT turns_end_after_start CHECK (ended_at IS NULL OR ended_at >= started_at),
    CONSTRAINT turns_id_session UNIQUE (id, session_id),
    CONSTRAINT turns_session_sequence UNIQUE (session_id, sequence),
    CONSTRAINT turns_client_request UNIQUE (session_id, client_request_id)
);

CREATE UNIQUE INDEX uq_turns_one_running_per_session
    ON turns(session_id) WHERE status = 'running';
```

两个计数的区别：`model_call_count` 是 Agent Loop 的**逻辑**轮次；`model_submission_count` 是实际发出的 provider-neutral 请求次数。采样前 threshold 压缩发生在提交之前，不增加 submission；overflow 后同一逻辑轮次的重提交只增加 submission。

**不设 `total_tokens` 生成列。** "总量 = 输入 + 输出"是跨 provider 未统一的计费口径（reasoning 是否计入 output、input 是否已含 cached，各家不同），冻结在 `GENERATED ALWAYS ... STORED` 里是最难改的形态。由查询或 Rust 侧计算。

### messages

```sql
CREATE TABLE messages (
    id                      TEXT PRIMARY KEY,
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id                 TEXT,
    sequence                BIGINT NOT NULL,
    role                    TEXT NOT NULL,
    content                 JSONB NOT NULL,
    content_format_version  SMALLINT NOT NULL DEFAULT 1,
    provider_call_id / tool_name  TEXT,
    created_at,

    CONSTRAINT messages_turn_session_fk
        FOREIGN KEY (turn_id, session_id) REFERENCES turns(id, session_id) ON DELETE CASCADE,
    CONSTRAINT messages_role_valid CHECK (role IN ('system','user','assistant','tool')),
    CONSTRAINT messages_content_is_array CHECK (jsonb_typeof(content) = 'array'),
    CONSTRAINT messages_content_format_positive CHECK (content_format_version > 0),
    CONSTRAINT messages_turn_required CHECK (turn_id IS NOT NULL OR role = 'system'),
    CONSTRAINT messages_tool_identity_valid CHECK (
        (role = 'tool' AND provider_call_id IS NOT NULL AND tool_name IS NOT NULL) OR
        (role <> 'tool' AND provider_call_id IS NULL AND tool_name IS NULL)),
    CONSTRAINT messages_session_sequence UNIQUE (session_id, sequence)
);

CREATE UNIQUE INDEX uq_messages_tool_result
    ON messages(turn_id, provider_call_id) WHERE role = 'tool';
```

`content_format_version` 是**产品最核心持久化事实的版本标记**。`content` 的唯一结构约束只有"它是个数组"——`ContentBlock` 形状一旦变化，没有这一列就无法区分新旧行，也无法写针对性回填。

`uq_messages_tool_result` 保证一个 Turn 下同一 Provider Tool Call 只有一个结果。

### conversation_compactions

关键字段与约束：

```sql
kind                                TEXT NOT NULL   -- manual|threshold|overflow|rewind
through_message_sequence            BIGINT NOT NULL -- 事实边界
replaced_through_message_sequence   BIGINT NOT NULL -- 安装边界
last_user_message_id                TEXT REFERENCES messages(id)
last_user_message_sequence          BIGINT
summary                             TEXT NOT NULL
summary_format_version              SMALLINT NOT NULL
runtime_state                       JSONB NOT NULL
runtime_reminder                    TEXT NOT NULL
runtime_reminder_format_version     SMALLINT NOT NULL
trigger_turn_id                     TEXT
parent_compaction_id                TEXT

CONSTRAINT ..._replacement_covers_source
    CHECK (replaced_through_message_sequence >= through_message_sequence),
CONSTRAINT ..._trigger_shape CHECK (
    (kind IN ('threshold','overflow') AND trigger_turn_id IS NOT NULL) OR
    (kind NOT IN ('threshold','overflow') AND trigger_turn_id IS NULL)),
CONSTRAINT ..._parent_shape CHECK (
    (kind = 'rewind' AND parent_compaction_id IS NOT NULL) OR
    (kind <> 'rewind' AND parent_compaction_id IS NULL))
```

后两条 CHECK 让数据库直接拒绝"手动压缩却关联了触发 Turn"这类不可能状态。

三个 `*_format_version` 分别覆盖 checkpoint 结构、摘要格式和提醒格式，可以独立演进。

语义见 [compaction.md](compaction.md)。

## 6. Trace 三表 + 标注

### trace_spans

```sql
CREATE TABLE trace_spans (
    id                      TEXT PRIMARY KEY,
    trace_id                TEXT NOT NULL,          -- 结构根，刻意无外键
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id                 TEXT,                    -- 业务标签，可空
    parent_span_id          TEXT,                    -- 刻意无外键
    kind / name / status    TEXT NOT NULL,
    model_id / resolved_model_name / provider_request_id,
    provider_call_id / requested_tool_name / resolved_tool_name,
    attempt_count / input_tokens / output_tokens
        / cached_input_tokens / reasoning_tokens / permission_wait_ms,
    response_message_id     TEXT REFERENCES messages(id) ON DELETE SET NULL,
    started_at / ended_at,
    error_code / error_message,
    attributes              JSONB NOT NULL DEFAULT '{}'::jsonb,

    CONSTRAINT trace_spans_trace_not_blank CHECK (btrim(trace_id) <> ''),
    CONSTRAINT trace_spans_turn_session_fk
        FOREIGN KEY (turn_id, session_id) REFERENCES turns(id, session_id) ON DELETE CASCADE,
    CONSTRAINT trace_spans_kind_valid
        CHECK (kind IN ('model_call','tool_call','compaction')),
    CONSTRAINT trace_spans_tool_columns_scoped CHECK (
        kind = 'tool_call' OR (
            provider_call_id IS NULL AND requested_tool_name IS NULL AND
            resolved_tool_name IS NULL AND permission_wait_ms IS NULL)),
    CONSTRAINT trace_spans_response_message_scoped CHECK (
        response_message_id IS NULL OR kind = 'model_call'),
    -- 状态、终态时间、token 非负、attributes 是 object
);

CREATE INDEX idx_trace_spans_trace_started   ON trace_spans(trace_id, started_at);
CREATE INDEX idx_trace_spans_session_started ON trace_spans(session_id, started_at DESC);
CREATE INDEX idx_trace_spans_turn_started    ON trace_spans(turn_id, started_at)
    WHERE turn_id IS NOT NULL;
```

**结构根是 `trace_id`，不是 `turn_id`。** `turn_id` 是指向 `turns` 的外键，而 `turns` 有业务生命周期和会话轮次编号，无法给"不属于任何 Turn 的操作"发号。详见 [trace.md](trace.md)。

三条反直觉但有意为之的设计：

- **没有 `sequence` 列**，排序用 `started_at`（`id` 兜底）。序号需要全局分配器，配上 `UNIQUE (turn_id, sequence)` 就把"同时只有一个执行体在写"编码进了约束；并发时冲突会让整批事务回滚，**一次静默丢失最多 64 条 Span**。
- **`parent_span_id` 不建外键。** Trace 写入有损，父 Span 可能根本没落库；建外键会让子 Span 一并失败，把单点丢失放大成级联丢失。孤儿在读取时统计成完整性信号。
- **不存 `total_tokens` 生成列**，理由同 `turns`。

### trace_payloads / trace_span_payloads

```sql
CREATE TABLE trace_payloads (
    hash        TEXT PRIMARY KEY,       -- 内容哈希
    body        JSONB NOT NULL,
    byte_size   BIGINT NOT NULL,
    created_at
);

CREATE TABLE trace_span_payloads (
    span_id             TEXT NOT NULL REFERENCES trace_spans(id) ON DELETE CASCADE,
    slot                TEXT NOT NULL,  -- request|system_context|tool_definitions|response
    payload_hash        TEXT NOT NULL REFERENCES trace_payloads(hash) ON DELETE RESTRICT,
    truncated           BOOLEAN NOT NULL DEFAULT FALSE,
    original_byte_size  BIGINT,
    redacted_count      INTEGER NOT NULL DEFAULT 0,   -- 待删，见下
    PRIMARY KEY (span_id, slot),
    CONSTRAINT trace_span_payloads_truncation_shape CHECK (
        (truncated = FALSE AND original_byte_size IS NULL) OR
        (truncated = TRUE  AND original_byte_size IS NOT NULL AND original_byte_size >= 0))
);

CREATE INDEX idx_trace_span_payloads_hash ON trace_span_payloads(payload_hash);
```

**拆成两张表是为了去重。** System Context 和工具定义在一个 Session 内几乎不变，却随每次 Model Call 重复发送：20 KB 的工具定义在 400 次调用后按行存是 8 MB，按哈希存是 20 KB。

**`trace_payloads` 没有 `session_id`** —— 同样的工具定义本就跨 Session 相同，加上它等于放弃去重。三个后果必须一起接受：

1. 删除 Session **不**级联删除正文，必须由孤儿清扫收尾，而且**必须在删除的同一次操作里执行**——这是隐私必做项，不是空间优化；
2. `payload_hash` 用 `ON DELETE RESTRICT`，让清扫无法误删仍被引用的正文；
3. 清扫走 `idx_trace_span_payloads_hash` 的 `NOT EXISTS`。

**`span_id` 建了外键，和 `parent_span_id` 不建并不矛盾**：后者指向另一个可能被独立丢弃的 Span，前者指向同批写入的自己。判别法是"这个引用指向的行，有没有可能在被引用时还不存在或已经丢了"。

**截断了就必须说明原始多大**，由 CHECK 强制——界面上一个无法量化的"已截断"警告没有用。

**`redacted_count` 是一个应当删除的列。** 它的本意是"这份正文里剔除了 N 处敏感字段"，但我们**刻意记录用户的私有代码**、不对正文做内容扫描，所以没有任何东西会让它非零——实现里它被硬编码成 `0`。

保留它只会让读 schema 的人以为存在一套脱敏机制。**下次因别的原因修改 initial migration 时一并删掉**，不为它单独改一次 schema。这是设计遗留，不是待实现功能。

### trace_annotations

```sql
CREATE TABLE trace_annotations (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    trace_id    TEXT NOT NULL,
    span_id     TEXT REFERENCES trace_spans(id) ON DELETE CASCADE,  -- 空 = 评价整条 Trace
    rating      TEXT NOT NULL,      -- good | bad | unsure
    note        TEXT,
    created_at / updated_at
);

CREATE UNIQUE INDEX uq_trace_annotations_target
    ON trace_annotations(trace_id, COALESCE(span_id, ''));
```

`COALESCE` 让"整条 Trace 的标注"也受唯一约束——普通唯一索引对 `NULL` 不生效，会允许无限条。

改评价是 **upsert**，不是追加一条相反的。

这是 Trace 家族里唯一丢不起的表，因此**带标注的 Trace 不参与保留策略的自动清理**。

## 7. 写入顺序

### 开始 Turn（一个事务）

1. 锁定 Session 或依赖 `(session_id, sequence)` 唯一约束分配序号；
2. 插入 `turns(status='running')`；
3. 插入 User Message；
4. 更新 `sessions.last_turn_at / updated_at`；
5. **提交后才调用模型。**

### 完成一次 Model Call

1. 调用 Provider 前更新 `model_call_count = GREATEST(...)` 并累加 `model_submission_count`（同轮 overflow 重提交只增后者）；
2. 流式草稿只在内存；
3. 响应完整后插入 Assistant Message；
4. 同一事务累加 Token，并按响应中的调用数累加 `tool_call_count`；
5. **事务提交后才执行 Tool Call**；
6. Model Span 独立 best-effort 结束。

第 5 条保证"数据库尚未保存模型要求执行什么"时不会先产生工具副作用。

### 完成一次 Tool Call

工具执行 → 形成结果 Message → 插入 → **提交成功后才进入下一次 Model Call** → Span 独立结束。

若副作用已发生但 Message 写入失败，Turn 失败。重启后保持 `interrupted`/`outcome_unknown`，**不自动执行同一工具**。

### 结束 Turn

更新 `status`、`ended_at`、最终汇总、可选错误。最终回答读取最后一条 Assistant Message，**不在 `turns` 重复存一份**。

## 8. 启动修正

Migration 之后、接受新 Turn 之前执行一次：

```sql
UPDATE turns SET status = 'interrupted',
    ended_at = COALESCE(ended_at, CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    error_code = COALESCE(error_code, 'process_interrupted'), ...
WHERE status = 'running';

UPDATE trace_spans SET status = 'outcome_unknown', ... WHERE status = 'running';
```

随后读取刚被中断的 Turn：从 Assistant Message 提取完整 Tool Call → 用 `(turn_id, provider_call_id)` 查已有结果 → 对没有结果的调用按原顺序追加合成 Message，状态 `outcome_unknown`，文本明确说明副作用可能已发生且不得自动重试 → 提交后才允许该 Session 接受新 Turn。

**这只是状态与 Conversation 完整性收口**，不调度恢复任务，也不读 Trace 判断工具是否执行过。

同一个 bootstrap 阶段还按 `OpenWorkCoreConfig.trace_content.retention_days`（默认 30 天）删除过期且不带标注的 `trace_span_payloads` 映射，再用 §9 的候选哈希清扫删除无人引用的正文 body。过期按 `trace_spans.started_at` 判断；清理在 Recorder 启动与首个 Turn 被接受之前完成，不增加后台定时任务。

## 9. 常用读取

```sql
-- 原始聊天记录（不受压缩影响）
SELECT id, turn_id, sequence, role, content, provider_call_id, tool_name, created_at
FROM messages WHERE session_id = $1 ORDER BY sequence;

-- Turn Trace
SELECT * FROM trace_spans WHERE turn_id = $1 ORDER BY started_at, id;

-- Session 压缩历史（含手动压缩与 rewind——它们没有 Turn，上面那条查不到）
SELECT * FROM trace_spans
WHERE session_id = $1 AND kind = 'compaction'
ORDER BY started_at DESC, id LIMIT $2;

-- 单个正文，按需加载。绝不放进上面两条查询里。
SELECT p.body, p.byte_size, m.truncated, m.original_byte_size
FROM trace_span_payloads m JOIN trace_payloads p ON p.hash = m.payload_hash
WHERE m.span_id = $1 AND m.slot = $2;

-- 孤儿正文清扫：与删除 Session 或过期 mapping 同一事务，且只清理候选哈希。
-- 全表 NOT EXISTS 扫描在并发下会误删刚插入、mapping 尚未挂载的正文，见 trace.md §14。
DELETE FROM trace_payloads p
WHERE p.hash = ANY($2)
  AND NOT EXISTS (SELECT 1 FROM trace_span_payloads m WHERE m.payload_hash = p.hash);
```

**正文必须是独立的按需查询。** 把它并进 Turn Trace 会让打开一个 Turn 就拉走几 MB JSONB，而用户多数时候只想看时间线。

**Token 用量不要跨 Provider 直接相加。** `cached_input_tokens` 是否已含在 `input_tokens` 里各家不同，聚合前必须按 `resolved_provider_kind` 分组，见 [trace.md](trace.md) §7。

模型可见 Conversation 读取 latest checkpoint、精确加载 last-user 原始 Message、放入冻结的 summary 与 reminder，最后追加大于 `replaced_through_message_sequence` 的原始消息。该投影由 `load_conversation_items` 统一实现；**Desktop 的普通聊天记录仍读取全部 `messages`**。

## 10. 开发库重建

```bash
docker compose down -v
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

删除 volume 会清除 Session、Trace、模型设置和加密后的 API Key，**必须由开发者显式执行**，应用启动不得自动删除未知数据。

## 11. 验收

- 每个 Turn / Message / Compaction 在 Session 内 sequence 唯一；
- Compaction 的 last-user、trigger Turn 和 parent checkpoint 均属同一 Session；
- **原始 `messages` 不因压缩或 rewind 删除**；
- Tool Message 必须有 Provider Call ID 和 Tool Name；
- 一个 Turn 下同一 Provider Tool Call 只有一个结果；
- **一个 Session 同时最多一个 `running` Turn**，且启动清扫能解除它；
- terminal Turn / Span 必须有 `ended_at` 且不早于 `started_at`；
- Model Span 不允许 `denied`；
- 每个 Span 都有非空 `trace_id`；同一次请求的全部 Span 共享它，手动压缩与 rewind 各自独立；
- `trace_spans` 无 `sequence` 列，并发写入不会因排序键冲突丢行；
- 只对 Tool Call 有意义的列不出现在其他 kind 的行上，`response_message_id` 只出现在 Model Call 上；
- **Trace Parent 缺失不阻止 Child Span 落库**，读取时计入采集缺口；
- **Trace 写入失败不回滚 Message/Turn 事务**；正文写入失败时 Span 本身仍落库；
- 相同内容在 `trace_payloads` 中只有一行；
- `truncated = TRUE` 的挂载行必须有 `original_byte_size`；
- 仍被引用的正文无法被删除（`RESTRICT` 生效）；
- **删除 Session 后孤儿清扫立即执行，库中不残留该 Session 的独有正文**；
- 同一标注目标只有一行，改评价是 upsert；带标注的 Trace 不被自动清理；
- 时间列存东八区，出库字符串带 `+08:00` 而非 `Z`；
- 没有任何表被 Runtime 当成未完成 Turn 的恢复游标。
