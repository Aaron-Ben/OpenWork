CREATE TABLE provider_credentials_v2 (
    provider_id        TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    provider_kind      TEXT NOT NULL,
    base_url           TEXT NOT NULL,
    api_key_encrypted  TEXT NOT NULL,
    enabled            BOOLEAN NOT NULL DEFAULT TRUE,
    config             JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT provider_credentials_v2_id_not_blank
        CHECK (btrim(provider_id) <> ''),
    CONSTRAINT provider_credentials_v2_name_not_blank
        CHECK (btrim(display_name) <> ''),
    CONSTRAINT provider_credentials_v2_kind_valid
        CHECK (provider_kind IN ('openai', 'anthropic', 'deepseek', 'kimi', 'qwen', 'glm')),
    CONSTRAINT provider_credentials_v2_base_url_not_blank
        CHECK (btrim(base_url) <> ''),
    CONSTRAINT provider_credentials_v2_secret_not_blank
        CHECK (btrim(api_key_encrypted) <> ''),
    CONSTRAINT provider_credentials_v2_config_is_object
        CHECK (jsonb_typeof(config) = 'object')
);

CREATE TABLE models_v2 (
    id                  TEXT PRIMARY KEY,
    display_name        TEXT NOT NULL,
    provider_kind       TEXT NOT NULL,
    model_name          TEXT NOT NULL,
    base_url            TEXT NOT NULL,
    credential_ref      TEXT,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    config              JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT models_v2_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT models_v2_display_name_not_blank CHECK (btrim(display_name) <> ''),
    CONSTRAINT models_v2_provider_kind_not_blank CHECK (btrim(provider_kind) <> ''),
    CONSTRAINT models_v2_model_name_not_blank CHECK (btrim(model_name) <> ''),
    CONSTRAINT models_v2_base_url_not_blank CHECK (btrim(base_url) <> ''),
    CONSTRAINT models_v2_credential_ref_not_blank
        CHECK (credential_ref IS NULL OR btrim(credential_ref) <> ''),
    CONSTRAINT models_v2_config_is_object CHECK (jsonb_typeof(config) = 'object')
);

CREATE UNIQUE INDEX uq_models_v2_endpoint_credential
    ON models_v2(provider_kind, base_url, model_name, COALESCE(credential_ref, ''));

CREATE INDEX idx_models_v2_enabled_name
    ON models_v2(enabled, display_name);

CREATE TABLE sessions_v2 (
    id                  TEXT PRIMARY KEY,
    title               TEXT,
    working_directory   TEXT NOT NULL,
    default_model_id    TEXT REFERENCES models_v2(id) ON DELETE SET NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_turn_at        TIMESTAMPTZ,
    CONSTRAINT sessions_v2_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT sessions_v2_title_not_blank CHECK (title IS NULL OR btrim(title) <> ''),
    CONSTRAINT sessions_v2_working_directory_not_blank
        CHECK (btrim(working_directory) <> ''),
    CONSTRAINT sessions_v2_status_valid CHECK (status IN ('active', 'archived'))
);

CREATE INDEX idx_sessions_v2_status_updated
    ON sessions_v2(status, updated_at DESC, id);

CREATE INDEX idx_sessions_v2_default_model
    ON sessions_v2(default_model_id) WHERE default_model_id IS NOT NULL;

CREATE TABLE turns_v2 (
    id                      TEXT PRIMARY KEY,
    session_id              TEXT NOT NULL REFERENCES sessions_v2(id) ON DELETE CASCADE,
    client_request_id       TEXT NOT NULL,
    sequence                BIGINT NOT NULL,
    model_id                TEXT REFERENCES models_v2(id) ON DELETE SET NULL,
    resolved_provider_kind  TEXT NOT NULL,
    resolved_model_name     TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_call_count        INTEGER NOT NULL DEFAULT 0,
    tool_call_count         INTEGER NOT NULL DEFAULT 0,
    input_tokens            BIGINT,
    output_tokens           BIGINT,
    cached_input_tokens     BIGINT,
    error_code              TEXT,
    error_message           TEXT,
    started_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at                TIMESTAMPTZ,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT turns_v2_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT turns_v2_client_request_not_blank CHECK (btrim(client_request_id) <> ''),
    CONSTRAINT turns_v2_sequence_positive CHECK (sequence > 0),
    CONSTRAINT turns_v2_provider_not_blank CHECK (btrim(resolved_provider_kind) <> ''),
    CONSTRAINT turns_v2_model_not_blank CHECK (btrim(resolved_model_name) <> ''),
    CONSTRAINT turns_v2_status_valid CHECK (
        status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    CONSTRAINT turns_v2_counts_non_negative
        CHECK (model_call_count >= 0 AND tool_call_count >= 0),
    CONSTRAINT turns_v2_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (cached_input_tokens IS NULL OR cached_input_tokens >= 0)
    ),
    CONSTRAINT turns_v2_terminal_time_valid CHECK (
        (status = 'running' AND ended_at IS NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT turns_v2_end_after_start CHECK (ended_at IS NULL OR ended_at >= started_at),
    CONSTRAINT turns_v2_id_session UNIQUE (id, session_id),
    CONSTRAINT turns_v2_session_sequence UNIQUE (session_id, sequence),
    CONSTRAINT turns_v2_client_request UNIQUE (session_id, client_request_id)
);

CREATE UNIQUE INDEX uq_turns_v2_one_running_per_session
    ON turns_v2(session_id) WHERE status = 'running';

CREATE INDEX idx_turns_v2_session_sequence
    ON turns_v2(session_id, sequence DESC);

CREATE TABLE messages_v2 (
    id                  TEXT PRIMARY KEY,
    session_id          TEXT NOT NULL REFERENCES sessions_v2(id) ON DELETE CASCADE,
    turn_id             TEXT,
    sequence            BIGINT NOT NULL,
    role                TEXT NOT NULL,
    content             JSONB NOT NULL,
    provider_call_id    TEXT,
    tool_name           TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT messages_v2_turn_session_fk
        FOREIGN KEY (turn_id, session_id)
        REFERENCES turns_v2(id, session_id) ON DELETE CASCADE,
    CONSTRAINT messages_v2_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT messages_v2_sequence_positive CHECK (sequence > 0),
    CONSTRAINT messages_v2_role_valid CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    CONSTRAINT messages_v2_content_is_array CHECK (jsonb_typeof(content) = 'array'),
    CONSTRAINT messages_v2_turn_required CHECK (turn_id IS NOT NULL OR role = 'system'),
    CONSTRAINT messages_v2_tool_identity_valid CHECK (
        (role = 'tool' AND provider_call_id IS NOT NULL
                       AND btrim(provider_call_id) <> ''
                       AND tool_name IS NOT NULL
                       AND btrim(tool_name) <> '') OR
        (role <> 'tool' AND provider_call_id IS NULL AND tool_name IS NULL)
    ),
    CONSTRAINT messages_v2_session_sequence UNIQUE (session_id, sequence)
);

CREATE INDEX idx_messages_v2_session_sequence
    ON messages_v2(session_id, sequence);

CREATE UNIQUE INDEX uq_messages_v2_tool_result
    ON messages_v2(turn_id, provider_call_id) WHERE role = 'tool';

CREATE TABLE trace_spans_v2 (
    id                      TEXT PRIMARY KEY,
    turn_id                 TEXT NOT NULL REFERENCES turns_v2(id) ON DELETE CASCADE,
    parent_span_id          TEXT,
    sequence                BIGINT NOT NULL,
    kind                    TEXT NOT NULL,
    name                    TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_id                TEXT REFERENCES models_v2(id) ON DELETE SET NULL,
    resolved_model_name     TEXT,
    provider_request_id     TEXT,
    provider_call_id        TEXT,
    requested_tool_name     TEXT,
    resolved_tool_name      TEXT,
    attempt_count           INTEGER,
    input_tokens            BIGINT,
    output_tokens           BIGINT,
    cached_input_tokens     BIGINT,
    permission_wait_ms      BIGINT,
    started_at              TIMESTAMPTZ NOT NULL,
    ended_at                TIMESTAMPTZ,
    error_code              TEXT,
    error_message           TEXT,
    attributes              JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT trace_spans_v2_turn_sequence UNIQUE (turn_id, sequence),
    CONSTRAINT trace_spans_v2_kind_valid CHECK (kind IN ('model_call', 'tool_call')),
    CONSTRAINT trace_spans_v2_status_valid CHECK (
        status IN ('running', 'succeeded', 'failed', 'denied', 'cancelled', 'outcome_unknown')
    ),
    CONSTRAINT trace_spans_v2_terminal_time_valid CHECK (
        (status = 'running' AND ended_at IS NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT trace_spans_v2_attributes_is_object CHECK (jsonb_typeof(attributes) = 'object')
);

CREATE INDEX idx_trace_spans_v2_turn_sequence
    ON trace_spans_v2(turn_id, sequence);
