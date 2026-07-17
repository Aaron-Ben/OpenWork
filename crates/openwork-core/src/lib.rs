//! Durable Turn control loop and state machines.

mod agent;
mod approval;
mod core;
pub mod session;
pub mod storage;

pub use agent::{
    Agent, AgentConfig, AgentError, AgentEvent, AgentPorts, AgentTraceContext, ApprovalRecovery,
    RunResult,
};
pub use approval::{
    ApprovalCommandError, ApprovalState, ApprovalWaitOutcome, TurnCommandHandle, TurnCommandInbox,
    turn_command_channel,
};
pub use core::{
    CredentialResolver, EnvironmentCredentialResolver, LoadedSession, OpenWorkCore,
    OpenWorkCoreConfig, OpenWorkCoreError,
};
pub use openwork_models::provider::ApiCredential as ModelCredential;
pub use session::{
    ClientRequestId, LiveToolCall, ModelCallFinished, ModelCallStarted, NoopSessionStorage,
    NoopTraceRecorder, PermissionDecision, PermissionRequest, ResolvedModel, SessionError,
    SessionHandle, SessionId, SessionPhase, SessionRuntimeConfig, SessionRuntimeSnapshot,
    SessionSnapshot, SessionStorage, SessionUpdate, SessionUpdateEnvelope, ToolCallFinished,
    ToolCallId, ToolCallStarted, TraceFlushResult, TraceRecorder, TraceSignal, TraceStatus,
    TurnAccepted, TurnId as RuntimeTurnId, TurnOutcome,
};
pub use storage::{
    ModelInput, ModelRecord, PostgresStorage, PostgresTraceRecorder, SessionInput, SessionRecord,
    StorageError, StoredMessageRecord, TraceSpanRecord, TraceTurnSummary,
};
