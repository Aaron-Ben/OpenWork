-- Turn 级任务清单（update_plan）的当前快照。
--
-- 只保留最新快照，不建 plan_revisions：每次调用的事实已经由 messages 里的 Assistant
-- Tool Call 记录下来了，这张表回答的是"现在的计划是什么"，不是"改过几次"。
-- 也不拆 plan_steps 子表：V1 只整体替换和整体读取，拆表徒增写入与排序复杂度。
CREATE TABLE turn_plans (
    -- 计划属于 Turn 而不是 Session，turn_id 已唯一定位所属 Session，不加冗余列。
    turn_id       TEXT PRIMARY KEY
                  REFERENCES turns(id) ON DELETE CASCADE,
    explanation   TEXT,
    steps         JSONB NOT NULL,
    updated_at    TIMESTAMP WITHOUT TIME ZONE NOT NULL
                  DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),

    -- 数据库只兜底顶层类型与外键生命周期。每个元素的形状、以及"最多一个 in_progress"
    -- 这类跨元素不变量由应用层校验：后者 CHECK 表达不了，前者放这里会与 Rust 侧的
    -- 校验形成两份会漂移的真相。
    CONSTRAINT turn_plans_steps_is_array
        CHECK (jsonb_typeof(steps) = 'array')
);

-- 加载会话时按 Session 一次取回全部 Turn 的计划，避免按 Turn N+1 查询。
CREATE INDEX turn_plans_turn_id_idx ON turn_plans (turn_id);
