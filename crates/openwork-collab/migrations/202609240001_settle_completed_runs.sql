-- 成功完成的 Run 结算它携带的全部 delivery（collaboration.md §7）。
-- Agent 没有回复也没有 ack 时记为 `completed`；此前这批消息永不结算，
-- 保持沉默的 Agent 会在每次 poll 被同一条 User 消息重新唤醒。
-- DROP CONSTRAINT 理由：列内 CHECK 的取值清单需要加入 `completed`，只能删掉重建。
ALTER TABLE collab_run_deliveries
    DROP CONSTRAINT collab_run_deliveries_eligible_reason_check;
ALTER TABLE collab_run_deliveries
    ADD CONSTRAINT collab_run_deliveries_eligible_reason_check CHECK (
        eligible_reason IS NULL
        OR eligible_reason IN ('action', 'ack', 'triage_false', 'completed')
    );
