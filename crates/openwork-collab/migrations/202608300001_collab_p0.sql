CREATE TABLE collab_participants (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    display_name TEXT NOT NULL CHECK (btrim(display_name) <> ''),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_participants_id_format
        CHECK (id ~ '^[a-z][a-z0-9_]{0,47}$')
);

INSERT INTO collab_participants (id, kind, display_name)
VALUES ('user', 'user', 'User');

CREATE TABLE collab_computers (
    id TEXT PRIMARY KEY CHECK (id = 'local'),
    name TEXT NOT NULL CHECK (btrim(name) <> ''),
    status TEXT NOT NULL DEFAULT 'offline' CHECK (status IN ('online', 'offline')),
    last_seen_at TIMESTAMP WITHOUT TIME ZONE,
    credential_hash TEXT NOT NULL
        CHECK (credential_hash ~ '^sha256:[0-9a-f]{64}$'),
    registered_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    daemon_version TEXT,
    daemon_supervised BOOLEAN NOT NULL DEFAULT FALSE,
    daemon_generation BIGINT NOT NULL DEFAULT 0 CHECK (daemon_generation >= 0),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);

CREATE TABLE collab_computer_engines (
    computer_id TEXT NOT NULL REFERENCES collab_computers(id) ON DELETE CASCADE,
    engine_id TEXT NOT NULL CHECK (engine_id = 'opencode'),
    status TEXT NOT NULL DEFAULT 'unknown' CHECK (
        status IN ('unknown', 'ready', 'missing', 'unauthenticated', 'broken')
    ),
    version TEXT CHECK (version IS NULL OR btrim(version) <> ''),
    checked_at TIMESTAMP WITHOUT TIME ZONE,
    probe_version BIGINT NOT NULL DEFAULT 0 CHECK (probe_version >= 0),
    PRIMARY KEY (computer_id, engine_id),
    CONSTRAINT collab_computer_engines_probe_shape CHECK (
        (status = 'unknown' AND checked_at IS NULL AND version IS NULL) OR
        (status <> 'unknown' AND checked_at IS NOT NULL)
    )
);

CREATE TABLE collab_agents (
    id TEXT PRIMARY KEY REFERENCES collab_participants(id) ON DELETE CASCADE,
    computer_id TEXT NOT NULL REFERENCES collab_computers(id) ON DELETE RESTRICT
        CHECK (computer_id = 'local'),
    role TEXT,
    bio TEXT,
    system_prompt TEXT NOT NULL CHECK (btrim(system_prompt) <> ''),
    engine_id TEXT NOT NULL CHECK (engine_id = 'opencode'),
    model TEXT CHECK (model IS NULL OR btrim(model) <> ''),
    fast_model TEXT CHECK (fast_model IS NULL OR btrim(fast_model) <> ''),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    scanner_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    config_version BIGINT NOT NULL DEFAULT 1 CHECK (config_version > 0),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    FOREIGN KEY (computer_id, engine_id)
        REFERENCES collab_computer_engines(computer_id, engine_id) ON DELETE RESTRICT
);
CREATE INDEX idx_collab_agents_computer ON collab_agents(computer_id, enabled);

CREATE TABLE collab_rooms (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('group', 'direct')),
    title TEXT,
    direct_key TEXT,
    next_seq BIGINT NOT NULL DEFAULT 0 CHECK (next_seq >= 0),
    last_message_at TIMESTAMP WITHOUT TIME ZONE,
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
    id TEXT PRIMARY KEY,
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

CREATE TABLE collab_runs (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL REFERENCES collab_agents(id) ON DELETE CASCADE,
    computer_id TEXT NOT NULL REFERENCES collab_computers(id) ON DELETE RESTRICT,
    room_id TEXT REFERENCES collab_rooms(id) ON DELETE SET NULL,
    trigger TEXT NOT NULL CHECK (
        trigger IN ('message', 'rerun', 'reconnect', 'poll', 'agenda', 'scanner', 'user')
    ),
    status TEXT NOT NULL CHECK (
        status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    engine_id TEXT NOT NULL CHECK (engine_id = 'opencode'),
    model TEXT,
    computer_generation BIGINT NOT NULL CHECK (computer_generation > 0),
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
    )
);
CREATE INDEX idx_collab_runs_agent_started ON collab_runs(agent_id, started_at DESC);
CREATE UNIQUE INDEX uq_collab_runs_agent_running
    ON collab_runs(agent_id) WHERE status = 'running';
CREATE INDEX idx_collab_runs_computer_running
    ON collab_runs(computer_id, heartbeat_at) WHERE status = 'running';

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
    id TEXT PRIMARY KEY,
    run_id TEXT REFERENCES collab_runs(id) ON DELETE SET NULL,
    agent_id TEXT NOT NULL REFERENCES collab_agents(id) ON DELETE CASCADE,
    computer_id TEXT NOT NULL REFERENCES collab_computers(id) ON DELETE RESTRICT,
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
    engine_id TEXT NOT NULL CHECK (engine_id = 'opencode'),
    model TEXT,
    input_tokens BIGINT,
    output_tokens BIGINT,
    latency_ms BIGINT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_triages_room_scope CHECK (
        (source = 'empty_inbox' AND run_id IS NULL AND room_id IS NULL AND up_to_seq = 0) OR
        (source <> 'empty_inbox' AND room_id IS NOT NULL)
    ),
    CONSTRAINT collab_triages_nonaction_sources CHECK (
        source NOT IN ('empty_inbox', 'rate_limited', 'loop_cap', 'engine_error') OR
        actionable = FALSE
    ),
    CONSTRAINT collab_triages_run_scope CHECK (
        run_id IS NOT NULL OR source IN ('empty_inbox', 'rate_limited', 'loop_cap')
    ),
    CONSTRAINT collab_triages_usage_nonneg CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (latency_ms IS NULL OR latency_ms >= 0)
    )
);
CREATE INDEX idx_collab_triages_created ON collab_triages(created_at);
CREATE INDEX idx_collab_triages_room_created ON collab_triages(room_id, created_at DESC);

CREATE TABLE collab_events (
    id TEXT PRIMARY KEY,
    run_id TEXT REFERENCES collab_runs(id) ON DELETE CASCADE,
    agent_id TEXT REFERENCES collab_agents(id) ON DELETE CASCADE,
    computer_id TEXT REFERENCES collab_computers(id) ON DELETE RESTRICT,
    room_id TEXT REFERENCES collab_rooms(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (btrim(kind) <> ''),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(payload) = 'object'),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);
CREATE INDEX idx_collab_events_created ON collab_events(created_at);
CREATE INDEX idx_collab_events_run ON collab_events(run_id) WHERE run_id IS NOT NULL;
CREATE INDEX idx_collab_events_computer_created
    ON collab_events(computer_id, created_at DESC) WHERE computer_id IS NOT NULL;
