use openwork_persistence::{
    DATABASE_INFRA_MIGRATIONS, DROP_LEGACY_SESSION_MIGRATIONS, RECORDED_EVENT_MIGRATIONS,
};

#[test]
fn recorded_event_migration_matches_the_accepted_journal_contract() {
    assert_eq!(RECORDED_EVENT_MIGRATIONS.len(), 4);
    let create_sql = RECORDED_EVENT_MIGRATIONS[0].statements.join("\n");

    assert!(create_sql.contains("CREATE TABLE IF NOT EXISTS recorded_events"));
    assert!(create_sql.contains("global_position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY"));
    assert!(create_sql.contains("event_id TEXT NOT NULL UNIQUE"));
    assert!(create_sql.contains("aggregate_type TEXT NOT NULL"));
    assert!(create_sql.contains("aggregate_id TEXT NOT NULL"));
    assert!(create_sql.contains("aggregate_version BIGINT NOT NULL"));
    assert!(create_sql.contains("event_type TEXT NOT NULL"));
    assert!(create_sql.contains("event_version INTEGER NOT NULL DEFAULT 1"));
    assert!(create_sql.contains("payload_json JSONB NOT NULL"));
    assert!(create_sql.contains("occurred_at TIMESTAMP WITHOUT TIME ZONE NOT NULL"));
    assert!(create_sql.contains("recorded_at TIMESTAMP WITHOUT TIME ZONE NOT NULL"));
    assert!(create_sql.contains("CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'"));
    assert!(create_sql.contains("UNIQUE (aggregate_type, aggregate_id, aggregate_version)"));
    assert!(create_sql.contains("jsonb_typeof(payload_json) = 'object'"));

    let migration_sql = RECORDED_EVENT_MIGRATIONS[1..]
        .iter()
        .flat_map(|migration| migration.statements)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(migration_sql.contains("aggregate_type IN ('thread', 'session', 'turn')"));
    assert!(migration_sql.contains("SET aggregate_type = 'session'"));
    assert!(migration_sql.contains("'thread_created', 'session_created'"));
    assert!(migration_sql.contains("payload_json - 'threadId'"));
    assert!(migration_sql.contains("jsonb_build_object('sessionId'"));
    assert!(migration_sql.contains("aggregate_type IN ('session', 'turn')"));

    assert!(!create_sql.contains("stream_kind"));
    assert!(!create_sql.contains("updated_at"));
    assert!(!create_sql.contains("is_deleted"));
    assert!(!create_sql.contains("deleted_at"));
    assert!(!create_sql.contains("TIMESTAMPTZ"));
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
