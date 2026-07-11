use super::Migration;

/// Forward-only normalization for development databases created before the
/// project adopted Asia/Shanghai wall time in TIMESTAMP WITHOUT TIME ZONE.
pub const DATABASE_INFRA_MIGRATIONS: &[Migration] = &[Migration {
    version: 202607110203,
    name: "normalize_schema_migration_time",
    statements: &[r#"DO $$
        BEGIN
          IF EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name = 'schema_migrations'
              AND column_name = 'applied_at'
              AND data_type = 'timestamp with time zone'
          ) THEN
            ALTER TABLE schema_migrations
              ALTER COLUMN applied_at TYPE TIMESTAMP WITHOUT TIME ZONE
              USING applied_at AT TIME ZONE 'Asia/Shanghai';
          END IF;
          ALTER TABLE schema_migrations
            ALTER COLUMN applied_at SET DEFAULT
              (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai');
        END
        $$"#],
}];
