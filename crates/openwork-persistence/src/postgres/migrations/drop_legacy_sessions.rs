use super::Migration;

/// Development cutover: Session and Message reads/writes now replay the
/// append-only Journal, so these unused compatibility tables can be removed.
/// The user explicitly accepted losing their existing development data.
pub const DROP_LEGACY_SESSION_MIGRATIONS: &[Migration] = &[Migration {
    version: 202607110202,
    name: "drop_legacy_session_tables",
    statements: &[
        "DROP TABLE IF EXISTS tool_runs",
        "DROP TABLE IF EXISTS llm_events",
        "DROP TABLE IF EXISTS messages",
        "DROP TABLE IF EXISTS sessions",
    ],
}];
