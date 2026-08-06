mod policy;
mod projection;
mod recovery;
mod reminder;
mod state;
mod summary;
mod transcript;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use openwork_agent::Agent;
use openwork_chat_state::{ChatStateHandle, ConversationView};
use openwork_models::model::{ModelError, ModelPort};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::context::{ResolvedSystemContext, SystemContextBuilder};
use crate::model_call::estimate_conversation_tokens;
use crate::skills::SkillRoots;

use self::projection::{last_real_user, last_user_source};
use self::summary::{SummaryTraceContext, generate_summary};
use super::{
    CompactionStarted, CompactionTraceAttributesV1, CompactionTraceGuard, SessionId,
    SessionStorage, TraceRecorder, TurnId,
};

pub(crate) use policy::AutomaticCompactionPolicy;
pub use policy::{DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT, DEFAULT_CONTEXT_WINDOW_TOKENS};
pub(crate) use projection::{compacted_items, compaction_summary_message};
pub use recovery::ConversationProjectionSelector;
pub(super) use recovery::{CompactionTrigger, ConversationRewindRequest, rewind_conversation};
pub use reminder::ReminderSection;
pub(crate) use reminder::validate_system_reminder;
pub use state::{
    CompactionRuntimeState, CompactionStateCollectInput, CompactionStateCollector,
    CompactionStateContributor, CompactionStateEntry, CompactionStateError,
    CompactionStateFailurePolicy, CompactionStateWarning,
};
pub(crate) use summary::validate_summary_text;
pub(crate) use transcript::{COMPACTION_TRANSCRIPT_TOOL_NAME, ConversationTranscriptTool};

/// 无 Turn 的操作（手动压缩、rewind）自开一个 Trace 根。
pub(crate) fn new_trace_id() -> String {
    format!("trace-{}", Uuid::new_v4().simple())
}

const CHECKPOINT_FORMAT_VERSION: u16 = 1;
const SUMMARY_FORMAT_VERSION: u16 = 1;
const RUNTIME_REMINDER_FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCompaction {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub through_message_sequence: i64,
    pub replaced_through_message_sequence: i64,
    pub source_message_count: u32,
    pub checkpoint_format_version: u16,
    pub kind: ConversationCompactionKind,
    pub summary_format_version: u16,
    pub last_user_message_id: Option<String>,
    pub last_user_message_sequence: Option<i64>,
    pub resolved_model_name: String,
    pub summary: String,
    pub runtime_state: CompactionRuntimeState,
    pub runtime_reminder_format_version: u16,
    pub runtime_reminder: String,
    pub trigger_turn_id: Option<String>,
    pub parent_compaction_id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationCompactionKind {
    Manual,
    Threshold,
    Overflow,
    Rewind,
}

impl ConversationCompactionKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Threshold => "threshold",
            Self::Overflow => "overflow",
            Self::Rewind => "rewind",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewConversationCompaction {
    pub kind: ConversationCompactionKind,
    pub source_message_count: u32,
    pub resolved_model_name: String,
    pub summary: String,
    pub runtime_state: CompactionRuntimeState,
    pub runtime_reminder: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub trigger_turn_id: Option<TurnId>,
    pub last_user_message_id: Option<String>,
    pub last_user_message_sequence: Option<i64>,
}

impl ConversationCompaction {
    pub(crate) fn in_memory(session_id: &SessionId, input: &NewConversationCompaction) -> Self {
        Self {
            id: format!("compaction-{}", Uuid::new_v4().simple()),
            session_id: session_id.to_string(),
            sequence: 1,
            through_message_sequence: 0,
            replaced_through_message_sequence: 0,
            source_message_count: input.source_message_count,
            checkpoint_format_version: CHECKPOINT_FORMAT_VERSION,
            kind: input.kind,
            summary_format_version: SUMMARY_FORMAT_VERSION,
            last_user_message_id: input
                .last_user_message_id
                .clone()
                .or_else(|| Some("in-memory-user".to_string())),
            last_user_message_sequence: input.last_user_message_sequence.or(Some(0)),
            resolved_model_name: input.resolved_model_name.clone(),
            summary: input.summary.clone(),
            runtime_state: input.runtime_state.clone(),
            runtime_reminder_format_version: RUNTIME_REMINDER_FORMAT_VERSION,
            runtime_reminder: input.runtime_reminder.clone(),
            trigger_turn_id: input.trigger_turn_id.as_ref().map(ToString::to_string),
            parent_compaction_id: None,
            input_tokens: input.input_tokens,
            output_tokens: input.output_tokens,
            created_at: String::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum CompactionError {
    #[error("session has an active turn and cannot be compacted: {0}")]
    SessionActive(TurnId),
    #[error("conversation is empty and cannot be compacted")]
    EmptyConversation,
    #[error("conversation has no real user request to preserve")]
    MissingLastUser,
    #[error("conversation has too many messages to compact")]
    MessageCountOverflow,
    #[error("failed to resolve compaction System Context: {0}")]
    Context(String),
    #[error("failed to build compaction model request: {0}")]
    Request(String),
    #[error("compaction model request failed: {0}")]
    Model(#[from] ModelError),
    #[error("compaction model stream failed: {0}")]
    Stream(ModelError),
    #[error("compaction model stream ended without a completed response")]
    MissingResponse,
    #[error("compaction model stream completed more than once")]
    DuplicateResponse,
    #[error("compaction response was not usable: {0}")]
    InvalidResponse(String),
    #[error("compaction summary attempt timed out after {seconds} seconds")]
    SummaryAttemptTimeout { seconds: u64 },
    #[error("compaction summary failed after {attempts} attempts: {last_error}")]
    SummaryRetriesExhausted { attempts: usize, last_error: String },
    #[error("failed to persist conversation compaction: {0}")]
    Persistence(String),
    #[error("failed to install compacted Conversation: {0}")]
    ChatState(String),
    #[error("failed to collect compaction runtime state: {0}")]
    State(String),
    #[error("session runtime stopped while compacting the Conversation")]
    ActorStopped,
}

impl CompactionError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::SessionActive(_) => "session_active",
            Self::EmptyConversation => "empty_conversation",
            Self::MissingLastUser => "missing_last_user",
            Self::MessageCountOverflow => "message_count_overflow",
            Self::Context(_) => "context_error",
            Self::Request(_) => "request_error",
            Self::Model(_) => "model_error",
            Self::Stream(_) => "stream_error",
            Self::MissingResponse => "missing_response",
            Self::DuplicateResponse => "duplicate_response",
            Self::InvalidResponse(_) => "invalid_response",
            Self::SummaryAttemptTimeout { .. } => "summary_attempt_timeout",
            Self::SummaryRetriesExhausted { .. } => "summary_retries_exhausted",
            Self::Persistence(_) => "persistence_error",
            Self::ChatState(_) => "chat_state_error",
            Self::State(_) => "state_error",
            Self::ActorStopped => "actor_stopped",
        }
    }
}

pub(super) struct ConversationCompactionRequest {
    pub session_id: SessionId,
    pub model_id: Option<String>,
    pub resolved_model_name: String,
    pub working_directory: PathBuf,
    pub skill_roots: SkillRoots,
    pub disabled_skill_names: BTreeSet<String>,
    pub agent: Agent,
    pub chat: ChatStateHandle,
    pub model: Arc<dyn ModelPort>,
    pub storage: Arc<dyn SessionStorage>,
    pub state_collector: Arc<CompactionStateCollector>,
    pub reload_required: Arc<AtomicBool>,
    pub trigger: CompactionTrigger,
    pub system_context: Option<ResolvedSystemContext>,
    pub trace: Arc<dyn TraceRecorder>,
    /// Turn 内触发时复用 Turn 的 Trace 根；手动压缩自开一个。
    pub trace_id: String,
    pub cancellation: CancellationToken,
}

pub(super) async fn run_compaction(
    request: ConversationCompactionRequest,
) -> Result<ConversationCompaction, CompactionError> {
    let turn_id = request.trigger.turn_id().cloned();
    let mut attributes = CompactionTraceAttributesV1::new(request.trigger.as_str());
    request.trigger.record(&mut attributes);
    let mut trace = CompactionTraceGuard::start(
        Arc::clone(&request.trace),
        CompactionStarted {
            span_id: format!("compaction-{}", Uuid::new_v4().simple()),
            trace_id: request.trace_id.clone(),
            session_id: request.session_id.clone(),
            turn_id,
            model_id: request.model_id.clone(),
            resolved_model_name: request.resolved_model_name.clone(),
            started_at: OffsetDateTime::now_utc(),
            attributes,
        },
        request.cancellation.clone(),
    );
    let result = run_compaction_inner(request, &mut trace).await;
    match &result {
        Ok(compaction) => {
            trace.attributes_mut().checkpoint_id = Some(compaction.id.clone());
            trace.finish_success(compaction.input_tokens, compaction.output_tokens);
        }
        Err(error) => trace.finish_failure(error.code(), error.to_string()),
    }
    result
}

async fn run_compaction_inner(
    request: ConversationCompactionRequest,
    trace: &mut CompactionTraceGuard,
) -> Result<ConversationCompaction, CompactionError> {
    let prepare_started = Instant::now();
    let source = request
        .chat
        .compaction_view()
        .await
        .map_err(|error| CompactionError::ChatState(error.to_string()))?;
    if source.items.is_empty() {
        return Err(CompactionError::EmptyConversation);
    }
    let source_message_count =
        u32::try_from(source.items.len()).map_err(|_| CompactionError::MessageCountOverflow)?;
    trace.attributes_mut().source_message_count = Some(source_message_count);
    let source_conversation = ConversationView {
        messages: source
            .items
            .iter()
            .map(|item| item.message.clone())
            .collect(),
    };
    // Trace is best-effort: a failed measurement leaves the attribute absent
    // rather than failing a compaction the user asked for. The Desktop history
    // renders the missing pair as "—" instead of inventing a zero.
    if let Ok(tokens) = estimate_conversation_tokens(&source_conversation) {
        trace
            .attributes_mut()
            .record_conversation_tokens_before(tokens);
    }
    let last_user = last_real_user(&source).ok_or(CompactionError::MissingLastUser)?;

    let mut state_messages = request
        .storage
        .load_compaction_source_messages(&request.session_id)
        .await
        .map_err(CompactionError::Persistence)?;
    if state_messages.is_empty() {
        state_messages = source
            .items
            .iter()
            .map(|item| item.message.clone())
            .collect();
    }
    let base_runtime_state = request
        .storage
        .load_latest_compaction_runtime_state(&request.session_id)
        .await
        .map_err(CompactionError::Persistence)?
        .unwrap_or_default();
    let (runtime_state, runtime_reminder) = request
        .state_collector
        .collect_with_base(&state_messages, base_runtime_state)
        .await
        .map_err(|error| CompactionError::State(error.to_string()))?;
    let system_context = match request.system_context {
        Some(context) => context,
        None => SystemContextBuilder::new(&request.working_directory, request.skill_roots.clone())
            .with_disabled_skills(request.disabled_skill_names.clone())
            .build(request.agent.system_prompt())
            .await
            .map_err(|error| CompactionError::Context(error.to_string()))?,
    };
    trace.attributes_mut().prepare_ms = Some(elapsed_millis(prepare_started));
    let summary_started = Instant::now();
    let summary_trace = SummaryTraceContext::new(
        Arc::clone(&request.trace),
        request.trace_id.clone(),
        request.session_id.clone(),
        request.trigger.turn_id().cloned(),
        trace.span_id().to_string(),
        request.model_id.clone(),
        request.cancellation.clone(),
    );
    let generated_result = generate_summary(
        request.model.as_ref(),
        &request.resolved_model_name,
        &system_context,
        source_conversation,
        format!(
            "{}-compaction-{}",
            request.session_id,
            Uuid::new_v4().simple()
        ),
        summary_trace,
        trace,
    )
    .await;
    trace.attributes_mut().summary_ms = Some(elapsed_millis(summary_started));
    let generated = generated_result?;
    trace.attributes_mut().summary_chars =
        Some(u64::try_from(generated.text.chars().count()).unwrap_or(u64::MAX));

    let trigger_turn_id = request.trigger.turn_id().cloned();
    let kind = request.trigger.kind();
    let (last_user_message_id, last_user_message_sequence) = last_user_source(&last_user.origin);
    let persistence_started = Instant::now();
    let persisted = request
        .storage
        .save_conversation_compaction(
            &request.session_id,
            NewConversationCompaction {
                kind,
                source_message_count,
                resolved_model_name: request.resolved_model_name,
                summary: generated.text,
                runtime_state,
                runtime_reminder,
                input_tokens: generated.input_tokens,
                output_tokens: generated.output_tokens,
                trigger_turn_id,
                last_user_message_id,
                last_user_message_sequence,
            },
        )
        .await
        .map_err(CompactionError::Persistence)?;
    trace.attributes_mut().persistence_ms = Some(elapsed_millis(persistence_started));

    let install_started = Instant::now();
    let replacement = compacted_items(&persisted, last_user.message.clone())?;
    let replacement_conversation = ConversationView {
        messages: replacement
            .iter()
            .map(|item| item.message.clone())
            .collect(),
    };
    if let Ok(tokens) = estimate_conversation_tokens(&replacement_conversation) {
        trace
            .attributes_mut()
            .record_conversation_tokens_after(tokens);
    }
    if let Err(error) = request.chat.replace_items(replacement).await {
        request.reload_required.store(true, Ordering::Release);
        return Err(CompactionError::ChatState(format!(
            "{error}; the durable checkpoint was preserved and this Session must be reloaded"
        )));
    }
    trace.attributes_mut().install_ms = Some(elapsed_millis(install_started));

    Ok(persisted)
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
}
