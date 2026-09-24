-- lap floor（collaboration.md §8.3、§13.3.4）：用户在 Desktop 看到的最大 sequence 算作人类关注，
-- 这之前的 Agent 消息不计入“一轮”。放在房间上，因为用户可以查看自己不是成员的 Agent 私聊。
ALTER TABLE collab_rooms
    ADD COLUMN user_viewed_seq BIGINT NOT NULL DEFAULT 0
        CONSTRAINT collab_rooms_user_viewed_seq_valid CHECK (user_viewed_seq >= 0);

-- triage 可以因 lap floor 确定性跳过。
-- DROP CONSTRAINT 理由：CHECK 的取值清单需要加入 `lap_floor`，只能删掉重建。
ALTER TABLE collab_triages DROP CONSTRAINT collab_triages_source_check;
ALTER TABLE collab_triages
    ADD CONSTRAINT collab_triages_source_check CHECK (source IN (
        'empty_inbox', 'system_only', 'rate_limited', 'loop_cap',
        'local_model', 'deterministic', 'engine_error', 'human_dm', 'agent_dm_engage',
        'routing', 'lap_floor'
    ));
