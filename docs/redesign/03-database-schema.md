# OpenWork V1 目标数据库 Schema

> 状态：SQLx 单一干净基线已实施。业务表使用 `provider_credentials`、`models`、`sessions`、`turns`、`messages`、`trace_spans`；当前没有生产数据，因此不再保留旧表回填和 `legacy_*` 兼容路径。
>
> 边界：保存模型配置、Session、Turn、完整 Message 和诊断 Trace；不保存可恢复的运行时 Checkpoint。
>
> 本文 SQL 使用的名称就是当前物理表名；结构由 `crates/openwork-core/migrations/202607180001_initial_schema.sql` 创建。

## 1. 结论

V1 收敛为 7 张表：

| 表 | 职责 | 是否业务真相 |
| --- | --- | --- |
| `_sqlx_migrations` | SQLx Migration 版本、校验和与执行状态 | 基础设施 |
| `provider_credentials` | Provider 元数据与加密凭证 | 是，敏感数据 |
| `models` | 可选择的模型端点与凭证引用 | 是 |
| `sessions` | Session 元数据 | 是 |
| `turns` | 一次用户运行的状态和汇总 | 是 |
| `messages` | 模型 Conversation 的完整消息 | 是 |
| `trace_spans` | Model Call/Tool Call 诊断 | 否，best effort |

关系：

```text
Provider Credential
  └── Model.credential_ref

Model
  ├── Session.default_model_id
  └── Turn.model_id

Session
└── Turn
    ├── Message
    └── Trace Span
        └── child Trace Span
```

V1 不建立：

```text
明文凭证
recorded_events
event_streams
steps
tool_runs
approvals
runtime_states
session_updates
projection_checkpoints
trace_span_events
```

“7 张表”是当前功能边界，不是永久架构不变量。Provider Credential 单独成表，是为了避免把加密密文复制到每个 Model 行，并保持凭证轮换的原子性。

## 2. 为什么不用 Event Journal

当前 `recorded_events` 为 `Turn → Step → ToolRun → Approval` 回放服务，但 V1 明确不恢复未完成 Turn，因此 Journal 带来的版本分配、投影、Replay 和 Upcaster 没有对应收益。

目标采用直接关系写入：

- `turns` 保存粗粒度生命周期；
- `messages` 保存已完成的 Conversation Item；
- `trace_spans` 保存旁路诊断；
- 活动 Phase 和 Pending Permission 只在 SessionActor 内存中。

关键业务写入仍使用事务，但不再通过 Event → Projection 两层表达同一个事实。

## 3. ID 与时间约定

- ID 使用应用生成的稳定 `TEXT`，允许保留当前带前缀的 ID；
- 所有 ID 都校验 `btrim(id) <> ''`；
- 业务时间统一使用 UTC 的 `TIMESTAMP WITHOUT TIME ZONE`，写入时显式转换到 UTC；
- API 返回时间时补回 `Z`，桌面端固定按 `Asia/Shanghai` 展示；
- SQLx 自管的 `_sqlx_migrations.installed_on` 保持其内置类型，不纳入业务时间约定；
- `created_at` 写入后不修改；
- Repository 更新业务字段时必须同时更新 `updated_at`；
- Turn/Message/Span 的顺序由所属 SessionActor 分配，并由唯一约束兜底。

## 4. _sqlx_migrations

```sql
CREATE TABLE _sqlx_migrations (
    version         BIGINT PRIMARY KEY,
    description     TEXT NOT NULL,
    installed_on    TIMESTAMPTZ NOT NULL DEFAULT now(),
    success         BOOLEAN NOT NULL,
    checksum        BYTEA NOT NULL,
    execution_time  BIGINT NOT NULL
);
```

规则：

- 该表由 SQLx 创建和维护，业务代码不得直接写入；
- 已执行 Migration 不修改内容，checksum 不一致时迁移失败；
- 数据回填与 Schema 变更使用不同版本，便于重试和审计。

## 5. provider_credentials

一行保存一个 Provider 的公开元数据和加密 API Key。密文使用 `provider_id` 作为 AAD；Repository 不返回密文字段，只能返回公开 Profile 或在模型调用边界解密后的零化凭证类型。模型由 Session 显式选择，不维护全局 Active Provider。

```sql
CREATE TABLE provider_credentials (
    provider_id        TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    provider_kind      TEXT NOT NULL,
    base_url           TEXT NOT NULL,
    api_key_encrypted  TEXT NOT NULL,
    enabled            BOOLEAN NOT NULL DEFAULT TRUE,
    config             JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at         TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    updated_at         TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),

    CONSTRAINT provider_credentials_id_not_blank
        CHECK (btrim(provider_id) <> ''),
    CONSTRAINT provider_credentials_name_not_blank
        CHECK (btrim(display_name) <> ''),
    CONSTRAINT provider_credentials_kind_valid
        CHECK (provider_kind IN ('openai', 'anthropic', 'deepseek', 'kimi', 'qwen', 'glm')),
    CONSTRAINT provider_credentials_base_url_not_blank
        CHECK (btrim(base_url) <> ''),
    CONSTRAINT provider_credentials_secret_not_blank
        CHECK (btrim(api_key_encrypted) <> ''),
    CONSTRAINT provider_credentials_config_is_object
        CHECK (jsonb_typeof(config) = 'object')
);

```

## 6. models

一行表示一个可直接选择的模型端点。V1 不把 Provider 与 Provider Model 拆成两个管理 Aggregate。

```sql
CREATE TABLE models (
    id                  TEXT PRIMARY KEY,
    display_name        TEXT NOT NULL,
    provider_kind       TEXT NOT NULL,
    model_name          TEXT NOT NULL,
    base_url            TEXT NOT NULL,
    credential_ref      TEXT,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    config              JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    updated_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),

    CONSTRAINT models_id_not_blank
        CHECK (btrim(id) <> ''),
    CONSTRAINT models_display_name_not_blank
        CHECK (btrim(display_name) <> ''),
    CONSTRAINT models_provider_kind_not_blank
        CHECK (btrim(provider_kind) <> ''),
    CONSTRAINT models_model_name_not_blank
        CHECK (btrim(model_name) <> ''),
    CONSTRAINT models_base_url_not_blank
        CHECK (btrim(base_url) <> ''),
    CONSTRAINT models_credential_ref_not_blank
        CHECK (credential_ref IS NULL OR btrim(credential_ref) <> ''),
    CONSTRAINT models_config_is_object
        CHECK (jsonb_typeof(config) = 'object')
);

CREATE UNIQUE INDEX uq_models_endpoint_credential
    ON models(
        provider_kind,
        base_url,
        model_name,
        COALESCE(credential_ref, '')
    );

CREATE INDEX idx_models_enabled_name
    ON models(enabled, display_name);
```

`credential_ref` 使用 `provider:<provider_id>` 指向 Core 的 `provider_credentials`。Model 表不保存明文或密文 Secret。

`config` 只保存低频 Provider 选项，例如额外 Header 名称或兼容方言。禁止保存：

- API Key 明文；
- 完整请求/响应；
- Session 或 Turn 状态；
- 可由固定代码默认值表达的杂项字段。

## 7. sessions

```sql
CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,
    title               TEXT,
    working_directory   TEXT NOT NULL,
    default_model_id    TEXT REFERENCES models(id) ON DELETE SET NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    created_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    updated_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    last_turn_at        TIMESTAMP WITHOUT TIME ZONE,

    CONSTRAINT sessions_id_not_blank
        CHECK (btrim(id) <> ''),
    CONSTRAINT sessions_title_not_blank
        CHECK (title IS NULL OR btrim(title) <> ''),
    CONSTRAINT sessions_working_directory_not_blank
        CHECK (btrim(working_directory) <> ''),
    CONSTRAINT sessions_status_valid
        CHECK (status IN ('active', 'archived'))
);

CREATE INDEX idx_sessions_status_updated
    ON sessions(status, updated_at DESC, id);

CREATE INDEX idx_sessions_default_model
    ON sessions(default_model_id)
    WHERE default_model_id IS NOT NULL;
```

`working_directory` 是文件/进程工具的执行根目录，可以不是 Git 仓库；V1 不为它建立独立 Workspace 记录、Trust 状态或 Git 元数据。

Session 不保存 `runtime_state`、Pending Permission 或当前 Tool Call。活动状态由 `SessionActor` 拥有。

## 8. turns

一个 Turn 是一次输入触发的完整 Agent Loop。它可以包含多次 Model Call 和 Tool Call。

```sql
CREATE TABLE turns (
    id                      TEXT PRIMARY KEY,
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence                BIGINT NOT NULL,
    model_id                TEXT REFERENCES models(id) ON DELETE SET NULL,
    resolved_model_name     TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_call_count        INTEGER NOT NULL DEFAULT 0,
    tool_call_count         INTEGER NOT NULL DEFAULT 0,
    input_tokens            BIGINT,
    output_tokens           BIGINT,
    cached_input_tokens     BIGINT,
    reasoning_tokens        BIGINT,
    total_tokens            BIGINT GENERATED ALWAYS AS (
        CASE
            WHEN input_tokens IS NULL OR output_tokens IS NULL THEN NULL
            ELSE input_tokens + output_tokens
        END
    ) STORED,
    error_code              TEXT,
    error_message           TEXT,
    started_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    ended_at                TIMESTAMP WITHOUT TIME ZONE,
    updated_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),

    CONSTRAINT turns_id_not_blank
        CHECK (btrim(id) <> ''),
    CONSTRAINT turns_sequence_positive
        CHECK (sequence > 0),
    CONSTRAINT turns_resolved_model_not_blank
        CHECK (btrim(resolved_model_name) <> ''),
    CONSTRAINT turns_status_valid
        CHECK (status IN (
            'running', 'completed', 'failed', 'cancelled', 'interrupted'
        )),
    CONSTRAINT turns_counts_non_negative
        CHECK (model_call_count >= 0 AND tool_call_count >= 0),
    CONSTRAINT turns_tokens_non_negative
        CHECK (
            (input_tokens IS NULL OR input_tokens >= 0) AND
            (output_tokens IS NULL OR output_tokens >= 0) AND
            (cached_input_tokens IS NULL OR cached_input_tokens >= 0) AND
            (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
        ),
    CONSTRAINT turns_terminal_time_valid
        CHECK (
            (status = 'running' AND ended_at IS NULL) OR
            (status <> 'running' AND ended_at IS NOT NULL)
        ),
    CONSTRAINT turns_end_after_start
        CHECK (ended_at IS NULL OR ended_at >= started_at),
    CONSTRAINT turns_error_pair_valid
        CHECK (
            (error_code IS NULL AND error_message IS NULL) OR
            (error_code IS NOT NULL AND btrim(error_code) <> '')
        ),
    CONSTRAINT uq_turns_id_session
        UNIQUE (id, session_id),
    CONSTRAINT uq_turns_session_sequence
        UNIQUE (session_id, sequence)
);

CREATE INDEX idx_turns_session_sequence
    ON turns(session_id, sequence DESC);

CREATE INDEX idx_turns_model_started
    ON turns(model_id, started_at DESC)
    WHERE model_id IS NOT NULL;

CREATE INDEX idx_turns_running
    ON turns(started_at, id)
    WHERE status = 'running';

CREATE UNIQUE INDEX uq_turns_one_running_per_session
    ON turns(session_id)
    WHERE status = 'running';
```

`model_id` 可因模型配置删除而变为 `NULL`，`resolved_model_name` 仍保留实际调用身份。一次 Turn 内若未来允许切换模型，应在对应 Model Call Span 上记录真实模型；Turn 字段表示开始时解析出的默认模型。

## 9. messages

`messages` 保存完整模型消息，不保存 Token Delta。

```sql
CREATE TABLE messages (
    id                  TEXT PRIMARY KEY,
    session_id          TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id             TEXT,
    sequence            BIGINT NOT NULL,
    role                TEXT NOT NULL,
    content             JSONB NOT NULL,
    provider_call_id    TEXT,
    tool_name           TEXT,
    created_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),

    CONSTRAINT messages_turn_session_fk
        FOREIGN KEY (turn_id, session_id)
        REFERENCES turns(id, session_id)
        ON DELETE CASCADE,
    CONSTRAINT messages_id_not_blank
        CHECK (btrim(id) <> ''),
    CONSTRAINT messages_sequence_positive
        CHECK (sequence > 0),
    CONSTRAINT messages_role_valid
        CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    CONSTRAINT messages_content_is_array
        CHECK (jsonb_typeof(content) = 'array'),
    CONSTRAINT messages_turn_required
        CHECK (turn_id IS NOT NULL OR role = 'system'),
    CONSTRAINT messages_tool_identity_valid
        CHECK (
            (role = 'tool' AND provider_call_id IS NOT NULL
                           AND btrim(provider_call_id) <> ''
                           AND tool_name IS NOT NULL
                           AND btrim(tool_name) <> '') OR
            (role <> 'tool' AND provider_call_id IS NULL AND tool_name IS NULL)
        ),
    CONSTRAINT uq_messages_session_sequence
        UNIQUE (session_id, sequence)
);

CREATE INDEX idx_messages_session_sequence
    ON messages(session_id, sequence);

CREATE INDEX idx_messages_turn_sequence
    ON messages(turn_id, sequence)
    WHERE turn_id IS NOT NULL;

CREATE UNIQUE INDEX uq_messages_tool_result
    ON messages(turn_id, provider_call_id)
    WHERE role = 'tool';
```

`content` 使用 `openwork-models::ContentBlock` 的版本化 JSON 表达：

```json
[
  {
    "type": "tool_result",
    "version": 1,
    "callId": "call_123",
    "output": [
      { "type": "text", "text": "..." }
    ],
    "isError": false
  }
]
```

规则：

- Assistant Tool Call 存在 Assistant Message 的 `content` 中；
- 每个 Tool Result 独立一条 Tool Message；
- System Message 可为 Session 级，因此 `turn_id` 可空；
- Message 一旦提交不原地修改；
- 流式草稿只在完整响应结束后生成 Message；
- Provider Opaque Block 可以保存，但必须由 `openwork-models` 版本化并限制大小。

## 10. trace_spans

Turn 行本身是 Trace Root，`trace_spans` 只保存 Model Call 与 Tool Call。

```sql
CREATE TABLE trace_spans (
    id                      TEXT PRIMARY KEY,
    turn_id                 TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    parent_span_id          TEXT,
    sequence                BIGINT NOT NULL,
    kind                    TEXT NOT NULL,
    name                    TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_id                TEXT REFERENCES models(id) ON DELETE SET NULL,
    resolved_model_name     TEXT,
    provider_request_id     TEXT,
    provider_call_id        TEXT,
    requested_tool_name     TEXT,
    resolved_tool_name      TEXT,
    attempt_count           INTEGER,
    input_tokens            BIGINT,
    output_tokens           BIGINT,
    cached_input_tokens     BIGINT,
    reasoning_tokens        BIGINT,
    total_tokens            BIGINT GENERATED ALWAYS AS (
        CASE
            WHEN input_tokens IS NULL OR output_tokens IS NULL THEN NULL
            ELSE input_tokens + output_tokens
        END
    ) STORED,
    permission_wait_ms      BIGINT,
    started_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    ended_at                TIMESTAMP WITHOUT TIME ZONE,
    error_code              TEXT,
    error_message           TEXT,
    attributes              JSONB NOT NULL DEFAULT '{}'::jsonb,

    CONSTRAINT uq_trace_spans_turn_sequence
        UNIQUE (turn_id, sequence),
    CONSTRAINT trace_spans_id_not_blank
        CHECK (btrim(id) <> ''),
    CONSTRAINT trace_spans_sequence_positive
        CHECK (sequence > 0),
    CONSTRAINT trace_spans_kind_valid
        CHECK (kind IN ('model_call', 'tool_call')),
    CONSTRAINT trace_spans_name_not_blank
        CHECK (btrim(name) <> ''),
    CONSTRAINT trace_spans_status_valid
        CHECK (status IN (
            'running', 'succeeded', 'failed', 'denied',
            'cancelled', 'outcome_unknown'
        )),
    CONSTRAINT trace_spans_kind_status_valid
        CHECK (
            (kind = 'model_call' AND status IN (
                'running', 'succeeded', 'failed', 'cancelled', 'outcome_unknown'
            )) OR
            (kind = 'tool_call' AND status IN (
                'running', 'succeeded', 'failed', 'denied',
                'cancelled', 'outcome_unknown'
            ))
        ),
    CONSTRAINT trace_spans_terminal_time_valid
        CHECK (
            (status = 'running' AND ended_at IS NULL) OR
            (status <> 'running' AND ended_at IS NOT NULL)
        ),
    CONSTRAINT trace_spans_end_after_start
        CHECK (ended_at IS NULL OR ended_at >= started_at),
    CONSTRAINT trace_spans_attempt_count_valid
        CHECK (attempt_count IS NULL OR attempt_count > 0),
    CONSTRAINT trace_spans_metrics_non_negative
        CHECK (
            (input_tokens IS NULL OR input_tokens >= 0) AND
            (output_tokens IS NULL OR output_tokens >= 0) AND
            (cached_input_tokens IS NULL OR cached_input_tokens >= 0) AND
            (reasoning_tokens IS NULL OR reasoning_tokens >= 0) AND
            (permission_wait_ms IS NULL OR permission_wait_ms >= 0)
        ),
    CONSTRAINT trace_spans_attributes_is_object
        CHECK (jsonb_typeof(attributes) = 'object'),
    CONSTRAINT trace_spans_model_fields_valid
        CHECK (
            kind <> 'model_call' OR
            (resolved_model_name IS NOT NULL AND btrim(resolved_model_name) <> '')
        ),
    CONSTRAINT trace_spans_tool_fields_valid
        CHECK (
            kind <> 'tool_call' OR
            (provider_call_id IS NOT NULL AND btrim(provider_call_id) <> '' AND
             requested_tool_name IS NOT NULL AND btrim(requested_tool_name) <> '')
        )
);

CREATE INDEX idx_trace_spans_turn_sequence
    ON trace_spans(turn_id, sequence);

CREATE INDEX idx_trace_spans_turn_parent
    ON trace_spans(turn_id, parent_span_id)
    WHERE parent_span_id IS NOT NULL;

CREATE INDEX idx_trace_spans_kind_started
    ON trace_spans(kind, started_at DESC, turn_id);

CREATE INDEX idx_trace_spans_failed_started
    ON trace_spans(started_at DESC, turn_id)
    WHERE status IN ('failed', 'outcome_unknown');

CREATE UNIQUE INDEX uq_trace_spans_tool_provider_call
    ON trace_spans(turn_id, provider_call_id)
    WHERE kind = 'tool_call';
```

Repository 还必须校验：

- Model Call 的 `parent_span_id` 为 `NULL`；
- Tool Call 的 Parent 正常情况下是同一 Turn 的 Model Call；
- Parent Span 因采集丢失而不存在时，仍保存 Tool Span 并在查询结果中标记为 Orphan；
- `resolved_tool_name` 记录 Alias/路由后的真实工具名；
- `attempt_count` 表示一次 Model Call 内的总 Transport 尝试数；
- `provider_request_id` 表示最终一次或 Provider 公开的请求 ID，而不是尝试明细；
- `attributes` 不保存原始 Prompt 内容、完整 Tool Input/Output 或凭证。

## 11. 写入顺序和事务

### 10.1 开始 Turn

一个事务：

1. 锁定 Session 或依赖 `(session_id, sequence)` 唯一约束分配下一序号；
2. 插入 `turns(status='running')`；
3. 插入 User Message；
4. 更新 `sessions.last_turn_at/updated_at`；
5. 提交后才调用模型。

### 10.2 完成一次 Model Call

1. 调用 Provider 前先把 `model_call_count + 1` 提交；
2. 流式草稿只在内存中；
3. 响应完整后插入 Assistant Message；
4. 同一事务累加 Token，并按响应中的完整调用数累加 `tool_call_count`；
5. 事务提交后才执行响应中的 Tool Call；
6. Model Call Span 通过独立 best-effort 写入结束。

这保证“数据库尚未保存模型要求执行什么”时不会先产生工具副作用。

### 10.3 完成一次 Tool Call

1. 工具执行；
2. 形成成功或错误 Tool Message；
3. 插入 Tool Message；`tool_call_count` 已在 Assistant Message 事务中登记；
4. 提交成功后才进入下一次 Model Call；
5. Tool Call Span 独立结束。

若副作用已发生但 Tool Message 写入失败，Turn 失败。重启后保持 `interrupted/outcome_unknown`，不自动执行同一工具。

### 10.4 结束 Turn

更新 `turns`：

- `status`；
- `ended_at`；
- 最终 Token/Call 汇总；
- 可选 `error_code/error_message`；
- `updated_at`。

最终回答本身读取最后一条 Assistant Message，不在 `turns` 重复存一份。

## 12. 启动修正

Core 完成 Migration 后、接受新 Turn 前执行：

```sql
UPDATE turns
SET status = 'interrupted',
    ended_at = COALESCE(ended_at, CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC',
    error_code = COALESCE(error_code, 'process_interrupted'),
    error_message = COALESCE(error_message, 'process exited before turn completion')
WHERE status = 'running';

UPDATE trace_spans
SET status = 'outcome_unknown',
    ended_at = COALESCE(ended_at, CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    error_code = COALESCE(error_code, 'process_interrupted')
WHERE status = 'running';
```

随后 Core 读取刚被中断的 Turn Message：

1. 从 Assistant Message 提取完整 Tool Call；
2. 用 `(turn_id, provider_call_id)` 查找已有 Tool Message；
3. 对没有结果的调用，按原调用顺序追加合成 Tool Message；
4. 结果状态写为 `outcome_unknown`，文本明确说明副作用可能已经发生且不得自动重试；
5. 提交后才允许该 Session 接受新的 Turn。

这只是状态与 Conversation 完整性收口，不调度恢复任务，也不读取 Trace 判断工具是否执行过。

## 13. 常用读取

Session 列表：

```sql
SELECT id, title, working_directory, default_model_id,
       status, created_at, updated_at, last_turn_at
FROM sessions
WHERE status = 'active'
ORDER BY updated_at DESC, id
LIMIT $1;
```

模型 Conversation：

```sql
SELECT id, turn_id, sequence, role, content,
       provider_call_id, tool_name, created_at
FROM messages
WHERE session_id = $1
ORDER BY sequence;
```

Turn Trace：

```sql
SELECT *
FROM trace_spans
WHERE turn_id = $1
ORDER BY sequence;
```

## 14. 开发数据库基线切换

当前没有生产数据，因此不再把旧 Journal、Provider Registry 或 `schema_migrations` 接入新历史。开发数据库显式删除 volume 后，从 SQLx 基线重新创建：

```bash
docker compose down -v
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

基线版本为 `202607180001_initial_schema.sql`。它只创建当前六张业务表，不创建、回填或归档任何旧表。未来每次结构或数据变化都追加新的 SQLx migration；已经执行的文件保持不可变。

删除 volume 会清除 Session、Trace、模型设置和加密后的 Provider API Key，必须由开发者显式执行，应用启动不得自动删除未知数据。

## 15. Schema 验收清单

- 每个 Turn 在 Session 内 sequence 唯一；
- 每个 Message 在 Session 内 sequence 唯一；
- Tool Message 必须有 Provider Call ID 和 Tool Name；
- 一个 Turn 下同一 Provider Tool Call 只有一个 Tool Result；
- 一个 Session 同时最多一个 `running` Turn；
- terminal Turn/Span 必须有 `ended_at`；
- Model Span 不允许 `denied`；
- Tool Span 必须能关联 Provider Call ID；
- Trace Parent 缺失不会阻止 Child Span 落库，详情页会把它计入采集缺口；
- Trace 写入失败不回滚 Message/Turn 事务；
- 没有任何表被 Runtime 当成未完成 Turn 的恢复游标；
- 启动修正不会重新执行 Provider 或 Tool 请求；
- 启动修正会为悬空 Tool Call 写入 `outcome_unknown` Tool Result，使后续模型历史保持合法；
- Secret 不以明文或普通 JSON 配置落库。
