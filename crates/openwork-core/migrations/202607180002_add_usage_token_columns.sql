ALTER TABLE turns
    ADD COLUMN reasoning_tokens BIGINT,
    ADD COLUMN total_tokens BIGINT GENERATED ALWAYS AS (
        CASE
            WHEN input_tokens IS NULL OR output_tokens IS NULL THEN NULL
            ELSE input_tokens + output_tokens
        END
    ) STORED,
    ADD CONSTRAINT turns_reasoning_tokens_non_negative
        CHECK (reasoning_tokens IS NULL OR reasoning_tokens >= 0);

ALTER TABLE trace_spans
    ADD COLUMN reasoning_tokens BIGINT,
    ADD COLUMN total_tokens BIGINT GENERATED ALWAYS AS (
        CASE
            WHEN input_tokens IS NULL OR output_tokens IS NULL THEN NULL
            ELSE input_tokens + output_tokens
        END
    ) STORED,
    ADD CONSTRAINT trace_spans_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (cached_input_tokens IS NULL OR cached_input_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
    );
