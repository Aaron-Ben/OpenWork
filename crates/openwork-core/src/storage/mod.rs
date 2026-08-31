mod postgres;
mod provider;
pub(crate) mod time;
mod trace;

// Serializes the two operations that can change Trace payload reachability:
// attaching a payload mapping and deleting orphan payload bodies. The value is
// the ASCII bytes for "OPENWORK" and only names this application-local lock.
const TRACE_PAYLOAD_ADVISORY_LOCK: i64 = 0x4f50_454e_574f_524b;

pub use crate::session::ConversationProjectionSelector;
pub use openwork_credentials::{API_KEY_ENCRYPTION_KEY_ENV, ApiKeyCipher, ApiKeyCipherError};
pub(crate) use postgres::is_valid_task_name;
pub use postgres::{
    ConversationProjectionRecord, ConversationTranscriptPage, ConversationTranscriptQuery,
    DEFAULT_COMPACTION_TRANSCRIPT_PAGE_LIMIT, DeletedOrphanSubAgent,
    MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT, ModelInput, ModelRecord, PostgresStorage, SessionInput,
    SessionRecord, StorageError, StoredMessageRecord, SubAgentReconciliation, SubAgentSessionInput,
    TraceCompleteness, TraceCompletenessState, TraceSpanPayloadRecord, TraceSpanRecord,
    TraceTurnSummary, TurnTrace, UndeliveredSubAgentResult,
};
pub use provider::PostgresProviderRepository;
pub use trace::PostgresTraceRecorder;
