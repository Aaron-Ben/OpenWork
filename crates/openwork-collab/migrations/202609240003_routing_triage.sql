-- 点名路由（collaboration.md §8.2、§8.3）：triage 结论可以来自路由题，source 为 `routing`。
-- DROP CONSTRAINT 理由：列内 CHECK 的取值清单需要加入 `routing`，只能删掉重建。
ALTER TABLE collab_triages DROP CONSTRAINT collab_triages_source_check;
ALTER TABLE collab_triages
    ADD CONSTRAINT collab_triages_source_check CHECK (source IN (
        'empty_inbox', 'system_only', 'rate_limited', 'loop_cap',
        'local_model', 'deterministic', 'engine_error', 'human_dm', 'agent_dm_engage',
        'routing'
    ));
