use openwork_database::Migration;

pub const PROVIDER_MIGRATIONS: &[Migration] = &[Migration {
    version: 202606250001,
    name: "create_provider_registry",
    statements: &[
        r#"CREATE TABLE IF NOT EXISTS providers (
               id TEXT PRIMARY KEY,
               name TEXT NOT NULL,
               base_url TEXT NOT NULL,
               api_key TEXT NOT NULL,
               driver_code TEXT NOT NULL,
               enabled BOOLEAN NOT NULL,
               adapter_options_json JSONB,
               active BOOLEAN NOT NULL DEFAULT false,
               created_at TIMESTAMPTZ NOT NULL,
               updated_at TIMESTAMPTZ NOT NULL,
               CONSTRAINT providers_name_not_blank CHECK (btrim(name) <> ''),
               CONSTRAINT providers_base_url_not_blank CHECK (btrim(base_url) <> ''),
               CONSTRAINT providers_driver_code_valid CHECK (driver_code IN (
                 'openai_responses',
                 'anthropic_messages',
                 'openai_chat_standard',
                 'openai_chat_deepseek',
                 'openai_chat_kimi',
                 'openai_chat_qwen',
                 'openai_chat_glm'
               ))
             )"#,
        "CREATE INDEX IF NOT EXISTS idx_providers_active ON providers(active)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_providers_single_active ON providers(active) WHERE active",
        r#"CREATE TABLE IF NOT EXISTS provider_models (
               provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
               model_id TEXT NOT NULL,
               display_name TEXT,
               model_tier TEXT NOT NULL,
               position INTEGER NOT NULL CHECK (position >= 0),
               enabled BOOLEAN NOT NULL DEFAULT true,
               created_at TIMESTAMPTZ NOT NULL,
               updated_at TIMESTAMPTZ NOT NULL,
               CONSTRAINT pk_provider_models PRIMARY KEY(provider_id, model_id),
               CONSTRAINT provider_models_id_not_blank CHECK (btrim(model_id) <> ''),
               CONSTRAINT provider_models_tier_valid CHECK (model_tier IN ('lite', 'plus', 'pro')),
               CONSTRAINT uq_provider_models_position UNIQUE(provider_id, position)
             )"#,
        "CREATE INDEX IF NOT EXISTS idx_provider_models_provider_position ON provider_models(provider_id, position)",
    ],
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_baseline_creates_normalized_provider_registry() {
        assert_eq!(PROVIDER_MIGRATIONS.len(), 1);
        let sql = PROVIDER_MIGRATIONS[0].statements.join("\n");
        assert!(sql.contains("driver_code TEXT NOT NULL"));
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS provider_models"));
        assert!(sql.contains("model_tier TEXT NOT NULL"));
        assert!(sql.contains("model_tier IN ('lite', 'plus', 'pro')"));
        assert!(!sql.contains("models_json"));
    }
}
