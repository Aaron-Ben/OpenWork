//! OpenWork PostgreSQL persistence adapters.
//!
//! Domain ports live in `openwork-protocol`; this crate owns SQL schema, migrations and queries.

mod crypto;
pub mod postgres;
pub mod session;

pub use crypto::{API_KEY_ENCRYPTION_KEY_ENV, ApiKeyCipher, ApiKeyCipherError};
pub use postgres::{
    DATABASE_INFRA_MIGRATIONS, DROP_LEGACY_SESSION_MIGRATIONS, DatabaseConfig, PROVIDER_MIGRATIONS,
    PostgresEventJournal, PostgresPersistence, PostgresPersistenceError,
    PostgresProviderRepository, RECORDED_EVENT_MIGRATIONS,
};
pub use session::{
    NewMessage, Session, SessionError, SessionInput, SessionLoadResult, SessionMessage,
    SessionStore, SessionSummary, TurnOutcome,
};
