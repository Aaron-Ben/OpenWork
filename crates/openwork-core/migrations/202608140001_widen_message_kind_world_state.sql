-- World State 变化后会作为独立消息写入 messages，因此扩大 message_kind 的合法集合。
-- PostgreSQL 不能原地修改 CHECK 表达式，只能先删除再以原名重建；已有行不会丢失。

ALTER TABLE messages DROP CONSTRAINT messages_kind_valid;
ALTER TABLE messages
    ADD CONSTRAINT messages_kind_valid
        CHECK (message_kind IN ('normal', 'skill_instruction', 'agent_message', 'world_state'));
