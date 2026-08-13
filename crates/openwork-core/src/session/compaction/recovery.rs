use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use openwork_chat_state::{ChatStateHandle, ConversationView};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::policy::AutomaticCompactionPolicy;
use super::projection::compacted_items;
use super::{
    CompactionError, CompactionStateCollector, ConversationCompaction, ConversationCompactionKind,
    elapsed_millis, new_trace_id,
};
use crate::context::estimate_conversation_tokens;
use crate::session::{
    CompactionStarted, CompactionTraceAttributesV1, CompactionTraceGuard, SessionId,
    SessionStorage, TraceRecorder, TurnId,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ConversationProjectionSelector {
    Latest,
    Compaction { compaction_id: String },
    ThroughMessage { sequence: i64 },
}

/// Why a compaction ran, carrying the evidence that caused it.
///
/// The cause travels with the trigger rather than as loose optional fields so
/// an impossible pairing — a manual compaction blamed on a Model Call Span —
/// cannot be constructed.
#[derive(Debug, Clone)]
pub(crate) enum CompactionTrigger {
    /// The user asked for it. Not measured against a context window.
    Manual,
    /// The pre-sampling estimate reached the configured threshold.
    Threshold {
        turn_id: TurnId,
        policy: AutomaticCompactionPolicy,
        estimated_input_tokens: u64,
    },
    /// The provider rejected the request for exceeding its input budget.
    Overflow {
        turn_id: TurnId,
        policy: AutomaticCompactionPolicy,
        /// What the failed submission was estimated at. Absent only if the
        /// overflow surfaced before any request was measured.
        estimated_input_tokens: Option<u64>,
        /// Model Call Span that failed, so the timeline can show the causal chain.
        model_span_id: Option<String>,
        error_code: String,
    },
}

impl CompactionTrigger {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Threshold { .. } => "threshold",
            Self::Overflow { .. } => "overflow",
        }
    }

    pub(crate) fn turn_id(&self) -> Option<&TurnId> {
        match self {
            Self::Manual => None,
            Self::Threshold { turn_id, .. } | Self::Overflow { turn_id, .. } => Some(turn_id),
        }
    }

    pub(crate) fn kind(&self) -> ConversationCompactionKind {
        match self {
            Self::Manual => ConversationCompactionKind::Manual,
            Self::Threshold { .. } => ConversationCompactionKind::Threshold,
            Self::Overflow { .. } => ConversationCompactionKind::Overflow,
        }
    }

    /// Copy the trigger's evidence onto the Span attributes.
    pub(crate) fn record(&self, attributes: &mut CompactionTraceAttributesV1) {
        match self {
            Self::Manual => {}
            Self::Threshold {
                policy,
                estimated_input_tokens,
                ..
            } => {
                attributes.record_trigger_estimate(*estimated_input_tokens);
                attributes.record_policy(policy.context_window_tokens, policy.threshold_percent);
            }
            Self::Overflow {
                policy,
                estimated_input_tokens,
                model_span_id,
                error_code,
                ..
            } => {
                if let Some(estimated_input_tokens) = estimated_input_tokens {
                    attributes.record_trigger_estimate(*estimated_input_tokens);
                }
                attributes.record_policy(policy.context_window_tokens, policy.threshold_percent);
                attributes.record_overflow_trigger(model_span_id.as_deref(), error_code);
            }
        }
    }
}

pub(crate) struct ConversationRewindRequest<'a> {
    pub session_id: &'a SessionId,
    pub compaction_id: &'a str,
    pub resolved_model_name: &'a str,
    pub chat: &'a ChatStateHandle,
    pub storage: &'a dyn SessionStorage,
    pub state_collector: &'a CompactionStateCollector,
    pub reload_required: &'a AtomicBool,
    pub trace: Arc<dyn TraceRecorder>,
}

/// Reinstall an earlier checkpoint's projection.
///
/// A rewind writes a `rewind` row to `conversation_compactions` exactly like the
/// other triggers do, so it records a Span too — otherwise a durable compaction
/// record would exist with nothing in the Trace to explain it. There is no
/// summary Model Call, so `attempt_count` stays zero and no model is attributed.
pub(crate) async fn rewind_conversation(
    request: ConversationRewindRequest<'_>,
) -> Result<ConversationCompaction, CompactionError> {
    let mut trace = CompactionTraceGuard::start(
        Arc::clone(&request.trace),
        CompactionStarted {
            span_id: format!("compaction-{}", Uuid::new_v4().simple()),
            trace_id: new_trace_id(),
            session_id: request.session_id.clone(),
            turn_id: None,
            model_id: None,
            resolved_model_name: request.resolved_model_name.to_string(),
            started_at: OffsetDateTime::now_utc(),
            attributes: CompactionTraceAttributesV1::new("rewind"),
        },
        CancellationToken::new(),
    );
    let result = rewind_conversation_inner(request, &mut trace).await;
    match &result {
        Ok(checkpoint) => {
            trace.attributes_mut().checkpoint_id = Some(checkpoint.id.clone());
            trace.finish_success(None, None);
        }
        Err(error) => trace.finish_failure(error.code(), error.to_string()),
    }
    result
}

async fn rewind_conversation_inner(
    request: ConversationRewindRequest<'_>,
    trace: &mut CompactionTraceGuard,
) -> Result<ConversationCompaction, CompactionError> {
    let prepare_started = Instant::now();
    if let Ok(source) = request.chat.conversation_view().await
        && let Ok(tokens) = estimate_conversation_tokens(&source)
    {
        trace
            .attributes_mut()
            .record_conversation_tokens_before(tokens);
    }
    let state_messages = request
        .storage
        .load_compaction_source_messages(request.session_id)
        .await
        .map_err(CompactionError::Persistence)?;
    let base_runtime_state = request
        .storage
        .load_latest_compaction_runtime_state(request.session_id)
        .await
        .map_err(CompactionError::Persistence)?
        .unwrap_or_default();
    // Rewind 重装的是某个更早 checkpoint 的投影，没有"当前 Turn"可言，因此不提供计划：
    // collector 会结转上次收集到的值，而不是把它当成一次清空。
    let (runtime_state, runtime_reminder) = request
        .state_collector
        .collect_with_base(&state_messages, None, base_runtime_state)
        .await
        .map_err(|error| CompactionError::State(error.to_string()))?;
    trace.attributes_mut().prepare_ms = Some(elapsed_millis(prepare_started));

    let persistence_started = Instant::now();
    let checkpoint = request
        .storage
        .rewind_conversation_compaction(
            request.session_id,
            request.compaction_id,
            runtime_state,
            runtime_reminder,
        )
        .await
        .map_err(CompactionError::Persistence)?;
    let last_user = request
        .storage
        .load_compaction_last_user_message(request.session_id, &checkpoint.id)
        .await
        .map_err(CompactionError::Persistence)?;
    trace.attributes_mut().persistence_ms = Some(elapsed_millis(persistence_started));
    trace.attributes_mut().summary_chars =
        Some(u64::try_from(checkpoint.summary.chars().count()).unwrap_or(u64::MAX));

    let install_started = Instant::now();
    let replacement = compacted_items(&checkpoint, last_user)?;
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
            "{error}; the durable rewind checkpoint was preserved and this Session must be reloaded"
        )));
    }
    trace.attributes_mut().install_ms = Some(elapsed_millis(install_started));
    Ok(checkpoint)
}
