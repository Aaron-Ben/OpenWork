-- Turn 收尾时还没标成 completed 的计划步骤数。
--
-- 这一列不是约束，是观测点。Prompt 里有"结束前把所有步骤置为 completed"这条规则，但
-- 它无法用 CHECK 强制：Turn 因错误、取消或达到调用上限而终止时，留下未完成步骤是
-- **正确**的记录。所以规则只能靠提示词引导，而引导是否生效必须能被查询证伪，否则
-- 只能靠人翻聊天记录。
--
-- 与 model_call_count / tool_call_count 同性质：turns 上有意保留的聚合列，独立于
-- trace_spans。trace 的 payload 有保留期会被清理，不适合承载跨月对比的指标。
--
-- 典型查询（只看正常完成的 Turn，出错和取消要排除）：
--   SELECT count(*) FILTER (WHERE plan_unfinished_step_count > 0) AS forgot_to_finish,
--          count(*)                                               AS completed_with_plan
--   FROM turns
--   WHERE status = 'completed' AND plan_unfinished_step_count IS NOT NULL;
ALTER TABLE turns ADD COLUMN plan_unfinished_step_count INTEGER;

-- NULL 与 0 是两件事，不要给默认值把它们合并掉：
--   NULL = 这个 Turn 根本没建计划（简单任务不该建，是正常的）
--   0    = 建了计划并且全部收尾（规则生效）
--   > 0  = 建了计划但没收尾（status = 'completed' 时才可疑）
-- 合并成 0 会让"没用计划"和"用了且做对了"无法区分，分母就错了。
ALTER TABLE turns
    ADD CONSTRAINT turns_plan_unfinished_non_negative
        CHECK (plan_unfinished_step_count IS NULL OR plan_unfinished_step_count >= 0);
