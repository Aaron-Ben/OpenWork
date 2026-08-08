-- 子 Agent 从属关系。设计见 docs/multi-agent.md，DDL 理由见 docs/data-model.md。
--
-- 子 Agent 本身就是一个 Session：它有自己的 sessions / turns / messages 行，
-- 复用同一个 SessionActor 与唯一的 Agent Loop。这四列只表达从属关系本身。

ALTER TABLE sessions
    ADD COLUMN parent_session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    -- 父会话内唯一的可读名，模型用它寻址。
    ADD COLUMN task_name         TEXT,
    ADD COLUMN agent_role        TEXT,
    -- 发起它的 Tool Call Span。Trace 是 best-effort，Span 可能因队列满而根本没落库，
    -- 因此不建外键——与 trace.md §2「结构标识不建外键」一致。
    ADD COLUMN spawn_span_id     TEXT;

-- 前三列同生共死：要么全是根会话，要么全是子 Agent。
-- 半填充状态（有父但没名字）无法寻址，直接由数据库拒绝。
ALTER TABLE sessions
    ADD CONSTRAINT sessions_subagent_fields_consistent CHECK (
        (parent_session_id IS     NULL AND task_name IS     NULL AND agent_role IS     NULL) OR
        (parent_session_id IS NOT NULL AND task_name IS NOT NULL AND agent_role IS NOT NULL)
    );

ALTER TABLE sessions
    ADD CONSTRAINT sessions_task_name_format CHECK (
        task_name IS NULL OR task_name ~ '^[a-z][a-z0-9_]{0,47}$'
    );

-- 深度上限 1 的兜底。工具面不给子 Agent 注册 spawn_agent 是第一道防线，
-- 但那是运行时决策；写错一次就没有第二道防线，所以数据库也要拦。
-- 自引用禁止只挡住直接自环；真正的深度由插入路径保证父必须是根会话。
ALTER TABLE sessions
    ADD CONSTRAINT sessions_spawn_not_self CHECK (
        parent_session_id IS NULL OR parent_session_id <> id
    );

CREATE UNIQUE INDEX uq_sessions_parent_task_name
    ON sessions(parent_session_id, task_name)
    WHERE parent_session_id IS NOT NULL;

CREATE INDEX idx_sessions_parent
    ON sessions(parent_session_id, created_at)
    WHERE parent_session_id IS NOT NULL;

-- 子 Agent 回传给父的消息落在 messages 上，需要一个新的 message_kind 取值。
-- 这里 DROP 再 ADD 是因为 PostgreSQL 不支持原地修改 CHECK 的表达式；
-- 约束名与语义都不变，只是取值集合扩大，不丢弃任何既有行。
ALTER TABLE messages DROP CONSTRAINT messages_kind_valid;
ALTER TABLE messages
    ADD CONSTRAINT messages_kind_valid
        CHECK (message_kind IN ('normal', 'skill_instruction', 'agent_message'));
