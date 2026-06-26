use openwork_database::PgSchema;
use openwork_db_macros::PgEntity;

#[derive(Debug, Clone, PgEntity)]
#[table_name = "sessions"]
#[allow(dead_code)]
struct SessionRecord {
    #[primary_key]
    id: String,
    #[indexed]
    provider_id: String,
    title: String,
    working_dir: Option<String>,
}

#[test]
fn derive_pg_entity_generates_schema_metadata() {
    assert_eq!(SessionRecord::TABLE, "sessions");
    assert_eq!(SessionRecord::COLUMNS.len(), 4);
    assert_eq!(SessionRecord::primary_key().unwrap().name, "id");
    assert_eq!(SessionRecord::INDEXES.len(), 1);
    assert_eq!(SessionRecord::INDEXES[0].name, "sessions_provider_id_idx");
    assert!(SessionRecord::COLUMNS[3].nullable);
}
