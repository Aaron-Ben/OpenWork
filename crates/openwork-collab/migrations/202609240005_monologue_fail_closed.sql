-- 连发检查（collaboration.md §9.4）：Agent 通过 reply / dm 发的消息记录所属 Run，
-- 同一个 Run 在同一房间的第 2 条放行。历史消息无从得知所属 Run，保持为空。
ALTER TABLE collab_messages
    ADD COLUMN run_id TEXT REFERENCES collab_runs(id) ON DELETE SET NULL;

-- triage 模型输出无法解析或 Engine 出错时 fail closed（collaboration.md §8.3）。
-- DROP CONSTRAINT 理由：CHECK 的取值清单需要加入 `fail_closed`，只能删掉重建。
ALTER TABLE collab_triages DROP CONSTRAINT collab_triages_source_check;
ALTER TABLE collab_triages
    ADD CONSTRAINT collab_triages_source_check CHECK (source IN (
        'empty_inbox', 'system_only', 'rate_limited', 'loop_cap',
        'local_model', 'deterministic', 'engine_error', 'human_dm', 'agent_dm_engage',
        'routing', 'lap_floor', 'fail_closed'
    ));
