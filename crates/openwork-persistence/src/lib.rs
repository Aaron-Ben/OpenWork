//! OpenWork PostgreSQL persistence adapters.
//!
//! Domain ports live in `openwork-protocol`; this crate owns SQL schema, migrations and queries.

pub mod postgres;

pub use postgres::{PROVIDER_MIGRATIONS, PostgresPersistence, PostgresProviderRepository};
