CREATE TABLE collab_participants (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at   TIMESTAMP WITHOUT TIME ZONE NOT NULL
                 DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_participants_kind_valid
        CHECK (kind IN ('user', 'agent')),
    CONSTRAINT collab_participants_id_format
        CHECK (id ~ '^[a-z][a-z0-9_]{0,47}$'),
    CONSTRAINT collab_participants_name_not_blank
        CHECK (btrim(display_name) <> '')
);

INSERT INTO collab_participants (id, kind, display_name)
VALUES ('user', 'user', 'User');

CREATE TABLE collab_agents (
    id                  TEXT PRIMARY KEY
                        REFERENCES collab_participants(id) ON DELETE CASCADE,
    role                TEXT,
    bio                 TEXT,
    system_prompt       TEXT NOT NULL,
    provider_id         TEXT NOT NULL,
    model_id            TEXT NOT NULL,
    opencode_session_id TEXT,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL
                        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_agents_prompt_not_blank CHECK (btrim(system_prompt) <> '')
);

CREATE TABLE collab_rooms (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,
    title           TEXT,
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

CREATE TABLE collab_room_members (
    room_id        TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    last_read_seq  BIGINT NOT NULL DEFAULT 0,
    muted          BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at      TIMESTAMP WITHOUT TIME ZONE NOT NULL
                   DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (room_id, participant_id),
    CONSTRAINT collab_room_members_seq_nonneg CHECK (last_read_seq >= 0)
);

CREATE TABLE collab_messages (
    id             TEXT PRIMARY KEY,
    room_id        TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    sequence       BIGINT NOT NULL,
    author_id      TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    kind           TEXT NOT NULL DEFAULT 'normal',
    body           TEXT NOT NULL,
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

CREATE UNIQUE INDEX uq_collab_messages_room_seq
    ON collab_messages(room_id, sequence);
CREATE INDEX idx_collab_messages_room_seq_desc
    ON collab_messages(room_id, sequence DESC);

CREATE TABLE collab_settings (
    id                 TEXT PRIMARY KEY DEFAULT 'singleton',
    triage_provider_id TEXT,
    triage_model_id    TEXT,
    opencode_base_url  TEXT,
    updated_at         TIMESTAMP WITHOUT TIME ZONE NOT NULL
                       DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_settings_singleton CHECK (id = 'singleton'),
    CONSTRAINT collab_settings_triage_pair CHECK (
        (triage_provider_id IS     NULL AND triage_model_id IS     NULL) OR
        (triage_provider_id IS NOT NULL AND triage_model_id IS NOT NULL)
    )
);

INSERT INTO collab_settings (id) VALUES ('singleton');

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
    CONSTRAINT collab_runs_trigger_valid CHECK (
        trigger IN ('message', 'rerun', 'idle', 'agenda', 'scanner', 'user')
    ),
    CONSTRAINT collab_runs_status_valid CHECK (
        status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    CONSTRAINT collab_runs_terminal_time_valid CHECK (
        (status =  'running' AND ended_at IS     NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT collab_runs_end_after_start CHECK (
        ended_at IS NULL OR ended_at >= started_at
    )
);

CREATE INDEX idx_collab_runs_agent_started
    ON collab_runs(agent_id, started_at DESC);
