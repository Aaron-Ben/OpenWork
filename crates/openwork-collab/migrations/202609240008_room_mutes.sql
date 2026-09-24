-- 静音（collaboration.md §10.1）：Agent 可以一直静音或静音到某个时刻，布尔的 `muted` 表达不了到期时间。
-- `mute_expires_at` 为空或已过去表示没有静音，`infinity` 表示一直静音到 follow。
-- 回填口径：旧 `muted` 为真的成员视为一直静音。此前没有任何入口能把它设为真，实际不会有这样的行。
-- 删除 `muted`：语义已完整转入 `mute_expires_at`，不保留两份。
ALTER TABLE collab_room_members ADD COLUMN mute_expires_at TIMESTAMP WITHOUT TIME ZONE;

UPDATE collab_room_members SET mute_expires_at = 'infinity' WHERE muted;

ALTER TABLE collab_room_members DROP COLUMN muted;
