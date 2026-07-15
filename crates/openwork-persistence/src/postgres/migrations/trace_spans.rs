use super::Migration;

/// Best-effort diagnostic projection. Unlike `recorded_events`, rows are mutable
/// because a running Span is completed with an UPSERT using the same span id.
pub const TRACE_SPAN_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 202607150101,
        name: "create_trace_spans",
        statements: &[
            r#"CREATE TABLE IF NOT EXISTS trace_spans (
           span_id TEXT PRIMARY KEY,
           trace_id TEXT NOT NULL,
           parent_span_id TEXT,
           span_kind TEXT NOT NULL,
           span_name TEXT NOT NULL,
           status TEXT NOT NULL,
           session_id TEXT NOT NULL,
           turn_id TEXT NOT NULL,
           step_id TEXT,
           tool_run_id TEXT,
           started_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
           ended_at TIMESTAMP WITHOUT TIME ZONE,
           attributes_json JSONB NOT NULL DEFAULT '{}'::JSONB,
           error_type TEXT,
           error_code TEXT,
           error_message TEXT,
           created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
           updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
           CONSTRAINT trace_spans_span_id_not_blank CHECK (btrim(span_id) <> ''),
           CONSTRAINT trace_spans_trace_id_not_blank CHECK (btrim(trace_id) <> ''),
           CONSTRAINT trace_spans_name_not_blank CHECK (btrim(span_name) <> ''),
           CONSTRAINT trace_spans_session_id_not_blank CHECK (btrim(session_id) <> ''),
           CONSTRAINT trace_spans_turn_id_not_blank CHECK (btrim(turn_id) <> ''),
           CONSTRAINT trace_spans_kind_valid CHECK (span_kind IN (
               'turn', 'step', 'model_attempt', 'transport_attempt',
               'tool_run', 'approval', 'recovery'
           )),
           CONSTRAINT trace_spans_status_valid CHECK (status IN (
               'running', 'waiting', 'succeeded', 'failed',
               'cancelled', 'denied', 'outcome_unknown'
           )),
           CONSTRAINT trace_spans_attributes_is_object
               CHECK (jsonb_typeof(attributes_json) = 'object'),
           CONSTRAINT trace_spans_end_after_start
               CHECK (ended_at IS NULL OR ended_at >= started_at)
         )"#,
            "CREATE INDEX IF NOT EXISTS idx_trace_spans_session_started ON trace_spans(session_id, started_at, span_id)",
            "CREATE INDEX IF NOT EXISTS idx_trace_spans_turn_started ON trace_spans(turn_id, started_at, span_id)",
            "CREATE INDEX IF NOT EXISTS idx_trace_spans_trace_parent ON trace_spans(trace_id, parent_span_id)",
        ],
    },
    Migration {
        version: 202607150102,
        name: "index_recent_trace_turns",
        statements: &[
            "CREATE INDEX IF NOT EXISTS idx_trace_spans_kind_started ON trace_spans(span_kind, started_at DESC, turn_id)",
        ],
    },
];
