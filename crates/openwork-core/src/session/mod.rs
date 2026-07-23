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
pub(crate) use compaction::compaction_summary_message;
pub use compaction::{CompactionError, ConversationCompaction};
pub use ids::{ClientRequestId, SessionId, ToolCallId, TurnId};
pub use storage::{NoopSessionStorage, SessionStorage};
pub use trace::{
    ModelCallFinished, ModelCallStarted, ModelCallTraceGuard, ModelTraceAttributesV1,
    ModelTransportAttemptTrace, NoopTraceRecorder, ToolCallFinished, ToolCallStarted,
    ToolCallTraceGuard, ToolTraceAttributesV1, TraceFlushResult, TraceRecorder, TraceSignal,
    TraceStatus,
};
pub use updates::{
    LiveToolCall, SessionPhase, SessionRuntimeSnapshot, SessionSnapshot, SessionUpdate,
    SessionUpdateEnvelope, ToolProgressUpdate,
};
