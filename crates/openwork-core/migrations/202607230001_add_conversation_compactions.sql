CREATE TABLE conversation_compactions (
    id                          TEXT PRIMARY KEY,
    session_id                  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence                    BIGINT NOT NULL,
    through_message_sequence    BIGINT NOT NULL,
    source_message_count        INTEGER NOT NULL,
    resolved_model_name         TEXT NOT NULL,
    summary                     TEXT NOT NULL,
    input_tokens                BIGINT,
    output_tokens               BIGINT,
    created_at                  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
    CONSTRAINT conversation_compactions_id_not_blank
        CHECK (btrim(id) <> ''),
    CONSTRAINT conversation_compactions_sequence_positive
        CHECK (sequence > 0),
    CONSTRAINT conversation_compactions_message_sequence_non_negative
        CHECK (through_message_sequence >= 0),
    CONSTRAINT conversation_compactions_source_count_positive
        CHECK (source_message_count > 0),
    CONSTRAINT conversation_compactions_model_not_blank
        CHECK (btrim(resolved_model_name) <> ''),
    CONSTRAINT conversation_compactions_summary_not_blank
        CHECK (btrim(summary) <> ''),
    CONSTRAINT conversation_compactions_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0)
    ),
    CONSTRAINT conversation_compactions_session_sequence
        UNIQUE (session_id, sequence)
);

CREATE INDEX idx_conversation_compactions_session_sequence
    ON conversation_compactions(session_id, sequence DESC);
