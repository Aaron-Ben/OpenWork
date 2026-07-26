mod actor;
mod commands;
mod compaction;
mod ids;
mod run_loop;
mod storage;
mod trace;
mod updates;

pub use actor::{SessionHandle, SessionRuntimeConfig};
pub use commands::{
    PermissionDecision, PermissionRequest, ResolvedModel, SessionError, TurnAccepted, TurnOutcome,
};
pub(crate) use compaction::{
    COMPACTION_TRANSCRIPT_TOOL_NAME, ConversationTranscriptTool, compacted_items,
    compaction_summary_message, validate_summary_text, validate_system_reminder,
};
pub use compaction::{
    CompactionError, CompactionRuntimeState, CompactionStateCollectInput, CompactionStateCollector,
    CompactionStateContributor, CompactionStateEntry, CompactionStateError,
    CompactionStateFailurePolicy, CompactionStateWarning, ConversationCompaction,
    ConversationCompactionKind, ConversationProjectionSelector,
    DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT, DEFAULT_CONTEXT_WINDOW_TOKENS,
    NewConversationCompaction, ReminderSection,
};
pub use ids::{ClientRequestId, SessionId, ToolCallId, TurnId};
pub use storage::{NoopSessionStorage, SessionStorage};
pub use trace::{
    CompactionAttemptOutcome, CompactionFinished, CompactionStarted, CompactionSummaryAttemptTrace,
    CompactionTraceAttributesV1, CompactionTraceGuard, DEFAULT_TRACE_PAYLOAD_RETENTION_DAYS,
    DEFAULT_TRACE_PAYLOAD_SLOT_MAX_BYTES, ModelCallFinished, ModelCallStarted, ModelCallTraceGuard,
    ModelTraceAttributesV1, NoopTraceRecorder, ToolCallFinished, ToolCallStarted,
    ToolCallTraceGuard, ToolTraceAttributesV1, TraceContentConfig, TraceContentConfigError,
    TraceContentPolicy, TraceFlushResult, TracePayloadSlot, TracePayloads, TraceRecorder,
    TraceSignal, TraceStatus,
};
pub use updates::{
    LiveToolCall, SessionPhase, SessionRuntimeSnapshot, SessionSnapshot, SessionUpdate,
    SessionUpdateEnvelope, ToolProgressUpdate,
};
