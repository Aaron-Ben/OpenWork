-- 引用回复（collaboration.md §9.3、§13.3.3）：消息可以引用同一房间里的另一条消息。
-- (room_id, id) 唯一约束供复合外键使用，保证被引用的消息与回复在同一房间。
ALTER TABLE collab_messages
    ADD CONSTRAINT collab_messages_room_message_unique UNIQUE (room_id, id);
ALTER TABLE collab_messages ADD COLUMN quoted_message_id TEXT;
ALTER TABLE collab_messages
    ADD CONSTRAINT collab_messages_quote_in_same_room
    FOREIGN KEY (room_id, quoted_message_id) REFERENCES collab_messages (room_id, id);
