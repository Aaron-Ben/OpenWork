use async_trait::async_trait;
use openwork_models::model::{
    FinishReason, ModelError, ModelEvent, ModelRequest, ModelResponse, ModelTransportObserver,
    ModelTransportSignal, ModelTransportSignalKind, ThinkingMode, TokenUsage,
};
use openwork_tools::{ToolResult, ToolResultStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use crate::model_call::ContextBudgetEstimate;

use super::{SessionId, TurnId};

const TRACE_SCHEMA_VERSION: u16 = 1;
const MAX_TRACE_STRING_CHARS: usize = 256;
const MAX_TRACE_ERROR_CHARS: usize = 512;
const MAX_ARTIFACT_TYPES: usize = 16;
pub const DEFAULT_TRACE_PAYLOAD_SLOT_MAX_BYTES: usize = 1024 * 1024;
pub const DEFAULT_TRACE_PAYLOAD_RETENTION_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceContentPolicy {
    #[default]
    Full,
    CompactionOnly,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceContentConfig {
    policy: TraceContentPolicy,
    slot_max_bytes: usize,
    retention_days: u32,
}

impl TraceContentConfig {
    pub fn new(
        policy: TraceContentPolicy,
        slot_max_bytes: usize,
        retention_days: u32,
    ) -> Result<Self, TraceContentConfigError> {
        if slot_max_bytes < 2 {
            return Err(TraceContentConfigError::SlotLimitTooSmall);
        }
        if retention_days == 0 || retention_days > i32::MAX as u32 {
            return Err(TraceContentConfigError::RetentionDaysOutOfRange);
        }
        Ok(Self {
            policy,
            slot_max_bytes,
            retention_days,
        })
    }

    pub fn policy(self) -> TraceContentPolicy {
        self.policy
    }

    pub fn slot_max_bytes(self) -> usize {
        self.slot_max_bytes
    }

    pub fn retention_days(self) -> u32 {
        self.retention_days
    }
}

impl Default for TraceContentConfig {
    fn default() -> Self {
        Self {
            policy: TraceContentPolicy::Full,
            slot_max_bytes: DEFAULT_TRACE_PAYLOAD_SLOT_MAX_BYTES,
            retention_days: DEFAULT_TRACE_PAYLOAD_RETENTION_DAYS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TraceContentConfigError {
    #[error("trace payload slot limit must be at least 2 bytes")]
    SlotLimitTooSmall,
    #[error("trace payload retention must be between 1 and 2147483647 days")]
    RetentionDaysOutOfRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracePayloadSlot {
    Request,
    SystemContext,
    ToolDefinitions,
    Response,
}

impl TracePayloadSlot {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::SystemContext => "system_context",
            Self::ToolDefinitions => "tool_definitions",
            Self::Response => "response",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TracePayloads {
    pub request: Option<Value>,
    pub system_context: Option<Value>,
    pub tool_definitions: Option<Value>,
}

impl TracePayloads {
    pub fn for_model_request(request: &ModelRequest) -> Self {
        Self {
            request: serde_json::to_value(&request.messages).ok(),
            system_context: None,
            tool_definitions: serde_json::to_value(&request.tools).ok(),
        }
    }

    pub(crate) fn for_model_call(
        request: &ModelRequest,
        system_context: &crate::context::ResolvedSystemContext,
    ) -> Self {
        let mut payloads = Self::for_model_request(request);
        payloads.system_context = serde_json::to_value(system_context.parts()).ok();
        payloads
    }

    fn clear(&mut self) {
        self.request = None;
        self.system_context = None;
        self.tool_definitions = None;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelTraceAttributesV1 {
    pub schema_version: u16,
    pub model_call_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    pub request_build_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_message_count: Option<u64>,
    #[serde(flatten)]
    request_context_budget: Option<Box<ContextBudgetTraceV1>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_definition_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_tool_call_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_chars: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_retry_delay_ms: Option<u64>,
}

impl ModelTraceAttributesV1 {
    pub fn from_request(
        model_call_index: u32,
        request_build_ms: u64,
        request: &ModelRequest,
    ) -> Self {
        Self {
            schema_version: TRACE_SCHEMA_VERSION,
            model_call_index,
            temperature: request.temperature,
            top_p: request.top_p,
            tool_choice: (!request.tools.is_empty()).then(|| "auto".to_string()),
            request_build_ms,
            ttft_ms: None,
            stream_ms: None,
            finish_reason: None,
            response_id: None,
            actual_model: None,
            error_phase: None,
            delivery_state: None,
            http_status: None,
            provider_code: None,
            request_message_count: Some(saturating_u64(request.messages.len())),
            request_context_budget: None,
            tool_definition_count: Some(saturating_u64(request.tools.len())),
            max_output_tokens: request.max_output_tokens,
            thinking_mode: request.thinking.map(|thinking| match thinking.mode {
                ThinkingMode::Enabled => "enabled".to_string(),
                ThinkingMode::Disabled => "disabled".to_string(),
            }),
            response_tool_call_count: None,
            summary_chars: None,
            summary_retry_delay_ms: None,
        }
    }

    pub(crate) fn record_context_budget(&mut self, estimate: ContextBudgetEstimate) {
        debug_assert_eq!(self.max_output_tokens, estimate.reserved_output_tokens);
        self.request_context_budget = Some(Box::new(ContextBudgetTraceV1 {
            request_estimated_system_context_tokens: estimate.system_context_tokens,
            request_estimated_conversation_tokens: estimate.conversation_tokens,
            request_estimated_tool_surface_tokens: estimate.tool_surface_tokens,
            request_estimated_input_tokens: estimate.estimated_input_tokens,
        }));
    }

    fn record_response(&mut self, response: &ModelResponse) {
        self.finish_reason = Some(normalized_finish_reason(&response.finish_reason).to_string());
        self.response_id = bounded_option(response.response_id.as_deref(), MAX_TRACE_STRING_CHARS);
        self.actual_model = bounded_option(response.model.as_deref(), MAX_TRACE_STRING_CHARS);
        self.response_tool_call_count = Some(saturating_u64(response.tool_calls.len()));
    }

    fn record_error(&mut self, error: &ModelError) {
        self.error_phase = Some(serialized_name(&error.phase));
        self.delivery_state = Some(serialized_name(&error.delivery));
        self.http_status = error.http_status;
        self.provider_code = bounded_option(error.provider_code.as_deref(), MAX_TRACE_STRING_CHARS);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextBudgetTraceV1 {
    request_estimated_system_context_tokens: u64,
    request_estimated_conversation_tokens: u64,
    request_estimated_tool_surface_tokens: u64,
    request_estimated_input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolTraceAttributesV1 {
    pub schema_version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly_proof_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_rule_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_rule_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_persisted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_types: Vec<String>,
}

/// Classified outcome of one compaction summary attempt.
///
/// A bare `failed` cannot distinguish "the model returned an unusable summary"
/// from "the provider rejected the credentials", which are the two cases that
/// need different operator responses. The classification is diagnostic only —
/// it does not currently steer the retry loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionAttemptOutcome {
    /// A usable summary was produced.
    Succeeded,
    /// The response arrived but was not a usable summary (too short, missing
    /// headings, truncated, requested a tool).
    Degenerate,
    /// Re-sending the same input cannot help (auth, invalid request, schema).
    Deterministic,
    /// The provider input budget was exceeded; retrying the same input cannot help.
    InputOverflow,
    /// A retry of the same input may succeed (network, overload, 5xx).
    Transient,
    /// The attempt exceeded its own wall-clock budget.
    Timeout,
}

impl CompactionAttemptOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Degenerate => "degenerate",
            Self::Deterministic => "deterministic",
            Self::InputOverflow => "input_overflow",
            Self::Transient => "transient",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionSummaryAttemptTrace {
    pub index: u32,
    pub status: CompactionAttemptOutcome,
    pub duration_ms: u64,
    pub model_attempt_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_chars: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionTraceAttributesV1 {
    pub schema_version: u16,
    pub trigger: String,
    /// Configured window the trigger was measured against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u64>,
    /// Automatic-compaction threshold in effect, even for a manual compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_percent: Option<u8>,
    /// Agent-loop input estimate that tripped the trigger. Absent for a manual
    /// compaction, which is not measured against the loop's own request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_estimated_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_percent: Option<u32>,
    /// Model Call Span whose context overflow forced an `overflow` compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_model_span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_error_code: Option<String>,
    /// Conversation-only token estimate before and after the replacement was
    /// installed, measured on the same basis so the difference is meaningful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_tokens_before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_tokens_after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reclaimed_conversation_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_message_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_request_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_estimated_system_context_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_estimated_conversation_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_estimated_tool_surface_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistence_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_chars: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
}

impl CompactionTraceAttributesV1 {
    pub fn new(trigger: impl Into<String>) -> Self {
        Self {
            schema_version: TRACE_SCHEMA_VERSION,
            trigger: trigger.into(),
            context_window_tokens: None,
            threshold_percent: None,
            trigger_estimated_input_tokens: None,
            trigger_percent: None,
            trigger_model_span_id: None,
            trigger_error_code: None,
            conversation_tokens_before: None,
            conversation_tokens_after: None,
            reclaimed_conversation_tokens: None,
            source_message_count: None,
            prepare_ms: None,
            summary_request_message_count: None,
            summary_estimated_system_context_tokens: None,
            summary_estimated_conversation_tokens: None,
            summary_estimated_tool_surface_tokens: None,
            summary_max_output_tokens: None,
            summary_ms: None,
            persistence_ms: None,
            install_ms: None,
            summary_chars: None,
            checkpoint_id: None,
        }
    }

    /// Record the policy the trigger was evaluated against. Only an automatic
    /// trigger has one; a manual compaction is not measured against a window,
    /// and recording a guessed default there would be worse than recording
    /// nothing.
    pub fn record_policy(&mut self, context_window_tokens: u64, threshold_percent: u8) {
        self.context_window_tokens = Some(context_window_tokens);
        self.threshold_percent = Some(threshold_percent);
        self.trigger_percent = self
            .trigger_estimated_input_tokens
            .and_then(|used| usage_percent(used, context_window_tokens));
    }

    /// Record the agent-loop input estimate that tripped an automatic trigger.
    pub fn record_trigger_estimate(&mut self, estimated_input_tokens: u64) {
        self.trigger_estimated_input_tokens = Some(estimated_input_tokens);
        self.trigger_percent = self
            .context_window_tokens
            .and_then(|window| usage_percent(estimated_input_tokens, window));
    }

    /// Record the Model Call whose context overflow forced this compaction.
    pub fn record_overflow_trigger(&mut self, model_span_id: Option<&str>, error_code: &str) {
        self.trigger_model_span_id = bounded_option(model_span_id, MAX_TRACE_STRING_CHARS);
        self.trigger_error_code = Some(bounded(error_code, MAX_TRACE_STRING_CHARS));
    }

    /// Record the conversation-only estimate before the summary call runs.
    pub fn record_conversation_tokens_before(&mut self, tokens: u64) {
        self.conversation_tokens_before = Some(tokens);
    }

    /// Record the conversation-only estimate for the installed replacement and
    /// derive how much the compaction actually reclaimed.
    pub fn record_conversation_tokens_after(&mut self, tokens: u64) {
        self.conversation_tokens_after = Some(tokens);
        self.reclaimed_conversation_tokens = self
            .conversation_tokens_before
            .map(|before| before.saturating_sub(tokens));
    }
}

impl ToolTraceAttributesV1 {
    pub fn new() -> Self {
        Self {
            schema_version: TRACE_SCHEMA_VERSION,
            permission_policy: None,
            permission_decision: None,
            permission_decision_source: None,
            readonly_proof_key: None,
            permission_rule_id: None,
            permission_rule_scope: None,
            execution_ms: None,
            artifact_count: None,
            error_retryable: None,
            result_persisted: None,
            output_truncated: None,
            artifact_types: Vec::new(),
        }
    }
}

impl Default for ToolTraceAttributesV1 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Running,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
    OutcomeUnknown,
    Degenerate,
    Deterministic,
    InputOverflow,
    Transient,
    Timeout,
}

impl TraceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Degenerate => "degenerate",
            Self::Deterministic => "deterministic",
            Self::InputOverflow => "input_overflow",
            Self::Transient => "transient",
            Self::Timeout => "timeout",
        }
    }
}

impl From<CompactionAttemptOutcome> for TraceStatus {
    fn from(outcome: CompactionAttemptOutcome) -> Self {
        match outcome {
            CompactionAttemptOutcome::Succeeded => Self::Succeeded,
            CompactionAttemptOutcome::Degenerate => Self::Degenerate,
            CompactionAttemptOutcome::Deterministic => Self::Deterministic,
            CompactionAttemptOutcome::InputOverflow => Self::InputOverflow,
            CompactionAttemptOutcome::Transient => Self::Transient,
            CompactionAttemptOutcome::Timeout => Self::Timeout,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCallStarted {
    pub span_id: String,
    /// 一次用户请求的全部 Span 共享的结构根。Turn 内的 Span 直接复用 Turn 的标识。
    pub trace_id: String,
    pub session_id: SessionId,
    pub turn_id: Option<TurnId>,
    /// Summary sampling is a child of the Compaction Span. Ordinary Agent Loop
    /// Model Calls remain top-level and leave this empty.
    pub parent_span_id: Option<String>,
    pub model_id: Option<String>,
    pub resolved_model_name: String,
    pub started_at: OffsetDateTime,
    pub attributes: ModelTraceAttributesV1,
    pub payloads: TracePayloads,
}

#[derive(Debug, Clone)]
pub struct ModelCallFinished {
    pub started: ModelCallStarted,
    pub status: TraceStatus,
    pub provider_request_id: Option<String>,
    pub attempt_count: Option<i32>,
    pub usage: Option<TokenUsage>,
    pub ended_at: OffsetDateTime,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attributes: ModelTraceAttributesV1,
    pub response_message_id: Option<String>,
    pub response_payload: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ToolCallStarted {
    pub span_id: String,
    pub trace_id: String,
    pub turn_id: TurnId,
    pub parent_span_id: String,
    pub provider_call_id: String,
    pub requested_tool_name: String,
    pub started_at: OffsetDateTime,
    pub attributes: ToolTraceAttributesV1,
}

#[derive(Debug, Clone)]
pub struct ToolCallFinished {
    pub started: ToolCallStarted,
    pub status: TraceStatus,
    pub resolved_tool_name: Option<String>,
    pub permission_wait_ms: Option<i64>,
    pub ended_at: OffsetDateTime,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attributes: ToolTraceAttributesV1,
    pub response_payload: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct CompactionStarted {
    pub span_id: String,
    /// 无 Turn 的压缩（manual / rewind）自开一个 Trace；有 Turn 的复用 Turn 的标识。
    pub trace_id: String,
    pub session_id: super::SessionId,
    pub turn_id: Option<TurnId>,
    pub model_id: Option<String>,
    pub resolved_model_name: String,
    pub started_at: OffsetDateTime,
    pub attributes: CompactionTraceAttributesV1,
}

#[derive(Debug, Clone)]
pub struct CompactionFinished {
    pub started: CompactionStarted,
    pub status: TraceStatus,
    pub attempt_count: Option<i32>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub ended_at: OffsetDateTime,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attributes: CompactionTraceAttributesV1,
}

#[derive(Debug)]
struct AttemptRuntime {
    index: u32,
    provider_request_id: Option<String>,
}

#[derive(Debug)]
struct ModelTraceState {
    attributes: ModelTraceAttributesV1,
    first_transport_started_at: Option<Instant>,
    first_semantic_at: Option<Instant>,
    stream_ended_at: Option<Instant>,
    max_transport_attempts: usize,
    attempts: Vec<AttemptRuntime>,
    partial_response: PartialModelResponse,
    completed_response_payload: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct PartialModelResponse {
    #[serde(skip_serializing_if = "String::is_empty")]
    text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    reasoning_text: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    tool_calls: BTreeMap<u32, PartialToolCall>,
}

impl PartialModelResponse {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.reasoning_text.is_empty() && self.tool_calls.is_empty()
    }

    fn to_value(&self) -> Option<Value> {
        (!self.is_empty())
            .then(|| serde_json::to_value(self).ok())
            .flatten()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct PartialToolCall {
    id: String,
    name: String,
    partial_input: String,
}

#[derive(Clone)]
struct ModelCallTransportObserver {
    state: Arc<Mutex<ModelTraceState>>,
}

impl ModelTransportObserver for ModelCallTransportObserver {
    fn observe(&self, signal: ModelTransportSignal) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Ok(index) = u32::try_from(signal.transport_attempt) else {
            return;
        };
        let position = state
            .attempts
            .iter()
            .position(|attempt| attempt.index == index);
        match signal.kind {
            ModelTransportSignalKind::Started => {
                let position = match position {
                    Some(position) => position,
                    None if state.attempts.len() < state.max_transport_attempts => {
                        state.attempts.push(AttemptRuntime {
                            index,
                            provider_request_id: None,
                        });
                        state.attempts.len() - 1
                    }
                    None => return,
                };
                state.attempts[position].provider_request_id = None;
                let now = Instant::now();
                if state.first_transport_started_at.is_none() {
                    state.first_transport_started_at = Some(now);
                }
            }
            ModelTransportSignalKind::Failed { error, .. } => {
                let Some(position) = position else {
                    return;
                };
                state.attempts[position].provider_request_id =
                    bounded_option(error.provider_request_id.as_deref(), MAX_TRACE_STRING_CHARS);
            }
            ModelTransportSignalKind::Succeeded {
                provider_request_id,
            } => {
                let Some(position) = position else {
                    return;
                };
                state.attempts[position].provider_request_id =
                    bounded_option(provider_request_id.as_deref(), MAX_TRACE_STRING_CHARS);
            }
        }
    }
}

pub struct ModelCallTraceGuard {
    recorder: Arc<dyn TraceRecorder>,
    started: ModelCallStarted,
    state: Arc<Mutex<ModelTraceState>>,
    cancellation: CancellationToken,
    finished: bool,
}

impl ModelCallTraceGuard {
    pub fn start(
        recorder: Arc<dyn TraceRecorder>,
        started: ModelCallStarted,
        cancellation: CancellationToken,
        max_transport_attempts: usize,
    ) -> Self {
        let state = Arc::new(Mutex::new(ModelTraceState {
            attributes: started.attributes.clone(),
            first_transport_started_at: None,
            first_semantic_at: None,
            stream_ended_at: None,
            max_transport_attempts: max_transport_attempts.max(1),
            attempts: Vec::new(),
            partial_response: PartialModelResponse::default(),
            completed_response_payload: None,
        }));
        recorder.record(TraceSignal::ModelCallStarted(Box::new(started.clone())));
        Self {
            recorder,
            started,
            state,
            cancellation,
            finished: false,
        }
    }

    pub fn span_id(&self) -> &str {
        &self.started.span_id
    }

    pub fn transport_observer(&self) -> Arc<dyn ModelTransportObserver> {
        Arc::new(ModelCallTransportObserver {
            state: Arc::clone(&self.state),
        })
    }

    pub fn record_first_semantic_event(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.first_semantic_at.is_none() {
            let now = Instant::now();
            state.attributes.ttft_ms = state
                .first_transport_started_at
                .map(|started_at| elapsed_between(started_at, now));
            state.first_semantic_at = Some(now);
        }
    }

    pub fn record_stream_finished(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.stream_ended_at = Some(Instant::now());
    }

    pub fn record_text_delta(&mut self, delta: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.partial_response.text.push_str(delta);
        }
    }

    pub fn record_reasoning_delta(&mut self, delta: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.partial_response.reasoning_text.push_str(delta);
        }
    }

    pub fn record_tool_call_start(&mut self, index: u32, id: &str, name: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.partial_response.tool_calls.insert(
                index,
                PartialToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    partial_input: String::new(),
                },
            );
        }
    }

    pub fn record_tool_call_delta(&mut self, index: u32, id: &str, partial_input: &str) {
        if let Ok(mut state) = self.state.lock() {
            let call = state
                .partial_response
                .tool_calls
                .entry(index)
                .or_insert_with(|| PartialToolCall {
                    id: id.to_string(),
                    ..PartialToolCall::default()
                });
            call.partial_input.push_str(partial_input);
        }
    }

    pub fn record_response_event(&mut self, event: &ModelEvent) {
        match event {
            ModelEvent::TextDelta { delta, .. } => self.record_text_delta(delta),
            ModelEvent::ReasoningDelta { delta, .. } => self.record_reasoning_delta(delta),
            ModelEvent::ToolCallStart { index, id, name } => {
                self.record_tool_call_start(*index, id, name);
            }
            ModelEvent::ToolCallDelta {
                index,
                id,
                partial_input,
            } => self.record_tool_call_delta(*index, id, partial_input),
            ModelEvent::TextStart { .. }
            | ModelEvent::TextEnd { .. }
            | ModelEvent::ReasoningStart { .. }
            | ModelEvent::ReasoningEnd { .. }
            | ModelEvent::ToolCallEnd { .. } => {}
            ModelEvent::ResponseCompleted { response } => {
                if let Ok(mut state) = self.state.lock() {
                    state.completed_response_payload = serde_json::to_value(response).ok();
                }
            }
        }
    }

    pub fn record_compaction_summary_attempt(
        &mut self,
        summary_chars: Option<u64>,
        retry_delay_ms: Option<u64>,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.attributes.summary_chars = summary_chars;
        state.attributes.summary_retry_delay_ms = retry_delay_ms;
    }

    pub fn finish_success(mut self, response: &ModelResponse) {
        let provider_request_id = response.provider_request_id.clone();
        let usage = response.usage;
        self.finish(
            TraceStatus::Succeeded,
            provider_request_id,
            usage,
            None,
            None,
            Some(response),
            None,
            None,
        );
    }

    pub fn finish_success_with_message(
        mut self,
        response: &ModelResponse,
        response_message_id: String,
    ) {
        self.finish(
            TraceStatus::Succeeded,
            response.provider_request_id.clone(),
            response.usage,
            None,
            None,
            Some(response),
            None,
            Some(response_message_id),
        );
    }

    pub fn finish_failure(
        mut self,
        status: TraceStatus,
        provider_request_id: Option<String>,
        error_code: impl Into<String>,
        error_message: impl Into<String>,
        model_error: Option<&ModelError>,
    ) {
        self.finish(
            status,
            provider_request_id,
            None,
            Some(error_code.into()),
            Some(error_message.into()),
            None,
            model_error,
            None,
        );
    }

    /// Finish a provider call that returned a complete response whose content
    /// was unusable for the caller-specific contract. Usage and response shape
    /// still belong on the Model Call Span even though the logical attempt
    /// failed validation.
    pub fn finish_response_failure(
        mut self,
        status: TraceStatus,
        response: &ModelResponse,
        error_code: impl Into<String>,
        error_message: impl Into<String>,
    ) {
        self.finish(
            status,
            response.provider_request_id.clone(),
            response.usage,
            Some(error_code.into()),
            Some(error_message.into()),
            Some(response),
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        status: TraceStatus,
        provider_request_id: Option<String>,
        usage: Option<TokenUsage>,
        error_code: Option<String>,
        error_message: Option<String>,
        response: Option<&ModelResponse>,
        model_error: Option<&ModelError>,
        response_message_id: Option<String>,
    ) {
        if self.finished {
            return;
        }
        let (
            attributes,
            attempt_count,
            observed_provider_request_id,
            completed_response_payload,
            partial_response,
        ) = match self.state.lock() {
            Ok(mut state) => {
                if let Some(first_semantic_at) = state.first_semantic_at {
                    state.attributes.stream_ms = Some(match state.stream_ended_at {
                        Some(stream_ended_at) => {
                            elapsed_between(first_semantic_at, stream_ended_at)
                        }
                        None => elapsed_millis(first_semantic_at),
                    });
                }
                if let Some(response) = response {
                    state.attributes.record_response(response);
                    state.attributes.delivery_state = Some(if state.first_semantic_at.is_some() {
                        "semantic_output_emitted".to_string()
                    } else {
                        "accepted_no_semantic_output".to_string()
                    });
                }
                if let Some(error) = model_error {
                    state.attributes.record_error(error);
                    if state.first_semantic_at.is_some() {
                        state.attributes.delivery_state =
                            Some("semantic_output_emitted".to_string());
                    }
                } else if status == TraceStatus::Cancelled {
                    state.attributes.error_phase = Some("cancelled".to_string());
                    state.attributes.delivery_state = Some(if state.first_semantic_at.is_some() {
                        "semantic_output_emitted".to_string()
                    } else {
                        "possibly_sent".to_string()
                    });
                }
                state.attempts.sort_by_key(|attempt| attempt.index);
                let count = i32::try_from(state.attempts.len())
                    .ok()
                    .filter(|count| *count > 0);
                let observed_provider_request_id = state
                    .attempts
                    .iter()
                    .rev()
                    .find_map(|attempt| attempt.provider_request_id.clone());
                (
                    state.attributes.clone(),
                    count,
                    observed_provider_request_id,
                    state.completed_response_payload.clone(),
                    state.partial_response.to_value(),
                )
            }
            Err(_) => (self.started.attributes.clone(), None, None, None, None),
        };
        let response_payload = response
            .and_then(|response| serde_json::to_value(response).ok())
            .or(completed_response_payload)
            .or(partial_response)
            .filter(|_| response_message_id.is_none());
        self.recorder
            .record(TraceSignal::ModelCallFinished(Box::new(
                ModelCallFinished {
                    started: self.started.clone(),
                    status,
                    provider_request_id: bounded_owned(
                        provider_request_id.or(observed_provider_request_id),
                        MAX_TRACE_STRING_CHARS,
                    ),
                    attempt_count,
                    usage,
                    ended_at: OffsetDateTime::now_utc(),
                    error_code: bounded_owned(error_code, MAX_TRACE_STRING_CHARS),
                    error_message: bounded_owned(error_message, MAX_TRACE_ERROR_CHARS),
                    attributes,
                    response_message_id,
                    response_payload,
                },
            )));
        self.finished = true;
    }
}

impl Drop for ModelCallTraceGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let (status, code, message) = if self.cancellation.is_cancelled() {
            (
                TraceStatus::Cancelled,
                "cancelled",
                "model trace scope cancelled before explicit finish",
            )
        } else {
            (
                TraceStatus::Failed,
                "scope_dropped",
                "model trace scope dropped before explicit finish",
            )
        };
        self.finish(
            status,
            None,
            None,
            Some(code.to_string()),
            Some(message.to_string()),
            None,
            None,
            None,
        );
    }
}

pub struct ToolCallTraceGuard {
    recorder: Arc<dyn TraceRecorder>,
    started: ToolCallStarted,
    attributes: ToolTraceAttributesV1,
    cancellation: CancellationToken,
    resolved_tool_name: Option<String>,
    permission_wait_ms: Option<i64>,
    finished: bool,
}

impl ToolCallTraceGuard {
    pub fn start(
        recorder: Arc<dyn TraceRecorder>,
        started: ToolCallStarted,
        cancellation: CancellationToken,
    ) -> Self {
        recorder.record(TraceSignal::ToolCallStarted(Box::new(started.clone())));
        Self {
            attributes: started.attributes.clone(),
            recorder,
            started,
            cancellation,
            resolved_tool_name: None,
            permission_wait_ms: None,
            finished: false,
        }
    }

    pub fn set_resolved_tool_name(&mut self, name: &str) {
        self.resolved_tool_name = Some(bounded(name, MAX_TRACE_STRING_CHARS));
    }

    pub fn record_permission_policy(&mut self, policy: &str) {
        self.attributes.permission_policy = Some(bounded(policy, MAX_TRACE_STRING_CHARS));
    }

    pub fn record_permission_decision(&mut self, decision: &str, source: &str) {
        self.attributes.permission_decision = Some(bounded(decision, MAX_TRACE_STRING_CHARS));
        self.attributes.permission_decision_source = Some(bounded(source, MAX_TRACE_STRING_CHARS));
    }

    pub fn record_readonly_proof(&mut self, key: &str) {
        self.attributes.readonly_proof_key = Some(bounded(key, MAX_TRACE_STRING_CHARS));
    }

    pub fn record_permission_rule(&mut self, rule_id: &str, rule_scope: &str) {
        self.attributes.permission_rule_id = Some(bounded(rule_id, MAX_TRACE_STRING_CHARS));
        self.attributes.permission_rule_scope = Some(bounded(rule_scope, MAX_TRACE_STRING_CHARS));
    }

    pub fn record_permission_wait_ms(&mut self, duration_ms: i64) {
        self.permission_wait_ms = Some(duration_ms.max(0));
    }

    pub fn record_execution_ms(&mut self, duration_ms: u64) {
        self.attributes.execution_ms = Some(duration_ms);
    }

    pub fn finish_result(mut self, result: &ToolResult, result_persisted: bool) {
        self.attributes.artifact_count = Some(saturating_u64(result.artifacts.len()));
        self.attributes.error_retryable = result.error.as_ref().map(|error| error.retryable);
        self.attributes.result_persisted = Some(result_persisted);
        let mut artifact_types = result
            .artifacts
            .iter()
            .map(|artifact| bounded(&artifact.kind, MAX_TRACE_STRING_CHARS))
            .collect::<Vec<_>>();
        artifact_types.sort();
        artifact_types.dedup();
        artifact_types.truncate(MAX_ARTIFACT_TYPES);
        self.attributes.artifact_types = artifact_types;
        self.finish(
            trace_status_for_tool_result(result.status),
            result
                .error
                .as_ref()
                .map(|error| serialized_name(&error.code)),
            result.error.as_ref().map(|error| error.message.clone()),
            (result.status == ToolResultStatus::Denied || !result_persisted)
                .then(|| serde_json::to_value(result).ok())
                .flatten(),
        );
    }

    fn finish(
        &mut self,
        status: TraceStatus,
        error_code: Option<String>,
        error_message: Option<String>,
        response_payload: Option<Value>,
    ) {
        if self.finished {
            return;
        }
        self.recorder
            .record(TraceSignal::ToolCallFinished(Box::new(ToolCallFinished {
                started: self.started.clone(),
                status,
                resolved_tool_name: self.resolved_tool_name.clone(),
                permission_wait_ms: self.permission_wait_ms,
                ended_at: OffsetDateTime::now_utc(),
                error_code: bounded_owned(error_code, MAX_TRACE_STRING_CHARS),
                error_message: bounded_owned(error_message, MAX_TRACE_ERROR_CHARS),
                attributes: self.attributes.clone(),
                response_payload,
            })));
        self.finished = true;
    }
}

impl Drop for ToolCallTraceGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let (status, code, message) = if self.cancellation.is_cancelled() {
            (
                TraceStatus::Cancelled,
                "cancelled",
                "tool trace scope cancelled before explicit finish",
            )
        } else {
            (
                TraceStatus::Failed,
                "scope_dropped",
                "tool trace scope dropped before explicit finish",
            )
        };
        self.finish(
            status,
            Some(code.to_string()),
            Some(message.to_string()),
            None,
        );
    }
}

pub struct CompactionTraceGuard {
    recorder: Arc<dyn TraceRecorder>,
    started: CompactionStarted,
    attributes: CompactionTraceAttributesV1,
    cancellation: CancellationToken,
    attempt_count: i32,
    finished: bool,
}

impl CompactionTraceGuard {
    pub fn start(
        recorder: Arc<dyn TraceRecorder>,
        started: CompactionStarted,
        cancellation: CancellationToken,
    ) -> Self {
        recorder.record(TraceSignal::CompactionStarted(Box::new(started.clone())));
        Self {
            recorder,
            attributes: started.attributes.clone(),
            started,
            cancellation,
            attempt_count: 0,
            finished: false,
        }
    }

    pub fn attributes_mut(&mut self) -> &mut CompactionTraceAttributesV1 {
        &mut self.attributes
    }

    pub fn span_id(&self) -> &str {
        &self.started.span_id
    }

    pub fn record_attempt_count(&mut self, attempt_count: usize) {
        self.attempt_count = i32::try_from(attempt_count).unwrap_or(i32::MAX);
    }

    pub fn finish_success(&mut self, input_tokens: Option<u64>, output_tokens: Option<u64>) {
        self.finish(
            TraceStatus::Succeeded,
            input_tokens,
            output_tokens,
            None,
            None,
        );
    }

    pub fn finish_failure(&mut self, error_code: impl Into<String>, error_message: String) {
        let status = if self.cancellation.is_cancelled() {
            TraceStatus::Cancelled
        } else {
            TraceStatus::Failed
        };
        self.finish(
            status,
            None,
            None,
            Some(error_code.into()),
            Some(error_message),
        );
    }

    fn finish(
        &mut self,
        status: TraceStatus,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        error_code: Option<String>,
        error_message: Option<String>,
    ) {
        if self.finished {
            return;
        }
        self.recorder
            .record(TraceSignal::CompactionFinished(Box::new(
                CompactionFinished {
                    started: self.started.clone(),
                    status,
                    attempt_count: Some(self.attempt_count),
                    input_tokens,
                    output_tokens,
                    ended_at: OffsetDateTime::now_utc(),
                    error_code: bounded_owned(error_code, MAX_TRACE_STRING_CHARS),
                    error_message: bounded_owned(error_message, MAX_TRACE_ERROR_CHARS),
                    attributes: self.attributes.clone(),
                },
            )));
        self.finished = true;
    }
}

impl Drop for CompactionTraceGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let (code, message) = if self.cancellation.is_cancelled() {
            (
                "cancelled",
                "compaction trace scope cancelled before explicit finish",
            )
        } else {
            (
                "scope_dropped",
                "compaction trace scope dropped before explicit finish",
            )
        };
        self.finish_failure(code, message.to_string());
    }
}

#[derive(Debug, Clone)]
pub enum TraceSignal {
    ModelCallStarted(Box<ModelCallStarted>),
    ModelCallFinished(Box<ModelCallFinished>),
    ToolCallStarted(Box<ToolCallStarted>),
    ToolCallFinished(Box<ToolCallFinished>),
    CompactionStarted(Box<CompactionStarted>),
    CompactionFinished(Box<CompactionFinished>),
}

impl TraceSignal {
    pub(crate) fn apply_content_policy(&mut self, policy: TraceContentPolicy) {
        let keep_model_payloads = |started: &ModelCallStarted| match policy {
            TraceContentPolicy::Full => true,
            TraceContentPolicy::CompactionOnly => started.parent_span_id.is_some(),
            TraceContentPolicy::Off => false,
        };
        match self {
            Self::ModelCallStarted(started) => {
                if !keep_model_payloads(started) {
                    started.payloads.clear();
                }
            }
            Self::ModelCallFinished(finished) => {
                if !keep_model_payloads(&finished.started) {
                    finished.started.payloads.clear();
                    finished.response_payload = None;
                }
            }
            Self::ToolCallFinished(finished) => {
                if policy != TraceContentPolicy::Full {
                    finished.response_payload = None;
                }
            }
            Self::ToolCallStarted(_) | Self::CompactionStarted(_) | Self::CompactionFinished(_) => {
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceFlushResult {
    pub flushed: bool,
    pub dropped_signals: u64,
    pub write_failures: u64,
}

#[async_trait]
pub trait TraceRecorder: Send + Sync {
    fn record(&self, signal: TraceSignal);

    async fn flush_turn(&self, turn_id: &TurnId) -> TraceFlushResult;

    async fn flush_session(&self, _session_id: &super::SessionId) -> TraceFlushResult {
        TraceFlushResult {
            flushed: true,
            ..TraceFlushResult::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct NoopTraceRecorder;

#[async_trait]
impl TraceRecorder for NoopTraceRecorder {
    fn record(&self, _signal: TraceSignal) {}

    async fn flush_turn(&self, _turn_id: &TurnId) -> TraceFlushResult {
        TraceFlushResult {
            flushed: true,
            ..TraceFlushResult::default()
        }
    }
}

fn trace_status_for_tool_result(status: ToolResultStatus) -> TraceStatus {
    match status {
        ToolResultStatus::Succeeded => TraceStatus::Succeeded,
        ToolResultStatus::Failed => TraceStatus::Failed,
        ToolResultStatus::Denied => TraceStatus::Denied,
        ToolResultStatus::Cancelled => TraceStatus::Cancelled,
        ToolResultStatus::OutcomeUnknown => TraceStatus::OutcomeUnknown,
    }
}

fn normalized_finish_reason(reason: &FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolUse => "tool_use",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Refusal => "refusal",
        FinishReason::Cancelled => "cancelled",
        FinishReason::Incomplete => "incomplete",
        FinishReason::Unknown(_) => "unknown",
    }
}

fn serialized_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Rounded percentage of `window` consumed by `used`. `None` when the window is
/// unknown; deliberately not clamped to 100 so an overflow trigger stays visible.
fn usage_percent(used: u64, window: u64) -> Option<u32> {
    (window > 0).then(|| {
        let scaled = (u128::from(used) * 100 + u128::from(window) / 2) / u128::from(window);
        u32::try_from(scaled).unwrap_or(u32::MAX)
    })
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn elapsed_between(started_at: Instant, ended_at: Instant) -> u64 {
    ended_at
        .saturating_duration_since(started_at)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn bounded_option(value: Option<&str>, max_chars: usize) -> Option<String> {
    value.map(|value| bounded(value, max_chars))
}

fn bounded_owned(value: Option<String>, max_chars: usize) -> Option<String> {
    value.map(|value| bounded(&value, max_chars))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use openwork_models::model::{
        ContentBlock, FinishReason, Message, ModelError, ModelErrorCode, ModelRequest,
        ModelResponse, ModelTransportSignal, ModelTransportSignalKind, RetryHint, Role,
        ToolCallBlock, ToolCallState,
    };
    use tokio_util::sync::CancellationToken;

    use super::*;

    #[derive(Default)]
    struct RecordingTrace {
        signals: Mutex<Vec<TraceSignal>>,
    }

    #[async_trait]
    impl TraceRecorder for RecordingTrace {
        fn record(&self, signal: TraceSignal) {
            self.signals.lock().unwrap().push(signal);
        }

        async fn flush_turn(&self, _turn_id: &TurnId) -> TraceFlushResult {
            TraceFlushResult {
                flushed: true,
                ..TraceFlushResult::default()
            }
        }
    }

    #[test]
    fn tool_trace_attributes_accept_legacy_json_without_p2_permission_fields() {
        let attributes: ToolTraceAttributesV1 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "permissionPolicy": "allow",
            "permissionDecision": "allow",
            "permissionDecisionSource": "builtin",
            "artifactTypes": []
        }))
        .expect("legacy tool attributes");

        assert!(attributes.readonly_proof_key.is_none());
        assert!(attributes.permission_rule_id.is_none());
        assert!(attributes.permission_rule_scope.is_none());
    }

    #[test]
    fn content_payload_whitelist_excludes_transport_credentials_headers_and_errors() {
        let request = ModelRequest {
            model: "model-name-is-not-part-of-request-slot".to_string(),
            messages: vec![Message::text(Role::User, "safe user content")],
            temperature: Some(0.2),
            top_p: Some(0.8),
            max_output_tokens: Some(128),
            thinking: None,
            tools: Vec::new(),
        };
        let payloads = TracePayloads::for_model_request(&request);
        let encoded = serde_json::to_string(&[
            payloads.request,
            payloads.system_context,
            payloads.tool_definitions,
        ])
        .unwrap();

        assert!(encoded.contains("safe user content"));
        for forbidden in [
            "sk-test-api-key",
            "Authorization: Bearer",
            "raw-provider-error-body",
            "model-name-is-not-part-of-request-slot",
        ] {
            assert!(!encoded.contains(forbidden));
        }
    }

    #[test]
    fn trace_content_defaults_to_full_with_a_one_megabyte_slot_limit_and_thirty_day_retention() {
        let config = TraceContentConfig::default();

        assert_eq!(config.policy(), TraceContentPolicy::Full);
        assert_eq!(config.slot_max_bytes(), 1024 * 1024);
        assert_eq!(config.retention_days(), 30);
    }

    #[test]
    fn trace_content_rejects_zero_day_retention() {
        assert_eq!(
            TraceContentConfig::new(TraceContentPolicy::Full, 1024, 0),
            Err(TraceContentConfigError::RetentionDaysOutOfRange)
        );
    }

    #[test]
    fn model_guard_counts_retries_without_serializing_attempt_details() {
        let request = ModelRequest {
            model: "test-model".to_string(),
            messages: vec![
                Message::text(Role::System, "secret system prompt"),
                Message::text(Role::User, "secret user prompt"),
            ],
            temperature: Some(0.25),
            top_p: Some(0.75),
            max_output_tokens: Some(4096),
            thinking: None,
            tools: vec![openwork_models::model::ToolDefinition {
                name: "read".to_string(),
                description: "secret tool description".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        };
        let mut attributes = ModelTraceAttributesV1::from_request(2, 7, &request);
        let estimate = ContextBudgetEstimate {
            system_context_tokens: 15,
            conversation_tokens: 8,
            tool_surface_tokens: 3,
            estimated_input_tokens: 26,
            reserved_output_tokens: request.max_output_tokens,
        };
        attributes.record_context_budget(estimate);
        let recorder = Arc::new(RecordingTrace::default());
        let recorder_port: Arc<dyn TraceRecorder> = recorder.clone();
        let mut guard = ModelCallTraceGuard::start(
            recorder_port,
            ModelCallStarted {
                span_id: "model-1".to_string(),
                trace_id: "turn-1".to_string(),
                session_id: SessionId::new("session-1"),
                turn_id: Some(TurnId::new("turn-1")),
                parent_span_id: None,
                model_id: None,
                resolved_model_name: "test-model".to_string(),
                started_at: OffsetDateTime::now_utc(),
                attributes,
                payloads: TracePayloads::for_model_request(&request),
            },
            CancellationToken::new(),
            3,
        );
        let observer = guard.transport_observer();
        observer.observe(ModelTransportSignal {
            model_attempt_id: "turn-1-model-2".to_string(),
            transport_attempt: 1,
            kind: ModelTransportSignalKind::Started,
        });
        observer.observe(ModelTransportSignal {
            model_attempt_id: "turn-1-model-2".to_string(),
            transport_attempt: 1,
            kind: ModelTransportSignalKind::Failed {
                error: ModelError::timeout(),
                retry_delay_ms: Some(25),
            },
        });
        observer.observe(ModelTransportSignal {
            model_attempt_id: "turn-1-model-2".to_string(),
            transport_attempt: 2,
            kind: ModelTransportSignalKind::Started,
        });
        observer.observe(ModelTransportSignal {
            model_attempt_id: "turn-1-model-2".to_string(),
            transport_attempt: 2,
            kind: ModelTransportSignalKind::Succeeded {
                provider_request_id: Some("provider-request-2".to_string()),
            },
        });
        guard.record_first_semantic_event();
        guard.finish_success(&ModelResponse {
            response_id: Some("response-1".to_string()),
            provider_request_id: None,
            model: Some("actual-model".to_string()),
            text: "secret response".to_string(),
            reasoning_text: Some("secret reasoning".to_string()),
            tool_calls: vec![ToolCallBlock {
                id: "call-1".to_string(),
                name: "read".to_string(),
                input: "{\"secret\":true}".to_string(),
                state: ToolCallState::Submitted,
            }],
            provider_opaque_blocks: Vec::new(),
            finish_reason: FinishReason::ToolUse,
            raw_finish_reason: Some("tool_calls".to_string()),
            usage: None,
        });

        let signals = recorder.signals.lock().unwrap();
        let finished = signals
            .iter()
            .find_map(|signal| match signal {
                TraceSignal::ModelCallFinished(finished) => Some(finished),
                _ => None,
            })
            .expect("finished signal");
        assert_eq!(finished.attempt_count, Some(2));
        assert_eq!(
            finished.provider_request_id.as_deref(),
            Some("provider-request-2")
        );
        assert_eq!(finished.attributes.temperature, Some(0.25));
        assert_eq!(finished.attributes.top_p, Some(0.75));
        assert_eq!(finished.attributes.tool_choice.as_deref(), Some("auto"));
        assert_eq!(
            finished.attributes.finish_reason.as_deref(),
            Some("tool_use")
        );
        assert_eq!(finished.attributes.response_tool_call_count, Some(1));
        assert!(finished.attributes.ttft_ms.is_some());
        assert_eq!(
            finished
                .attributes
                .request_context_budget
                .as_deref()
                .map(|budget| budget.request_estimated_input_tokens),
            Some(estimate.estimated_input_tokens)
        );

        let encoded = serde_json::to_string(&finished.attributes).expect("attributes serialize");
        assert!(!encoded.contains("\"attempts\""));
        assert!(!encoded.contains("\"appVersion\""));
        for removed in [
            "requestSystemMessageCount",
            "requestUserMessageCount",
            "requestAssistantMessageCount",
            "requestToolMessageCount",
            "requestContentBytes",
            "toolDefinitionBytes",
            "responseTextBytes",
            "responseReasoningBytes",
            "responseToolArgumentsBytes",
        ] {
            assert!(
                !encoded.contains(removed),
                "serialized removed field {removed}"
            );
        }
        let decoded: ModelTraceAttributesV1 =
            serde_json::from_str(&encoded).expect("attributes round trip");
        assert_eq!(decoded, finished.attributes);
        for secret in [
            "secret system prompt",
            "secret user prompt",
            "secret tool description",
            "secret response",
            "secret reasoning",
            "secret\\\":true",
        ] {
            assert!(!encoded.contains(secret), "attributes leaked {secret}");
        }
        assert!(matches!(
            request.messages[0].content[0],
            ContentBlock::Text(_)
        ));

        let mut with_unknown =
            serde_json::to_value(&finished.attributes).expect("attributes value");
        with_unknown
            .as_object_mut()
            .expect("attributes object")
            .insert(
                "rawPrompt".to_string(),
                serde_json::json!("must be rejected"),
            );
        assert!(serde_json::from_value::<ModelTraceAttributesV1>(with_unknown).is_err());
    }

    #[test]
    fn model_guard_keeps_semantic_delivery_when_stream_decode_fails() {
        let request = ModelRequest::text("test-model", "hello");
        let attributes = ModelTraceAttributesV1::from_request(1, 0, &request);
        let recorder = Arc::new(RecordingTrace::default());
        let recorder_port: Arc<dyn TraceRecorder> = recorder.clone();
        let mut guard = ModelCallTraceGuard::start(
            recorder_port,
            ModelCallStarted {
                span_id: "model-stream-error".to_string(),
                trace_id: "turn-stream-error".to_string(),
                session_id: SessionId::new("session-stream-error"),
                turn_id: Some(TurnId::new("turn-stream-error")),
                parent_span_id: None,
                model_id: None,
                resolved_model_name: "test-model".to_string(),
                started_at: OffsetDateTime::now_utc(),
                attributes,
                payloads: TracePayloads::for_model_request(&request),
            },
            CancellationToken::new(),
            3,
        );
        guard.transport_observer().observe(ModelTransportSignal {
            model_attempt_id: "turn-stream-error-model-1".to_string(),
            transport_attempt: 1,
            kind: ModelTransportSignalKind::Started,
        });
        guard.record_first_semantic_event();
        guard.record_text_delta("safe partial semantic output");
        let error = ModelError::protocol("unredacted-provider-error-body-must-not-enter-payload");
        guard.finish_failure(
            TraceStatus::Failed,
            None,
            "model_protocol_error",
            error.to_string(),
            Some(&error),
        );

        let signals = recorder.signals.lock().unwrap();
        let finished = signals
            .iter()
            .find_map(|signal| match signal {
                TraceSignal::ModelCallFinished(finished) => Some(finished),
                _ => None,
            })
            .expect("finished signal");
        assert_eq!(
            finished.attributes.error_phase.as_deref(),
            Some("stream_decode")
        );
        assert_eq!(
            finished.attributes.delivery_state.as_deref(),
            Some("semantic_output_emitted")
        );
        let response_payload = finished
            .response_payload
            .as_ref()
            .expect("partial response payload");
        assert_eq!(response_payload["text"], "safe partial semantic output");
        assert!(
            !serde_json::to_string(response_payload)
                .unwrap()
                .contains("unredacted-provider-error-body")
        );
    }

    #[test]
    fn model_guard_keeps_only_the_final_transport_failure() {
        let request = ModelRequest::text("test-model", "hello");
        let attributes = ModelTraceAttributesV1::from_request(1, 0, &request);
        let recorder = Arc::new(RecordingTrace::default());
        let recorder_port: Arc<dyn TraceRecorder> = recorder.clone();
        let guard = ModelCallTraceGuard::start(
            recorder_port,
            ModelCallStarted {
                span_id: "model-final-failure".to_string(),
                trace_id: "turn-final-failure".to_string(),
                session_id: SessionId::new("session-final-failure"),
                turn_id: Some(TurnId::new("turn-final-failure")),
                parent_span_id: None,
                model_id: None,
                resolved_model_name: "test-model".to_string(),
                started_at: OffsetDateTime::now_utc(),
                attributes,
                payloads: TracePayloads::for_model_request(&request),
            },
            CancellationToken::new(),
            3,
        );
        let observer = guard.transport_observer();
        let first_error = ModelError::http(
            ModelErrorCode::RateLimited,
            429,
            "rate limited",
            Some("rate_limited".to_string()),
            Some("request-1".to_string()),
            RetryHint::Backoff,
        );
        let final_error = ModelError::http(
            ModelErrorCode::ServerError,
            503,
            "provider unavailable",
            Some("server_unavailable".to_string()),
            Some("request-2".to_string()),
            RetryHint::Backoff,
        );
        for (transport_attempt, error) in [(1, first_error), (2, final_error.clone())] {
            observer.observe(ModelTransportSignal {
                model_attempt_id: "turn-final-failure-model-1".to_string(),
                transport_attempt,
                kind: ModelTransportSignalKind::Started,
            });
            observer.observe(ModelTransportSignal {
                model_attempt_id: "turn-final-failure-model-1".to_string(),
                transport_attempt,
                kind: ModelTransportSignalKind::Failed {
                    error,
                    retry_delay_ms: Some(25),
                },
            });
        }
        guard.finish_failure(
            TraceStatus::Failed,
            None,
            "server_error",
            final_error.to_string(),
            Some(&final_error),
        );

        let signals = recorder.signals.lock().unwrap();
        assert_eq!(signals.len(), 2, "retries must not create attempt spans");
        let finished = signals
            .iter()
            .find_map(|signal| match signal {
                TraceSignal::ModelCallFinished(finished) => Some(finished),
                _ => None,
            })
            .expect("finished signal");
        assert_eq!(finished.attempt_count, Some(2));
        assert_eq!(finished.provider_request_id.as_deref(), Some("request-2"));
        assert_eq!(finished.attributes.http_status, Some(503));
        assert_eq!(
            finished.attributes.provider_code.as_deref(),
            Some("server_unavailable")
        );
        assert!(
            serde_json::to_value(&finished.attributes)
                .expect("model attributes")
                .get("attempts")
                .is_none()
        );
    }

    #[test]
    fn compaction_attributes_derive_the_trigger_percent_in_either_order() {
        let mut policy_first = CompactionTraceAttributesV1::new("threshold");
        policy_first.record_policy(200_000, 85);
        policy_first.record_trigger_estimate(170_000);

        let mut estimate_first = CompactionTraceAttributesV1::new("threshold");
        estimate_first.record_trigger_estimate(170_000);
        estimate_first.record_policy(200_000, 85);

        assert_eq!(policy_first.trigger_percent, Some(85));
        assert_eq!(estimate_first.trigger_percent, Some(85));
        assert_eq!(estimate_first.context_window_tokens, Some(200_000));
        assert_eq!(estimate_first.threshold_percent, Some(85));
    }

    #[test]
    fn compaction_attributes_keep_an_overflow_percent_above_one_hundred() {
        let mut attributes = CompactionTraceAttributesV1::new("overflow");
        attributes.record_trigger_estimate(220_000);
        attributes.record_policy(200_000, 85);

        assert_eq!(attributes.trigger_percent, Some(110));
    }

    #[test]
    fn compaction_attributes_derive_what_the_replacement_reclaimed() {
        let mut attributes = CompactionTraceAttributesV1::new("manual");
        attributes.record_conversation_tokens_before(12_000);
        attributes.record_conversation_tokens_after(900);

        assert_eq!(attributes.reclaimed_conversation_tokens, Some(11_100));
    }

    #[test]
    fn compaction_attributes_do_not_report_a_negative_reclaim() {
        let mut attributes = CompactionTraceAttributesV1::new("manual");
        attributes.record_conversation_tokens_before(500);
        attributes.record_conversation_tokens_after(900);

        assert_eq!(attributes.reclaimed_conversation_tokens, Some(0));
    }

    #[test]
    fn compaction_attributes_serialize_without_the_unset_trigger_evidence() {
        let attributes = CompactionTraceAttributesV1::new("manual");

        let json = serde_json::to_value(&attributes).expect("serializable attributes");
        assert_eq!(json["trigger"], "manual");
        assert!(json.get("triggerModelSpanId").is_none());
        assert!(json.get("contextWindowTokens").is_none());
        assert!(json.get("attemptRollup").is_none());
        assert!(json.get("appVersion").is_none());
    }
}
