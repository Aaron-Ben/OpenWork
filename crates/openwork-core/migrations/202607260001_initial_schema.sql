-- OpenWork 基线 schema。
--
-- 本文件取代 202607180001..202607250003 共 7 个迁移。合并的原因是那批迁移里有相当一部分
-- 复杂度是增量演进的产物而非设计意图：`trace_spans_scope_valid` 是 `turn_id` 兼任 Trace 根
-- 之后打的补丁，`messages_id_session_sequence_unique` 是为了满足一个三列外键而生的冗余索引，
-- naive UTC 时间列是为了迁就一句硬编码 `Z` 的 `to_char`。合并后这些都不再需要。
--
-- 时间口径：所有时间列为 TIMESTAMP WITHOUT TIME ZONE，存东八区（Asia/Shanghai）墙上时间。
-- 见 .claude/rules/database.md。

CREATE TABLE provider_credentials (
    provider_id        TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    provider_kind      TEXT NOT NULL,
    base_url           TEXT NOT NULL,
    api_key_encrypted  TEXT NOT NULL,
    enabled            BOOLEAN NOT NULL DEFAULT TRUE,
    config             JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at         TIMESTAMP WITHOUT TIME ZONE NOT NULL
                       DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at         TIMESTAMP WITHOUT TIME ZONE NOT NULL
                       DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
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
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT models_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT models_display_name_not_blank CHECK (btrim(display_name) <> ''),
    -- 与 provider_credentials 同一套取值，避免同一概念在两张表约束强度不一致。
    CONSTRAINT models_provider_kind_valid
        CHECK (provider_kind IN ('openai', 'anthropic', 'deepseek', 'kimi', 'qwen', 'glm')),
    CONSTRAINT models_model_name_not_blank CHECK (btrim(model_name) <> ''),
    CONSTRAINT models_base_url_not_blank CHECK (btrim(base_url) <> ''),
    -- 多态引用：'provider:<provider_id>' 指向 provider_credentials，其余值是环境变量名。
    -- 因此不能建外键。解析方是 ProviderCredentialResolver。
    CONSTRAINT models_credential_ref_not_blank
        CHECK (credential_ref IS NULL OR btrim(credential_ref) <> ''),
    CONSTRAINT models_config_is_object CHECK (jsonb_typeof(config) = 'object')
);

CREATE UNIQUE INDEX uq_models_endpoint_credential
    ON models(provider_kind, base_url, model_name, COALESCE(credential_ref, ''));

CREATE INDEX idx_models_enabled_name
    ON models(enabled, display_name);

CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,
    title               TEXT,
    working_directory   TEXT NOT NULL,
    default_model_id    TEXT REFERENCES models(id) ON DELETE SET NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    created_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    last_turn_at        TIMESTAMP WITHOUT TIME ZONE,
    CONSTRAINT sessions_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT sessions_title_not_blank CHECK (title IS NULL OR btrim(title) <> ''),
    CONSTRAINT sessions_working_directory_not_blank
        CHECK (btrim(working_directory) <> ''),
    CONSTRAINT sessions_status_valid CHECK (status IN ('active', 'archived'))
);

CREATE INDEX idx_sessions_status_updated
    ON sessions(status, updated_at DESC, id);

CREATE INDEX idx_sessions_default_model
    ON sessions(default_model_id) WHERE default_model_id IS NOT NULL;

-- 一个 Turn 是一次用户输入触发的完整 Agent Loop，可以包含多次 Model Call 和 Tool Call。
-- 它是业务实体：取消、恢复和用量归集都以它为单位。它不是 Trace 的关联根，
-- 那是 trace_spans.trace_id 的职责。
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
    -- 与 trace_spans 的计数互为独立参照物：两者一致才判定 Trace 完整。
    -- 这是有意的冗余，不要用 span 计数替代。
    model_call_count        INTEGER NOT NULL DEFAULT 0,
    model_submission_count  INTEGER NOT NULL DEFAULT 0,
    tool_call_count         INTEGER NOT NULL DEFAULT 0,
    input_tokens            BIGINT,
    output_tokens           BIGINT,
    cached_input_tokens     BIGINT,
    reasoning_tokens        BIGINT,
    error_code              TEXT,
    error_message           TEXT,
    started_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL
                            DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    ended_at                TIMESTAMP WITHOUT TIME ZONE,
    updated_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL
                            DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT turns_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT turns_client_request_not_blank CHECK (btrim(client_request_id) <> ''),
    CONSTRAINT turns_sequence_positive CHECK (sequence > 0),
    CONSTRAINT turns_provider_not_blank CHECK (btrim(resolved_provider_kind) <> ''),
    CONSTRAINT turns_model_not_blank CHECK (btrim(resolved_model_name) <> ''),
    CONSTRAINT turns_app_version_not_blank CHECK (btrim(app_version) <> ''),
    CONSTRAINT turns_status_valid CHECK (
        status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    CONSTRAINT turns_counts_non_negative CHECK (
        model_call_count >= 0 AND model_submission_count >= 0 AND tool_call_count >= 0
    ),
    CONSTRAINT turns_model_submission_covers_logical_calls
        CHECK (model_submission_count >= model_call_count),
    CONSTRAINT turns_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (cached_input_tokens IS NULL OR cached_input_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
    ),
    CONSTRAINT turns_terminal_time_valid CHECK (
        (status = 'running' AND ended_at IS NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT turns_end_after_start CHECK (ended_at IS NULL OR ended_at >= started_at),
    CONSTRAINT turns_id_session UNIQUE (id, session_id),
    CONSTRAINT turns_session_sequence UNIQUE (session_id, sequence),
    CONSTRAINT turns_client_request UNIQUE (session_id, client_request_id)
);

CREATE UNIQUE INDEX uq_turns_one_running_per_session
    ON turns(session_id) WHERE status = 'running';

CREATE INDEX idx_turns_session_sequence
    ON turns(session_id, sequence DESC);

CREATE TABLE messages (
    id                      TEXT PRIMARY KEY,
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id                 TEXT,
    sequence                BIGINT NOT NULL,
    role                    TEXT NOT NULL,
    content                 JSONB NOT NULL,
    -- content 的块结构版本。这是产品最核心的持久化事实，形状演进时必须能区分新旧行。
    content_format_version  SMALLINT NOT NULL DEFAULT 1,
    provider_call_id        TEXT,
    tool_name               TEXT,
    created_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL
                            DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    -- 复合外键防止消息挂到别的 Session 的 Turn 上。
    CONSTRAINT messages_turn_session_fk
        FOREIGN KEY (turn_id, session_id)
        REFERENCES turns(id, session_id) ON DELETE CASCADE,
    CONSTRAINT messages_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT messages_sequence_positive CHECK (sequence > 0),
    CONSTRAINT messages_role_valid CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    CONSTRAINT messages_content_is_array CHECK (jsonb_typeof(content) = 'array'),
    CONSTRAINT messages_content_format_positive CHECK (content_format_version > 0),
    CONSTRAINT messages_turn_required CHECK (turn_id IS NOT NULL OR role = 'system'),
    CONSTRAINT messages_tool_identity_valid CHECK (
        (role = 'tool' AND provider_call_id IS NOT NULL
                       AND btrim(provider_call_id) <> ''
                       AND tool_name IS NOT NULL
                       AND btrim(tool_name) <> '') OR
        (role <> 'tool' AND provider_call_id IS NULL AND tool_name IS NULL)
    ),
    CONSTRAINT messages_session_sequence UNIQUE (session_id, sequence)
);

CREATE INDEX idx_messages_session_sequence
    ON messages(session_id, sequence);

CREATE UNIQUE INDEX uq_messages_tool_result
    ON messages(turn_id, provider_call_id) WHERE role = 'tool';

CREATE TABLE conversation_compactions (
    id                                  TEXT PRIMARY KEY,
    session_id                          TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence                            BIGINT NOT NULL,
    through_message_sequence            BIGINT NOT NULL,
    replaced_through_message_sequence   BIGINT NOT NULL,
    source_message_count                INTEGER NOT NULL,
    checkpoint_format_version           SMALLINT NOT NULL DEFAULT 0,
    kind                                TEXT NOT NULL DEFAULT 'manual',
    summary_format_version              SMALLINT NOT NULL DEFAULT 0,
    last_user_message_id                TEXT REFERENCES messages(id),
    last_user_message_sequence          BIGINT,
    resolved_model_name                 TEXT NOT NULL,
    summary                             TEXT NOT NULL,
    runtime_state                       JSONB NOT NULL
        DEFAULT '{"schemaVersion":1,"editedPaths":[],"extensions":{},"warnings":[]}'::jsonb,
    runtime_reminder_format_version     SMALLINT NOT NULL DEFAULT 1,
    runtime_reminder                    TEXT NOT NULL
        DEFAULT E'<system_reminder format_version="1">\nNo additional durable runtime state was recorded at compaction time.\n</system_reminder>',
    trigger_turn_id                     TEXT,
    parent_compaction_id                TEXT,
    input_tokens                        BIGINT,
    output_tokens                       BIGINT,
    created_at                          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT conversation_compactions_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT conversation_compactions_sequence_positive CHECK (sequence > 0),
    CONSTRAINT conversation_compactions_message_sequence_non_negative
        CHECK (through_message_sequence >= 0),
    CONSTRAINT conversation_compactions_replacement_covers_source
        CHECK (replaced_through_message_sequence >= through_message_sequence),
    CONSTRAINT conversation_compactions_source_count_positive
        CHECK (source_message_count > 0),
    CONSTRAINT conversation_compactions_checkpoint_format_non_negative
        CHECK (checkpoint_format_version >= 0),
    CONSTRAINT conversation_compactions_kind_valid
        CHECK (kind IN ('manual', 'threshold', 'overflow', 'rewind')),
    CONSTRAINT conversation_compactions_summary_format_non_negative
        CHECK (summary_format_version >= 0),
    CONSTRAINT conversation_compactions_last_user_pair CHECK (
        (last_user_message_id IS NULL AND last_user_message_sequence IS NULL) OR
        (last_user_message_id IS NOT NULL AND last_user_message_sequence IS NOT NULL)
    ),
    CONSTRAINT conversation_compactions_last_user_within_source CHECK (
        last_user_message_sequence IS NULL OR
        last_user_message_sequence <= through_message_sequence
    ),
    CONSTRAINT conversation_compactions_model_not_blank
        CHECK (btrim(resolved_model_name) <> ''),
    CONSTRAINT conversation_compactions_summary_not_blank CHECK (btrim(summary) <> ''),
    CONSTRAINT conversation_compactions_runtime_state_is_object
        CHECK (jsonb_typeof(runtime_state) = 'object'),
    CONSTRAINT conversation_compactions_runtime_reminder_format_positive
        CHECK (runtime_reminder_format_version > 0),
    CONSTRAINT conversation_compactions_runtime_reminder_not_blank
        CHECK (btrim(runtime_reminder) <> ''),
    CONSTRAINT conversation_compactions_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0)
    ),
    CONSTRAINT conversation_compactions_trigger_shape CHECK (
        (kind IN ('threshold', 'overflow') AND trigger_turn_id IS NOT NULL) OR
        (kind NOT IN ('threshold', 'overflow') AND trigger_turn_id IS NULL)
    ),
    CONSTRAINT conversation_compactions_parent_shape CHECK (
        (kind = 'rewind' AND parent_compaction_id IS NOT NULL) OR
        (kind <> 'rewind' AND parent_compaction_id IS NULL)
    ),
    CONSTRAINT conversation_compactions_session_sequence UNIQUE (session_id, sequence),
    CONSTRAINT conversation_compactions_id_session_unique UNIQUE (id, session_id),
    CONSTRAINT conversation_compactions_trigger_turn_fk
        FOREIGN KEY (trigger_turn_id, session_id) REFERENCES turns(id, session_id),
    CONSTRAINT conversation_compactions_parent_fk
        FOREIGN KEY (parent_compaction_id, session_id)
        REFERENCES conversation_compactions(id, session_id)
);

CREATE INDEX idx_conversation_compactions_session_sequence
    ON conversation_compactions(session_id, sequence DESC);

-- Trace 回答「模型看到了什么、说了什么、花了多少钱、人怎么评价」。
-- 它仍是 best-effort：写入有损（队列满丢弃、批量失败丢整批），因此刻意不给
-- parent_span_id 建外键 —— 父 Span 丢失时必须只丢它自己，不能把子 Span 一起拖失败。
-- 孤儿在读取时统计成完整性信号。
--
-- 内容不存在这张表里，见 trace_payloads / trace_span_payloads。
CREATE TABLE trace_spans (
    id                      TEXT PRIMARY KEY,
    -- 结构根：一次用户请求（或一次无 Turn 的独立操作）的全部 Span 共享它。
    -- 无外键 —— 它是关联标签，不是业务实体引用。
    trace_id                TEXT NOT NULL,
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    -- 业务标签：这个操作是否发生在某个 Agent Loop 内部。
    -- 手动压缩与 rewind 不在，因此为空。排序一律用 started_at。
    turn_id                 TEXT,
    parent_span_id          TEXT,
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
    -- 成功的 Model Call 把响应写进了 messages。messages 是业务真相且永不删除，
    -- 所以 Trace 不复制一份内容，只留指针。失败调用没有 Message，
    -- 它的响应才落到 trace_span_payloads 的 'response' 槽位。
    response_message_id     TEXT REFERENCES messages(id) ON DELETE SET NULL,
    permission_wait_ms      BIGINT,
    started_at              TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    ended_at                TIMESTAMP WITHOUT TIME ZONE,
    error_code              TEXT,
    error_message           TEXT,
    attributes              JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT trace_spans_trace_not_blank CHECK (btrim(trace_id) <> ''),
    CONSTRAINT trace_spans_turn_session_fk
        FOREIGN KEY (turn_id, session_id)
        REFERENCES turns(id, session_id) ON DELETE CASCADE,
    CONSTRAINT trace_spans_kind_valid
        CHECK (kind IN ('model_call', 'tool_call', 'compaction')),
    CONSTRAINT trace_spans_status_valid CHECK (
        status IN ('running', 'succeeded', 'failed', 'denied', 'cancelled', 'outcome_unknown')
        OR (
            kind = 'model_call' AND parent_span_id IS NOT NULL AND
            status IN ('degenerate', 'deterministic', 'input_overflow', 'transient', 'timeout')
        )
    ),
    -- 只对 Tool Call 有意义的列，不允许出现在别的 kind 上。
    CONSTRAINT trace_spans_tool_columns_scoped CHECK (
        kind = 'tool_call' OR (
            provider_call_id IS NULL AND requested_tool_name IS NULL AND
            resolved_tool_name IS NULL AND permission_wait_ms IS NULL
        )
    ),
    CONSTRAINT trace_spans_terminal_time_valid CHECK (
        (status = 'running' AND ended_at IS NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT trace_spans_end_after_start CHECK (ended_at IS NULL OR ended_at >= started_at),
    CONSTRAINT trace_spans_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (cached_input_tokens IS NULL OR cached_input_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
    ),
    -- 只有 Model Call 会产生 Assistant Message。
    CONSTRAINT trace_spans_response_message_scoped CHECK (
        response_message_id IS NULL OR kind = 'model_call'
    ),
    CONSTRAINT trace_spans_attributes_is_object CHECK (jsonb_typeof(attributes) = 'object')
);

CREATE INDEX idx_trace_spans_trace_started
    ON trace_spans(trace_id, started_at);

CREATE INDEX idx_trace_spans_session_started
    ON trace_spans(session_id, started_at DESC);

CREATE INDEX idx_trace_spans_turn_started
    ON trace_spans(turn_id, started_at) WHERE turn_id IS NOT NULL;

-- 按内容哈希去重的正文存储。
--
-- 为什么不把正文直接放进 trace_spans 或 trace_span_payloads：System Context 和工具定义
-- 在一个 Session 内几乎不变，却随每次 Model Call 重复发送。一个 20 KB 的工具定义
-- 在 50 个 Turn、400 次调用后按行存是 8 MB，去重后是 20 KB。
--
-- 这张表**没有 session_id**：同样的工具定义本就跨 Session 相同，加上它就等于放弃去重。
-- 代价是删除 Session 不会级联删除正文，必须由孤儿清扫收尾 —— 见 docs/trace.md。
CREATE TABLE trace_payloads (
    hash        TEXT PRIMARY KEY,
    body        JSONB NOT NULL,
    byte_size   BIGINT NOT NULL,
    created_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT trace_payloads_hash_not_blank CHECK (btrim(hash) <> ''),
    CONSTRAINT trace_payloads_size_non_negative CHECK (byte_size >= 0)
);

-- Span 与正文的挂载关系。
--
-- span_id 建外键，和 parent_span_id 的规则并不矛盾：parent_span_id 指向**另一个**
-- 可能被独立丢弃的 Span，这一行指向的是**同批写入的自己**，不存在指空的可能。
CREATE TABLE trace_span_payloads (
    span_id             TEXT NOT NULL REFERENCES trace_spans(id) ON DELETE CASCADE,
    slot                TEXT NOT NULL,
    -- RESTRICT 让孤儿清扫无法误删仍被引用的正文。
    payload_hash        TEXT NOT NULL REFERENCES trace_payloads(hash) ON DELETE RESTRICT,
    truncated           BOOLEAN NOT NULL DEFAULT FALSE,
    original_byte_size  BIGINT,
    redacted_count      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (span_id, slot),
    CONSTRAINT trace_span_payloads_slot_valid CHECK (
        slot IN ('request', 'system_context', 'tool_definitions', 'response')
    ),
    -- 截断了就必须说明原始多大，否则界面上的「已截断」是个无法量化的警告。
    CONSTRAINT trace_span_payloads_truncation_shape CHECK (
        (truncated = FALSE AND original_byte_size IS NULL) OR
        (truncated = TRUE AND original_byte_size IS NOT NULL AND original_byte_size >= 0)
    ),
    CONSTRAINT trace_span_payloads_redacted_non_negative CHECK (redacted_count >= 0)
);

-- 孤儿清扫走这条索引：DELETE FROM trace_payloads p
--   WHERE NOT EXISTS (SELECT 1 FROM trace_span_payloads m WHERE m.payload_hash = p.hash)
CREATE INDEX idx_trace_span_payloads_hash ON trace_span_payloads(payload_hash);

-- 人对一次运行的判断。这是**业务真相** —— 丢了就是用户的输入丢了 —— 尽管它指向
-- best-effort 的 Span。由此推出一条保留策略约束：带标注的 Trace 不得被自动清理。
CREATE TABLE trace_annotations (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    trace_id    TEXT NOT NULL,
    -- 为空表示评价整条 Trace，非空表示评价其中某一次调用。
    span_id     TEXT REFERENCES trace_spans(id) ON DELETE CASCADE,
    rating      TEXT NOT NULL,
    note        TEXT,
    created_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT trace_annotations_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT trace_annotations_trace_not_blank CHECK (btrim(trace_id) <> ''),
    CONSTRAINT trace_annotations_rating_valid CHECK (rating IN ('good', 'bad', 'unsure')),
    CONSTRAINT trace_annotations_note_not_blank CHECK (note IS NULL OR btrim(note) <> '')
);

-- 一个目标只有一条标注，改评价是 upsert 而不是追加一条相反的。
CREATE UNIQUE INDEX uq_trace_annotations_target
    ON trace_annotations(trace_id, COALESCE(span_id, ''));

CREATE INDEX idx_trace_annotations_session_created
    ON trace_annotations(session_id, created_at DESC);
