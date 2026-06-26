use openwork_database::{DbDateTime, Migration};
use openwork_db_macros::PgEntity;

#[derive(Debug, Clone, PgEntity)]
#[table_name = "providers"]
#[allow(dead_code)]
pub struct ProviderRecord {
    #[primary_key]
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: String,
    pub models_json: serde_json::Value,
    pub enabled: bool,
    pub extra_body_json: Option<serde_json::Value>,
    #[indexed]
    pub active: bool,
    pub created_at: DbDateTime,
    pub updated_at: DbDateTime,
}

pub const PROVIDER_MIGRATIONS: &[Migration] = &[Migration {
    version: 202606250001,
    name: "create_providers",
    statements: &[
        r#"CREATE TABLE IF NOT EXISTS providers (
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL,
           base_url TEXT NOT NULL,
           api_key TEXT NOT NULL,
           kind TEXT NOT NULL,
           models_json JSONB NOT NULL,
           enabled BOOLEAN NOT NULL,
           extra_body_json JSONB,
           active BOOLEAN NOT NULL DEFAULT false,
           created_at TIMESTAMPTZ NOT NULL,
           updated_at TIMESTAMPTZ NOT NULL
         )"#,
        "CREATE INDEX IF NOT EXISTS idx_providers_active ON providers(active)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_providers_single_active ON providers(active) WHERE active",
    ],
}];
