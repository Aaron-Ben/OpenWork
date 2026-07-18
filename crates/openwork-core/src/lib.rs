//! Durable Turn control loop and state machines.

mod core;
mod provider;
pub mod session;
pub mod storage;

pub use core::{
    CredentialResolver, EnvironmentCredentialResolver, LoadedSession, OpenWorkCore,
    OpenWorkCoreConfig, OpenWorkCoreError,
};
pub use openwork_models::provider::ApiCredential as ModelCredential;
pub use openwork_models::provider::{ProviderInput, ProviderProfile};
pub use provider::{ProviderIndex, ProviderPreset, ProviderPresetModel, ProviderTestResult};
pub use session::{
    ClientRequestId, LiveToolCall, ModelCallFinished, ModelCallStarted, NoopSessionStorage,
    NoopTraceRecorder, PermissionDecision, PermissionRequest, ResolvedModel, SessionError,
    SessionHandle, SessionId, SessionPhase, SessionRuntimeConfig, SessionRuntimeSnapshot,
    SessionSnapshot, SessionStorage, SessionUpdate, SessionUpdateEnvelope, ToolCallFinished,
    ToolCallId, ToolCallStarted, TraceFlushResult, TraceRecorder, TraceSignal, TraceStatus,
    TurnAccepted, TurnId as RuntimeTurnId, TurnOutcome,
};
pub use storage::{
    API_KEY_ENCRYPTION_KEY_ENV, ApiKeyCipher, ApiKeyCipherError, ModelInput, ModelRecord,
    PostgresProviderRepository, PostgresStorage, PostgresTraceRecorder, SessionInput,
    SessionRecord, StorageError, StoredMessageRecord, TraceSpanRecord, TraceTurnSummary,
};
