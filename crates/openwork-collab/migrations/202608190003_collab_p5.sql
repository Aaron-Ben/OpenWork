-- P5 scanner is an explicit per-Agent capability and is disabled by default.
ALTER TABLE collab_agents
    ADD COLUMN scanner_enabled BOOLEAN NOT NULL DEFAULT FALSE;
