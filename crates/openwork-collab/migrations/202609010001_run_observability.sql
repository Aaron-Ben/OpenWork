CREATE TABLE collab_run_events (
    id TEXT PRIMARY KEY CHECK (id ~ '^event-[0-9a-f]{32}$'),
    run_id TEXT NOT NULL REFERENCES collab_runs(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('runner', 'engine')),
    kind TEXT NOT NULL CHECK (kind IN (
        'triage.started',
        'engine.started',
        'engine.completed',
        'engine.failed',
        'engine.cancelled'
    )),
    level TEXT NOT NULL CHECK (level IN ('info', 'warning', 'error')),
    data JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(data) = 'object'),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);
CREATE INDEX idx_collab_run_events_run_created
    ON collab_run_events(run_id, created_at, id);
CREATE INDEX idx_collab_triages_run_created
    ON collab_triages(run_id, created_at, id) WHERE run_id IS NOT NULL;
