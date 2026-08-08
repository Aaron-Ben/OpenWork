//! Durable Turn control loop and state machines.

pub mod agent;
mod context;
mod core;
mod model_call;
pub mod plan;
mod provider;
pub mod session;
pub mod skills;
pub mod storage;
mod user_input;

pub use agent::{
    AgentControl, AgentControlError, DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS, SubAgent, SubAgentHost,
    SubAgentSpec, TurnSlot,
};
pub use context::{
    CONTEXT_WINDOW_INSPECTION_SCHEMA_VERSION, ContextInspectionBudget, ContextInspectionMessage,
    ContextInspectionSystemPart, ContextWindowInspection,
};
pub use core::{
    CredentialResolver, EnvironmentCredentialResolver, LoadedSession, OpenWorkCore,
    OpenWorkCoreConfig, OpenWorkCoreError,
};
pub use openwork_chat_state::MessageKind;
pub use openwork_models::provider::ApiCredential as ModelCredential;
pub use openwork_models::provider::{ProviderInput, ProviderProfile};
pub use openwork_tools::{PermissionMode, ReapplyFileChangesResult, UndoFileChangesResult};
pub use provider::{ProviderIndex, ProviderPreset, ProviderPresetModel, ProviderTestResult};
pub use session::{
    ClientRequestId, CompactionAttemptOutcome, CompactionError, CompactionFinished,
    CompactionRuntimeState, CompactionStarted, CompactionStateCollectInput,
    CompactionStateCollector, CompactionStateContributor, CompactionStateEntry,
    CompactionStateError, CompactionStateFailurePolicy, CompactionStateWarning,
    CompactionSummaryAttemptTrace, CompactionTraceAttributesV1, CompactionTraceGuard,
    ConversationCompaction, ConversationCompactionKind, ConversationProjectionSelector,
    DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT, DEFAULT_CONTEXT_WINDOW_TOKENS,
    DEFAULT_TRACE_PAYLOAD_RETENTION_DAYS, DEFAULT_TRACE_PAYLOAD_SLOT_MAX_BYTES, LiveToolCall,
    ModelCallFinished, ModelCallStarted, ModelCallTraceGuard, ModelTraceAttributesV1,
    NewConversationCompaction, NoopSessionStorage, NoopTraceRecorder, PermissionDecision,
    PermissionRequest, ReminderSection, ResolvedModel, SessionError, SessionHandle, SessionId,
    SessionPhase, SessionRuntimeConfig, SessionRuntimeSnapshot, SessionSnapshot, SessionStorage,
    SessionUpdate, SessionUpdateEnvelope, ToolCallFinished, ToolCallId, ToolCallStarted,
    ToolCallTraceGuard, ToolProgressUpdate, ToolTraceAttributesV1, TraceContentConfig,
    TraceContentConfigError, TraceContentPolicy, TraceFlushResult, TracePayloadSlot, TracePayloads,
    TraceRecorder, TraceSignal, TraceStatus, TurnAccepted, TurnId as RuntimeTurnId, TurnOutcome,
};
pub use storage::{
    API_KEY_ENCRYPTION_KEY_ENV, ApiKeyCipher, ApiKeyCipherError, ConversationProjectionRecord,
    ConversationTranscriptPage, ConversationTranscriptQuery,
    DEFAULT_COMPACTION_TRANSCRIPT_PAGE_LIMIT, MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT, ModelInput,
    ModelRecord, PostgresProviderRepository, PostgresStorage, PostgresTraceRecorder, SessionInput,
    SessionRecord, StorageError, StoredMessageRecord, SubAgentSessionInput, TraceCompleteness,
    TraceCompletenessState, TraceSpanPayloadRecord, TraceSpanRecord, TraceTurnSummary, TurnTrace,
};
pub use user_input::UserInput;
