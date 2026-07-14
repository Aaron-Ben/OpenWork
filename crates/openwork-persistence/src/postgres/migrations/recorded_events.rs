use super::Migration;

/// First durable-fact table. Query projections are deliberately not part of
/// this migration and may be rebuilt from these events later.
pub const RECORDED_EVENT_MIGRATIONS: &[Migration] = &[
    Migration {
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
    },
    // Expand first so the data migration is safe for databases that already
    // contain durable events using the former Thread name. Existing
    // migrations remain immutable.
    Migration {
        version: 202607140301,
        name: "allow_session_aggregate_name",
        statements: &[
            "ALTER TABLE recorded_events DROP CONSTRAINT IF EXISTS recorded_events_aggregate_type_valid",
            r#"ALTER TABLE recorded_events
                 ADD CONSTRAINT recorded_events_aggregate_type_valid
                 CHECK (aggregate_type IN ('thread', 'session', 'turn'))"#,
        ],
    },
    // Rename the stored domain vocabulary without changing event ordering,
    // aggregate versions, event ids, or global positions.
    Migration {
        version: 202607140302,
        name: "rename_thread_events_to_session",
        statements: &[
            "UPDATE recorded_events SET aggregate_type = 'session' WHERE aggregate_type = 'thread'",
            r#"UPDATE recorded_events AS events
                 SET event_type = renames.new_name
                 FROM (VALUES
                   ('thread_created', 'session_created'),
                   ('thread_title_changed', 'session_title_changed'),
                   ('thread_deleted', 'session_deleted')
                 ) AS renames(old_name, new_name)
                 WHERE events.event_type = renames.old_name"#,
            r#"UPDATE recorded_events
                 SET payload_json =
                   (payload_json - 'threadId') ||
                   jsonb_build_object('sessionId', payload_json -> 'threadId')
                 WHERE payload_json ? 'threadId'"#,
        ],
    },
    // Contract only after every durable record uses the new name.
    Migration {
        version: 202607140303,
        name: "require_session_aggregate_name",
        statements: &[
            "ALTER TABLE recorded_events DROP CONSTRAINT IF EXISTS recorded_events_aggregate_type_valid",
            r#"ALTER TABLE recorded_events
                 ADD CONSTRAINT recorded_events_aggregate_type_valid
                 CHECK (aggregate_type IN ('session', 'turn'))"#,
        ],
    },
];

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
