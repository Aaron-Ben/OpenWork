//! Environment-backed collaboration daemon configuration.

use std::{env, path::PathBuf};

use crate::{
    daemon::DaemonError,
    gc::{
        DEFAULT_BATCH_SIZE, DEFAULT_EVENT_RETENTION_DAYS, DEFAULT_STATEMENT_TIMEOUT_MS,
        DEFAULT_TRIAGE_RETENTION_DAYS,
    },
    storage::CollabGcPolicy,
};

pub const COLLAB_HOME_ENV: &str = "OPENWORK_COLLAB_HOME";
pub const EVENT_RETENTION_DAYS_ENV: &str = "OPENWORK_COLLAB_EVENT_RETENTION_DAYS";
pub const TRIAGE_RETENTION_DAYS_ENV: &str = "OPENWORK_COLLAB_TRIAGE_RETENTION_DAYS";
pub const GC_BATCH_SIZE_ENV: &str = "OPENWORK_COLLAB_GC_BATCH_SIZE";
pub const GC_STATEMENT_TIMEOUT_MS_ENV: &str = "OPENWORK_COLLAB_GC_STATEMENT_TIMEOUT_MS";

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub root: PathBuf,
    pub database_url: Option<String>,
    pub gc_policy: CollabGcPolicy,
}

impl DaemonConfig {
    pub fn from_env() -> Result<Self, DaemonError> {
        let _ = dotenvy::dotenv();
        let root = match env::var_os(COLLAB_HOME_ENV) {
            Some(path) => PathBuf::from(path),
            None => env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or(DaemonError::MissingHome)?
                .join(".openwork/collab"),
        };
        Ok(Self {
            root,
            database_url: env::var("DATABASE_URL").ok(),
            gc_policy: CollabGcPolicy::new(
                env_u32(EVENT_RETENTION_DAYS_ENV, DEFAULT_EVENT_RETENTION_DAYS)?,
                env_u32(TRIAGE_RETENTION_DAYS_ENV, DEFAULT_TRIAGE_RETENTION_DAYS)?,
                env_u32(GC_BATCH_SIZE_ENV, DEFAULT_BATCH_SIZE)?,
                env_u32(GC_STATEMENT_TIMEOUT_MS_ENV, DEFAULT_STATEMENT_TIMEOUT_MS)?,
            )?,
        })
    }

    pub fn socket_path(&self) -> PathBuf {
        self.root.join("daemon.sock")
    }
}

fn env_u32(name: &'static str, default: u32) -> Result<u32, DaemonError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u32>()
            .map_err(|error| DaemonError::InvalidEnvironment {
                name,
                value,
                reason: error.to_string(),
            }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(DaemonError::InvalidEnvironment {
            name,
            value: "<non-Unicode>".to_string(),
            reason: error.to_string(),
        }),
    }
}
