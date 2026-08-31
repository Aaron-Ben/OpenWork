-- Collaboration owns one self-contained schema. Existing collab_*
-- business tables are discarded as one namespace; no legacy schema is
-- migrated or retained behind compatibility columns.
DO $$
DECLARE
    target RECORD;
BEGIN
    FOR target IN
        SELECT tablename
        FROM pg_tables
        WHERE schemaname = current_schema()
          AND tablename LIKE 'collab\_%' ESCAPE '\'
          AND tablename <> 'collab_schema_migrations'
    LOOP
        EXECUTE format('DROP TABLE IF EXISTS %I CASCADE', target.tablename);
    END LOOP;
END
$$;

CREATE TABLE collab_participants (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    display_name TEXT NOT NULL CHECK (btrim(display_name) <> ''),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_participants_id_format CHECK (
        id = 'local-user' OR id ~ '^[a-z][a-z0-9-]{0,47}$'
    ),
    CONSTRAINT collab_participants_local_user_shape CHECK (
        id <> 'local-user' OR (kind = 'user' AND display_name = 'User')
    )
);

INSERT INTO collab_participants (id, kind, display_name)
VALUES ('local-user', 'user', 'User');

CREATE OR REPLACE FUNCTION collab_protect_local_user() RETURNS TRIGGER AS $$
BEGIN
    IF OLD.id = 'local-user' THEN
        RAISE EXCEPTION 'local-user is immutable';
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER collab_participants_protect_local_user
BEFORE UPDATE OR DELETE ON collab_participants
FOR EACH ROW EXECUTE FUNCTION collab_protect_local_user();

CREATE TABLE collab_agent_profiles (
    agent_id TEXT PRIMARY KEY REFERENCES collab_participants(id) ON DELETE RESTRICT,
    role TEXT,
    persona TEXT NOT NULL CHECK (btrim(persona) <> ''),
    archived_at TIMESTAMP WITHOUT TIME ZONE,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);

CREATE TABLE collab_agent_runtime_configs (
    agent_id TEXT PRIMARY KEY REFERENCES collab_agent_profiles(agent_id) ON DELETE RESTRICT,
    engine_id TEXT NOT NULL CHECK (btrim(engine_id) <> ''),
    main_model_id TEXT NOT NULL CHECK (btrim(main_model_id) <> ''),
    triage_model_id TEXT NOT NULL CHECK (btrim(triage_model_id) <> ''),
    agenda_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    config_revision BIGINT NOT NULL DEFAULT 1 CHECK (config_revision > 0),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);

CREATE TABLE collab_rooms (
    id TEXT PRIMARY KEY CHECK (id ~ '^room-[0-9a-f]{32}$'),
    kind TEXT NOT NULL CHECK (kind IN ('group', 'direct')),
    title TEXT,
    direct_key TEXT,
    next_seq BIGINT NOT NULL DEFAULT 0 CHECK (next_seq >= 0),
    last_message_at TIMESTAMP WITHOUT TIME ZONE,
    created_by TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_rooms_shape_valid CHECK (
        (kind = 'direct' AND direct_key IS NOT NULL AND btrim(direct_key) <> '' AND title IS NULL) OR
        (kind = 'group' AND direct_key IS NULL AND title IS NOT NULL AND btrim(title) <> '')
    )
);
CREATE UNIQUE INDEX uq_collab_rooms_direct
    ON collab_rooms(direct_key) WHERE direct_key IS NOT NULL;

CREATE TABLE collab_room_members (
    room_id TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    last_read_seq BIGINT NOT NULL DEFAULT 0 CHECK (last_read_seq >= 0),
    muted BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (room_id, participant_id)
);
CREATE INDEX idx_collab_room_members_participant
    ON collab_room_members(participant_id, room_id);

CREATE TABLE collab_messages (
    id TEXT PRIMARY KEY CHECK (id ~ '^msg-[0-9a-f]{32}$'),
    room_id TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL CHECK (sequence > 0),
    author_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL DEFAULT 'normal' CHECK (kind IN ('normal', 'system')),
    body TEXT NOT NULL CHECK (btrim(body) <> ''),
    system_payload JSONB,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_messages_payload_only_system CHECK (
        system_payload IS NULL OR kind = 'system'
    ),
    CONSTRAINT collab_messages_payload_is_object CHECK (
        system_payload IS NULL OR jsonb_typeof(system_payload) = 'object'
    )
);
CREATE UNIQUE INDEX uq_collab_messages_room_seq ON collab_messages(room_id, sequence);
CREATE INDEX idx_collab_messages_room_seq_desc ON collab_messages(room_id, sequence DESC);

CREATE TABLE collab_agent_climates (
    agent_id TEXT NOT NULL REFERENCES collab_agent_profiles(agent_id) ON DELETE RESTRICT,
    about_participant_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    affinity DOUBLE PRECISION NOT NULL DEFAULT 0 CHECK (affinity BETWEEN -1 AND 1),
    trust DOUBLE PRECISION NOT NULL DEFAULT 0 CHECK (trust BETWEEN -1 AND 1),
    last_note TEXT,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (agent_id, about_participant_id),
    CONSTRAINT collab_agent_climates_not_self CHECK (agent_id <> about_participant_id)
);

CREATE TABLE collab_boards (
    id TEXT PRIMARY KEY CHECK (id ~ '^board-[0-9a-f]{32}$'),
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    description TEXT,
    created_by TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);

CREATE TABLE collab_board_columns (
    id TEXT PRIMARY KEY CHECK (id ~ '^col-[0-9a-f]{32}$'),
    board_id TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    position INTEGER NOT NULL CHECK (position >= 0),
    is_terminal BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT collab_board_columns_board_id_id_unique UNIQUE (board_id, id),
    CONSTRAINT collab_board_columns_position_unique
        UNIQUE (board_id, position) DEFERRABLE INITIALLY IMMEDIATE
);

CREATE TABLE collab_cards (
    id TEXT PRIMARY KEY CHECK (id ~ '^card-[0-9a-f]{32}$'),
    board_id TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE RESTRICT,
    column_id TEXT NOT NULL,
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    description TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    assignee_id TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    created_by TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_cards_column_in_board
        FOREIGN KEY (board_id, column_id)
        REFERENCES collab_board_columns(board_id, id)
        ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT collab_cards_position_unique
        UNIQUE (column_id, position) DEFERRABLE INITIALLY IMMEDIATE
);
CREATE INDEX idx_collab_cards_assignee
    ON collab_cards(assignee_id) WHERE assignee_id IS NOT NULL;

CREATE TABLE collab_runs (
    id TEXT PRIMARY KEY CHECK (id ~ '^run-[0-9a-f]{32}$'),
    agent_id TEXT NOT NULL REFERENCES collab_agent_profiles(agent_id) ON DELETE RESTRICT,
    runtime_session_id TEXT NOT NULL CHECK (btrim(runtime_session_id) <> ''),
    room_id TEXT REFERENCES collab_rooms(id) ON DELETE SET NULL,
    focus_card_id TEXT REFERENCES collab_cards(id) ON DELETE SET NULL,
    trigger TEXT NOT NULL CHECK (
        trigger IN ('message', 'rerun', 'reconnect', 'poll', 'agenda', 'user')
    ),
    trigger_reason TEXT,
    agenda_anchor_seq BIGINT CHECK (agenda_anchor_seq IS NULL OR agenda_anchor_seq >= 0),
    status TEXT NOT NULL CHECK (
        status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    engine_id TEXT NOT NULL CHECK (btrim(engine_id) <> ''),
    main_model_id TEXT NOT NULL CHECK (btrim(main_model_id) <> ''),
    triage_model_id TEXT NOT NULL CHECK (btrim(triage_model_id) <> ''),
    runtime_config_snapshot JSONB NOT NULL CHECK (jsonb_typeof(runtime_config_snapshot) = 'object'),
    inbox_carried_over BOOLEAN NOT NULL DEFAULT FALSE,
    started_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    heartbeat_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    ended_at TIMESTAMP WITHOUT TIME ZONE,
    input_tokens BIGINT,
    cached_input_tokens BIGINT,
    output_tokens BIGINT,
    rate_limit_percent DOUBLE PRECISION,
    error_code TEXT,
    error_message TEXT,
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('acted', 'silent', 'unpublished')),
    CONSTRAINT collab_runs_outcome_scope CHECK (
        (status = 'completed' AND outcome IS NOT NULL) OR
        (status <> 'completed' AND outcome IS NULL)
    ),
    CONSTRAINT collab_runs_terminal_time_valid CHECK (
        (status = 'running' AND ended_at IS NULL) OR
        (status <> 'running' AND ended_at IS NOT NULL)
    ),
    CONSTRAINT collab_runs_time_valid CHECK (
        heartbeat_at >= started_at AND (ended_at IS NULL OR ended_at >= started_at)
    ),
    CONSTRAINT collab_runs_agenda_focus_shape CHECK (
        (trigger = 'agenda' AND (focus_card_id IS NOT NULL OR room_id IS NOT NULL)
            AND trigger_reason IS NOT NULL AND btrim(trigger_reason) <> '') OR
        (trigger <> 'agenda' AND focus_card_id IS NULL AND agenda_anchor_seq IS NULL
            AND trigger_reason IS NULL)
    )
);
CREATE INDEX idx_collab_runs_agent_started ON collab_runs(agent_id, started_at DESC);
CREATE UNIQUE INDEX uq_collab_runs_agent_running
    ON collab_runs(agent_id) WHERE status = 'running';
CREATE INDEX idx_collab_runs_session_running
    ON collab_runs(runtime_session_id, heartbeat_at) WHERE status = 'running';

CREATE TABLE collab_run_deliveries (
    run_id TEXT NOT NULL REFERENCES collab_runs(id) ON DELETE CASCADE,
    room_id TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    from_seq BIGINT NOT NULL,
    up_to_seq BIGINT NOT NULL,
    eligible_reason TEXT CHECK (
        eligible_reason IS NULL OR eligible_reason IN ('action', 'ack', 'triage_false')
    ),
    eligible_at TIMESTAMP WITHOUT TIME ZONE,
    settled_at TIMESTAMP WITHOUT TIME ZONE,
    PRIMARY KEY (run_id, room_id),
    CONSTRAINT collab_run_deliveries_range_valid CHECK (
        from_seq > 0 AND up_to_seq >= from_seq
    ),
    CONSTRAINT collab_run_deliveries_eligible_shape CHECK (
        (eligible_reason IS NULL AND eligible_at IS NULL AND settled_at IS NULL) OR
        (eligible_reason IS NOT NULL AND eligible_at IS NOT NULL)
    ),
    CONSTRAINT collab_run_deliveries_settle_after_eligible CHECK (
        settled_at IS NULL OR settled_at >= eligible_at
    )
);
CREATE INDEX idx_collab_run_deliveries_unsettled
    ON collab_run_deliveries(run_id)
    WHERE eligible_at IS NOT NULL AND settled_at IS NULL;

CREATE TABLE collab_triages (
    id TEXT PRIMARY KEY CHECK (id ~ '^triage-[0-9a-f]{32}$'),
    run_id TEXT REFERENCES collab_runs(id) ON DELETE SET NULL,
    agent_id TEXT NOT NULL REFERENCES collab_agent_profiles(agent_id) ON DELETE RESTRICT,
    runtime_session_id TEXT NOT NULL CHECK (btrim(runtime_session_id) <> ''),
    room_id TEXT REFERENCES collab_rooms(id) ON DELETE CASCADE,
    up_to_seq BIGINT NOT NULL CHECK (up_to_seq >= 0),
    actionable BOOLEAN NOT NULL,
    response_mode TEXT CHECK (response_mode IS NULL OR response_mode IN ('me', 'each', 'one_of_us')),
    source TEXT NOT NULL CHECK (source IN (
        'empty_inbox', 'system_only', 'rate_limited', 'loop_cap',
        'local_model', 'deterministic', 'engine_error', 'human_dm', 'agent_dm_engage'
    )),
    reason TEXT,
    prompt_note TEXT,
    engine_id TEXT NOT NULL CHECK (btrim(engine_id) <> ''),
    model_id TEXT,
    input_tokens BIGINT,
    output_tokens BIGINT,
    latency_ms BIGINT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_triages_usage_nonneg CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (latency_ms IS NULL OR latency_ms >= 0)
    )
);
CREATE INDEX idx_collab_triages_created ON collab_triages(created_at);
CREATE INDEX idx_collab_triages_room_created ON collab_triages(room_id, created_at DESC);

CREATE TABLE collab_command_requests (
    id TEXT PRIMARY KEY CHECK (id ~ '^cmd-[0-9a-f]{32}$'),
    runtime_session_id TEXT NOT NULL CHECK (btrim(runtime_session_id) <> ''),
    run_id TEXT REFERENCES collab_runs(id) ON DELETE CASCADE,
    request_id TEXT NOT NULL CHECK (request_id ~ '^req-[0-9a-f]{32}$'),
    actor_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    semantic_hash TEXT NOT NULL CHECK (semantic_hash ~ '^sha256:[0-9a-f]{64}$'),
    result JSONB CHECK (result IS NULL OR jsonb_typeof(result) = 'object'),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);
CREATE UNIQUE INDEX uq_collab_agent_command_request
    ON collab_command_requests(run_id, request_id) WHERE run_id IS NOT NULL;
CREATE UNIQUE INDEX uq_collab_desktop_command_request
    ON collab_command_requests(runtime_session_id, request_id) WHERE run_id IS NULL;

CREATE TABLE collab_engine_inventory (
    engine_id TEXT PRIMARY KEY CHECK (btrim(engine_id) <> ''),
    status TEXT NOT NULL CHECK (status IN ('unknown', 'ready', 'missing', 'error')),
    version TEXT,
    checked_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    last_error TEXT,
    observed_session_id TEXT NOT NULL CHECK (btrim(observed_session_id) <> ''),
    CONSTRAINT collab_engine_inventory_shape CHECK (
        (status = 'error' AND last_error IS NOT NULL) OR status <> 'error'
    )
);

DELETE FROM collab_schema_migrations;
