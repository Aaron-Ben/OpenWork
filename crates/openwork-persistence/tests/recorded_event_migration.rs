use openwork_persistence::{
    DATABASE_INFRA_MIGRATIONS, DROP_LEGACY_SESSION_MIGRATIONS, RECORDED_EVENT_MIGRATIONS,
};

#[test]
fn recorded_event_migration_matches_the_accepted_journal_contract() {
    assert_eq!(RECORDED_EVENT_MIGRATIONS.len(), 1);
    let sql = RECORDED_EVENT_MIGRATIONS[0].statements.join("\n");

    assert!(sql.contains("CREATE TABLE IF NOT EXISTS recorded_events"));
    assert!(sql.contains("global_position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY"));
    assert!(sql.contains("event_id TEXT NOT NULL UNIQUE"));
    assert!(sql.contains("aggregate_type TEXT NOT NULL"));
    assert!(sql.contains("aggregate_type IN ('thread', 'turn')"));
    assert!(sql.contains("aggregate_id TEXT NOT NULL"));
    assert!(sql.contains("aggregate_version BIGINT NOT NULL"));
    assert!(sql.contains("event_type TEXT NOT NULL"));
    assert!(sql.contains("event_version INTEGER NOT NULL DEFAULT 1"));
    assert!(sql.contains("payload_json JSONB NOT NULL"));
    assert!(sql.contains("occurred_at TIMESTAMP WITHOUT TIME ZONE NOT NULL"));
    assert!(sql.contains("recorded_at TIMESTAMP WITHOUT TIME ZONE NOT NULL"));
    assert!(sql.contains("CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'"));
    assert!(sql.contains("UNIQUE (aggregate_type, aggregate_id, aggregate_version)"));
    assert!(sql.contains("jsonb_typeof(payload_json) = 'object'"));

    assert!(!sql.contains("stream_kind"));
    assert!(!sql.contains("updated_at"));
    assert!(!sql.contains("is_deleted"));
    assert!(!sql.contains("deleted_at"));
    assert!(!sql.contains("TIMESTAMPTZ"));
}

#[test]
fn legacy_session_tables_are_dropped_after_journal_read_write_cutover() {
    assert_eq!(DROP_LEGACY_SESSION_MIGRATIONS.len(), 1);
    let sql = DROP_LEGACY_SESSION_MIGRATIONS[0].statements.join("\n");
    for table in ["sessions", "messages", "llm_events", "tool_runs"] {
        assert!(
            sql.contains(&format!("DROP TABLE IF EXISTS {table}")),
            "missing legacy table drop: {table}"
        );
        assert!(!sql.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")));
    }
}

#[test]
fn migration_metadata_time_is_normalized_without_editing_applied_migrations() {
    assert_eq!(DATABASE_INFRA_MIGRATIONS.len(), 1);
    let sql = DATABASE_INFRA_MIGRATIONS[0].statements.join("\n");
    assert!(sql.contains("timestamp with time zone"));
    assert!(sql.contains("TYPE TIMESTAMP WITHOUT TIME ZONE"));
    assert!(sql.contains("AT TIME ZONE 'Asia/Shanghai'"));
}
