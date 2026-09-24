-- 卡片唤醒持久化（collaboration.md §11.4、§13.3.6）：改派与新增 @ 写入待处理记录，Run 成功后结算。
CREATE TABLE collab_card_wakes (
    id TEXT PRIMARY KEY CHECK (id ~ '^cardwake-[0-9a-f]{32}$'),
    agent_id TEXT NOT NULL REFERENCES collab_agent_profiles(agent_id) ON DELETE RESTRICT,
    card_id TEXT NOT NULL REFERENCES collab_cards(id) ON DELETE CASCADE,
    reason TEXT NOT NULL CHECK (reason IN ('assigned', 'mentioned')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    run_id TEXT REFERENCES collab_runs(id) ON DELETE SET NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    settled_at TIMESTAMP WITHOUT TIME ZONE,
    CONSTRAINT collab_card_wakes_time_valid CHECK (
        updated_at >= created_at AND (settled_at IS NULL OR settled_at >= updated_at)
    )
);
-- 每个 Agent 每张卡片只有一条待处理记录，反复编辑合并进这一条。
CREATE UNIQUE INDEX uq_collab_card_wakes_pending
    ON collab_card_wakes(agent_id, card_id) WHERE settled_at IS NULL;
CREATE INDEX idx_collab_card_wakes_run ON collab_card_wakes(run_id) WHERE settled_at IS NULL;

-- 卡片唤醒开的 Run 以 `card` 为 trigger。
-- DROP CONSTRAINT 理由：CHECK 的取值清单需要加入 `card`，只能删掉重建。
ALTER TABLE collab_runs DROP CONSTRAINT collab_runs_trigger_check;
ALTER TABLE collab_runs
    ADD CONSTRAINT collab_runs_trigger_check CHECK (
        trigger IN ('message', 'card', 'rerun', 'reconnect', 'poll', 'agenda', 'user')
    );
