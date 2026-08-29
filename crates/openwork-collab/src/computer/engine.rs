use std::{collections::BTreeMap, path::PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

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
    pub home: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
    pub resume_session_id: Option<String>,
    pub environment: BTreeMap<String, String>,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TurnResult {
    pub text: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub usage: EngineUsage,
}

#[derive(Clone, Debug)]
pub struct ClassifyRequest {
    pub cwd: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifyResult {
    pub text: String,
    pub model: Option<String>,
    pub usage: EngineUsage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineProbeStatus {
    Ready,
    Missing,
    Unauthenticated,
    Broken,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProbe {
    pub status: EngineProbeStatus,
    pub version: Option<String>,
    pub detail: Option<String>,
}

#[async_trait]
pub trait EngineAdapter: Send + Sync {
    async fn probe(&self) -> Result<EngineProbe, EngineError>;
    async fn classify(&self, request: ClassifyRequest) -> Result<ClassifyResult, EngineError>;
    async fn run_turn(&self, request: TurnRequest) -> Result<TurnResult, EngineError>;
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("OpenCode is not installed: {0}")]
    Missing(String),
    #[error("OpenCode process failed: {0}")]
    Process(String),
    #[error("OpenCode protocol failed: {0}")]
    Protocol(String),
    #[error("OpenCode reported an error: {0}")]
    Reported(String),
    #[error("OpenCode I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OpenCode turn was cancelled")]
    Cancelled,
}
