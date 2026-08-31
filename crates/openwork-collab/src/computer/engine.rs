use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub use crate::protocol::{EngineId, EngineIdError};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub prompt: String,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TurnResult {
    pub text: String,
    pub model: Option<String>,
    pub usage: EngineUsage,
}

#[derive(Clone, Debug)]
pub struct ClassifyRequest {
    pub cwd: PathBuf,
    pub config_root: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
    pub environment: BTreeMap<String, String>,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifyResult {
    pub text: String,
    pub model: Option<String>,
    pub usage: EngineUsage,
}

#[derive(Clone, Debug)]
pub struct EngineRuntimeConfig {
    pub home: PathBuf,
    pub config_root: PathBuf,
    pub state_file: PathBuf,
    pub context_fingerprint: String,
    pub model: String,
    pub environment: BTreeMap<String, String>,
    /// Optional product-level wall-clock limit for one main Engine turn.
    /// `None` lets the turn run until it finishes or is explicitly cancelled.
    pub turn_timeout: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineAvailability {
    Available,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineInventory {
    pub availability: EngineAvailability,
}

#[async_trait]
pub trait EngineAdapter: Send + Sync {
    fn id(&self) -> EngineId;
    async fn probe(&self) -> Result<EngineInventory, EngineError>;
    async fn classify(&self, request: ClassifyRequest) -> Result<ClassifyResult, EngineError>;
    async fn create_agent_runtime(
        &self,
        config: EngineRuntimeConfig,
    ) -> Result<Box<dyn AgentEngineRuntime>, EngineError>;
}

#[async_trait]
pub trait AgentEngineRuntime: Send {
    async fn run_turn(&mut self, request: TurnRequest) -> Result<TurnResult, EngineError>;
    async fn shutdown(&mut self) -> Result<(), EngineError>;
}

#[derive(Clone, Default)]
pub struct EngineRegistry {
    adapters: HashMap<EngineId, Arc<dyn EngineAdapter>>,
}

impl EngineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn single<A>(adapter: A) -> Self
    where
        A: EngineAdapter + 'static,
    {
        let id = adapter.id();
        Self {
            adapters: HashMap::from([(id, Arc::new(adapter) as Arc<dyn EngineAdapter>)]),
        }
    }

    pub fn register<A>(&mut self, adapter: A) -> Result<(), EngineRegistryError>
    where
        A: EngineAdapter + 'static,
    {
        let id = adapter.id();
        if self.adapters.contains_key(&id) {
            return Err(EngineRegistryError::Duplicate(id));
        }
        self.adapters.insert(id, Arc::new(adapter));
        Ok(())
    }

    pub fn require(&self, id: &EngineId) -> Result<Arc<dyn EngineAdapter>, EngineError> {
        self.adapters
            .get(id)
            .cloned()
            .ok_or_else(|| EngineError::NotRegistered {
                engine_id: id.clone(),
            })
    }

    pub fn adapters(&self) -> Vec<Arc<dyn EngineAdapter>> {
        let mut adapters = self.adapters.values().cloned().collect::<Vec<_>>();
        adapters.sort_by_key(|adapter| adapter.id());
        adapters
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineRegistryError {
    #[error("Engine {0} is already registered")]
    Duplicate(EngineId),
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Engine {engine_id} is not registered")]
    NotRegistered { engine_id: EngineId },
    #[error("Engine executable is missing: {detail}")]
    Missing { detail: String },
    #[error("Engine authentication failed: {detail}")]
    Unauthenticated { detail: String },
    #[error("Engine is rate limited: {detail}")]
    RateLimited {
        retry_after: Option<Duration>,
        detail: String,
    },
    #[error("Engine process failed: {detail}")]
    Process { detail: String },
    #[error("Engine protocol failed: {detail}")]
    Protocol { detail: String },
    #[error("Engine reported an error: {detail}")]
    Reported { detail: String },
    #[error("Engine session is invalid: {detail}")]
    SessionInvalid { detail: String },
    #[error("Engine I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Engine turn was cancelled")]
    Cancelled,
    #[error("Engine {operation} timed out")]
    Timeout { operation: &'static str },
    #[error("Engine {stream} exceeded its {limit}-byte output limit")]
    OutputLimit { stream: &'static str, limit: usize },
}

impl EngineError {
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}
