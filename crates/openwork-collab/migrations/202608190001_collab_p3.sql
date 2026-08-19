CREATE TABLE collab_reactions (
    message_id TEXT NOT NULL REFERENCES collab_messages(id) ON DELETE CASCADE,
    actor_id   TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    emoji      TEXT NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (message_id, actor_id, emoji)
);

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
    CONSTRAINT collab_triages_source_valid CHECK (source IN (
        'empty_inbox', 'system_only', 'rate_limited', 'loop_cap',
        'support_model', 'fail_open', 'fail_closed', 'human_dm', 'dm_agent_engage'
    ))
);

CREATE INDEX idx_collab_triages_created ON collab_triages(created_at);
