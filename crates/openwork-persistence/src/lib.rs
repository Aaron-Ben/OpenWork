//! OpenWork PostgreSQL persistence adapters.
//!
//! Domain ports live in `openwork-protocol`; this crate owns SQL schema, migrations and queries.

mod crypto;
pub mod postgres;

pub use crypto::{API_KEY_ENCRYPTION_KEY_ENV, ApiKeyCipher, ApiKeyCipherError};
pub use postgres::{PROVIDER_MIGRATIONS, PostgresPersistence, PostgresProviderRepository};
