use super::Migration;

/// First durable-fact table. Query projections are deliberately not part of
/// this migration and may be rebuilt from these events later.
pub const RECORDED_EVENT_MIGRATIONS: &[Migration] = &[Migration {
    version: 202607110201,
    name: "create_recorded_events",
    statements: &[
        r#"CREATE TABLE IF NOT EXISTS recorded_events (
               global_position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
               event_id TEXT NOT NULL UNIQUE,
               aggregate_type TEXT NOT NULL,
               aggregate_id TEXT NOT NULL,
               aggregate_version BIGINT NOT NULL,
               event_type TEXT NOT NULL,
               event_version INTEGER NOT NULL DEFAULT 1,
               payload_json JSONB NOT NULL,
               occurred_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
               recorded_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
                   DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
               CONSTRAINT recorded_events_event_id_not_blank
                   CHECK (btrim(event_id) <> ''),
               CONSTRAINT recorded_events_aggregate_type_not_blank
                   CHECK (btrim(aggregate_type) <> ''),
               CONSTRAINT recorded_events_aggregate_id_not_blank
                   CHECK (btrim(aggregate_id) <> ''),
               CONSTRAINT recorded_events_aggregate_type_valid
                   CHECK (aggregate_type IN ('thread', 'turn')),
               CONSTRAINT recorded_events_aggregate_version_positive
                   CHECK (aggregate_version > 0),
               CONSTRAINT recorded_events_event_type_not_blank
                   CHECK (btrim(event_type) <> ''),
               CONSTRAINT recorded_events_event_version_positive
                   CHECK (event_version > 0),
               CONSTRAINT recorded_events_payload_is_object
                   CHECK (jsonb_typeof(payload_json) = 'object'),
               CONSTRAINT recorded_events_aggregate_version_unique
                   UNIQUE (aggregate_type, aggregate_id, aggregate_version)
             )"#,
        "CREATE INDEX IF NOT EXISTS idx_recorded_events_aggregate ON recorded_events(aggregate_type, aggregate_id, aggregate_version)",
        "CREATE INDEX IF NOT EXISTS idx_recorded_events_type_position ON recorded_events(event_type, global_position)",
    ],
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_is_append_only_and_uses_beijing_wall_time() {
        let sql = RECORDED_EVENT_MIGRATIONS[0].statements.join("\n");
        assert!(sql.contains("recorded_events_aggregate_version_unique"));
        assert!(sql.contains("aggregate_type IN ('thread', 'turn')"));
        assert!(sql.contains("CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'"));
        for mutable_column in ["updated_at", "is_deleted", "deleted_at"] {
            assert!(!sql.contains(mutable_column));
        }
    }
}
