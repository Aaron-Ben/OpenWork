-- 旧工具结果修剪的水位线（docs/compaction.md §1.1）：序号不超过它的消息里，
-- 过长的 Tool Result 在模型投影中保持修剪状态。它只增不减；NULL 表示从未修剪。
-- 已有会话取 NULL，与"尚未修剪过"的语义一致，不需要回填。

ALTER TABLE sessions ADD COLUMN tool_result_pruned_through_sequence BIGINT;
ALTER TABLE sessions
    ADD CONSTRAINT sessions_tool_result_pruned_through_positive
        CHECK (tool_result_pruned_through_sequence IS NULL OR tool_result_pruned_through_sequence > 0);
