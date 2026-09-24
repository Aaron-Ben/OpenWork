-- 会话的沙箱模式（docs/permissions.md §2.2、§6.3）：`auto` 或 `accept_edits`，随会话持久化，
-- 进程重启后恢复，免得用户刻意切到 `accept-edits` 的会话在重启后悄悄放宽。
-- 子 Agent 会话在派生时写入"父会话模式与角色上限中较窄者"的快照（§6.6）。
--
-- 已有会话的取值：旧的模式从未落盘，没有要保留的值。根会话取默认的 `auto`（开发计划 §5.2）；
-- 子 Agent 会话取 `accept_edits`——现有子 Agent 角色只有 explorer，它的上限就是 `accept_edits`，
-- 取 `auto` 会让恢复后的 explorer 越过自己的上限。

ALTER TABLE sessions
    ADD COLUMN sandbox_mode TEXT NOT NULL DEFAULT 'auto'
        CONSTRAINT sessions_sandbox_mode_valid CHECK (sandbox_mode IN ('auto', 'accept_edits'));

UPDATE sessions SET sandbox_mode = 'accept_edits' WHERE parent_session_id IS NOT NULL;
