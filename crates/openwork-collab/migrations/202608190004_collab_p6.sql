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
