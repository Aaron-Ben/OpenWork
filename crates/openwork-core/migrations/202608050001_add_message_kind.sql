ALTER TABLE messages
    ADD COLUMN message_kind TEXT NOT NULL DEFAULT 'normal',
    ADD CONSTRAINT messages_kind_valid
        CHECK (message_kind IN ('normal', 'skill_instruction'));
