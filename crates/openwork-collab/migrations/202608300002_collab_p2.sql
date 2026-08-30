ALTER TABLE collab_runs
    ADD COLUMN inbox_carried_over BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE collab_cli_requests (
    run_id TEXT NOT NULL REFERENCES collab_runs(id) ON DELETE CASCADE,
    request_id TEXT NOT NULL CHECK (
        request_id ~ '^cli_[a-zA-Z0-9_-]{1,96}$'
    ),
    agent_id TEXT NOT NULL REFERENCES collab_agents(id) ON DELETE CASCADE,
    argv_hash TEXT NOT NULL CHECK (argv_hash ~ '^sha256:[0-9a-f]{64}$'),
    dedupe_key TEXT NOT NULL CHECK (dedupe_key ~ '^sha256:[0-9a-f]{64}$'),
    result JSONB CHECK (result IS NULL OR jsonb_typeof(result) = 'object'),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (run_id, request_id),
    UNIQUE (run_id, dedupe_key)
);

CREATE INDEX idx_collab_cli_requests_created
    ON collab_cli_requests(created_at);

CREATE TABLE collab_reactions (
    message_id TEXT NOT NULL REFERENCES collab_messages(id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL REFERENCES collab_participants(id) ON DELETE RESTRICT,
    emoji TEXT NOT NULL CHECK (btrim(emoji) <> ''),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    PRIMARY KEY (message_id, actor_id, emoji)
);
