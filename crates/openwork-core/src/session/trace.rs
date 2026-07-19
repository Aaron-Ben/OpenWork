use async_trait::async_trait;
use openwork_models::model::{
    FinishReason, ModelError, ModelRequest, ModelResponse, ModelTransportObserver,
    ModelTransportSignal, ModelTransportSignalKind, Role, ThinkingMode, TokenUsage,
};
use openwork_tools::{ToolResult, ToolResultStatus};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use super::TurnId;

const TRACE_SCHEMA_VERSION: u16 = 1;
const MAX_TRACE_STRING_CHARS: usize = 256;
const MAX_TRACE_ERROR_CHARS: usize = 512;
const MAX_ARTIFACT_TYPES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelTransportAttemptTrace {
    pub index: u32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_delay_ms: Option<u64>,
}

impl ModelTransportAttemptTrace {
    fn started(index: u32) -> Self {
        Self {
            index,
            status: "started".to_string(),
            duration_ms: None,
            error_code: None,
            error_phase: None,
            delivery_state: None,
            http_status: None,
            provider_code: None,
            provider_request_id: None,
            retry_delay_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelTraceAttributesV1 {
    pub schema_version: u16,
    pub model_call_index: u32,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ModelTransportAttemptTrace>,
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_system_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_user_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_assistant_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_tool_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_content_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_definition_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_definition_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_reasoning_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_tool_call_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_tool_arguments_bytes: Option<u64>,
}

impl ModelTraceAttributesV1 {
    pub fn from_request(
        model_call_index: u32,
        request_build_ms: u64,
        request: &ModelRequest,
    ) -> Self {
        let mut system = 0_u64;
        let mut user = 0_u64;
        let mut assistant = 0_u64;
        let mut tool = 0_u64;
        for message in &request.messages {
            match message.role {
                Role::System => system = system.saturating_add(1),
                Role::User => user = user.saturating_add(1),
                Role::Assistant => assistant = assistant.saturating_add(1),
                Role::Tool => tool = tool.saturating_add(1),
            }
        }
        Self {
            schema_version: TRACE_SCHEMA_VERSION,
            model_call_index,
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
            attempts: Vec::new(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            request_message_count: Some(saturating_u64(request.messages.len())),
            request_system_message_count: Some(system),
            request_user_message_count: Some(user),
            request_assistant_message_count: Some(assistant),
            request_tool_message_count: Some(tool),
            request_content_bytes: Some(serialized_bytes(
                &request
                    .messages
                    .iter()
                    .map(|message| &message.content)
                    .collect::<Vec<_>>(),
            )),
            tool_definition_count: Some(saturating_u64(request.tools.len())),
            tool_definition_bytes: Some(serialized_bytes(&request.tools)),
            max_output_tokens: request.max_output_tokens,
            thinking_mode: request.thinking.map(|thinking| match thinking.mode {
                ThinkingMode::Enabled => "enabled".to_string(),
                ThinkingMode::Disabled => "disabled".to_string(),
            }),
            response_text_bytes: None,
            response_reasoning_bytes: None,
            response_tool_call_count: None,
            response_tool_arguments_bytes: None,
        }
    }

    fn record_response(&mut self, response: &ModelResponse) {
        self.finish_reason = Some(normalized_finish_reason(&response.finish_reason).to_string());
        self.response_id = bounded_option(response.response_id.as_deref(), MAX_TRACE_STRING_CHARS);
        self.actual_model = bounded_option(response.model.as_deref(), MAX_TRACE_STRING_CHARS);
        self.response_text_bytes = Some(saturating_u64(response.text.len()));
        self.response_reasoning_bytes = response
            .reasoning_text
            .as_ref()
            .map(|value| saturating_u64(value.len()));
        self.response_tool_call_count = Some(saturating_u64(response.tool_calls.len()));
        self.response_tool_arguments_bytes =
            Some(response.tool_calls.iter().fold(0_u64, |sum, call| {
                sum.saturating_add(saturating_u64(call.input.len()))
            }));
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
pub struct ToolTraceAttributesV1 {
    pub schema_version: u16,
    pub input_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_lines: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_persisted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_persist_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_persist_error_code: Option<String>,
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_top_level_key_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_event_count: Option<u64>,
}

impl ToolTraceAttributesV1 {
    pub fn new(input: &str) -> Self {
        Self {
            schema_version: TRACE_SCHEMA_VERSION,
            input_bytes: saturating_u64(input.len()),
            validation_ms: None,
            permission_policy: None,
            permission_decision: None,
            permission_decision_source: None,
            execution_ms: None,
            output_bytes: None,
            output_lines: None,
            artifact_count: None,
            error_retryable: None,
            result_persisted: None,
            result_persist_ms: None,
            result_persist_error_code: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            input_top_level_key_count: None,
            output_truncated: None,
            artifact_types: Vec::new(),
            progress_event_count: Some(0),
        }
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCallStarted {
    pub span_id: String,
    pub turn_id: TurnId,
    pub sequence: i64,
    pub model_id: Option<String>,
    pub resolved_model_name: String,
    pub started_at: OffsetDateTime,
    pub attributes: ModelTraceAttributesV1,
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
}

#[derive(Debug, Clone)]
pub struct ToolCallStarted {
    pub span_id: String,
    pub turn_id: TurnId,
    pub parent_span_id: String,
    pub sequence: i64,
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
}

#[derive(Debug)]
struct AttemptRuntime {
    trace: ModelTransportAttemptTrace,
    started_at: Option<Instant>,
}

#[derive(Debug)]
struct ModelTraceState {
    attributes: ModelTraceAttributesV1,
    first_transport_started_at: Option<Instant>,
    first_semantic_at: Option<Instant>,
    stream_ended_at: Option<Instant>,
    max_transport_attempts: usize,
    attempts: Vec<AttemptRuntime>,
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
            .position(|attempt| attempt.trace.index == index);
        let position = match position {
            Some(position) => position,
            None if state.attempts.len() < state.max_transport_attempts => {
                state.attempts.push(AttemptRuntime {
                    trace: ModelTransportAttemptTrace::started(index),
                    started_at: None,
                });
                state.attempts.len() - 1
            }
            None => return,
        };
        match signal.kind {
            ModelTransportSignalKind::Started => {
                let now = Instant::now();
                if state.first_transport_started_at.is_none() {
                    state.first_transport_started_at = Some(now);
                }
                let attempt = &mut state.attempts[position];
                attempt.trace = ModelTransportAttemptTrace::started(index);
                attempt.started_at = Some(now);
            }
            ModelTransportSignalKind::Failed {
                error,
                retry_delay_ms,
            } => {
                let attempt = &mut state.attempts[position];
                attempt.trace.status = "failed".to_string();
                attempt.trace.duration_ms = attempt.started_at.map(elapsed_millis);
                attempt.trace.error_code = Some(serialized_name(&error.kind));
                attempt.trace.error_phase = Some(serialized_name(&error.phase));
                attempt.trace.delivery_state = Some(serialized_name(&error.delivery));
                attempt.trace.http_status = error.http_status;
                attempt.trace.provider_code =
                    bounded_option(error.provider_code.as_deref(), MAX_TRACE_STRING_CHARS);
                attempt.trace.provider_request_id =
                    bounded_option(error.provider_request_id.as_deref(), MAX_TRACE_STRING_CHARS);
                attempt.trace.retry_delay_ms = retry_delay_ms;
            }
            ModelTransportSignalKind::Succeeded {
                provider_request_id,
            } => {
                let attempt = &mut state.attempts[position];
                attempt.trace.status = "succeeded".to_string();
                attempt.trace.duration_ms = attempt.started_at.map(elapsed_millis);
                attempt.trace.provider_request_id =
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
        }));
        recorder.record(TraceSignal::ModelCallStarted(started.clone()));
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
    ) {
        if self.finished {
            return;
        }
        let (attributes, attempt_count, observed_provider_request_id) = match self.state.lock() {
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
                state.attempts.sort_by_key(|attempt| attempt.trace.index);
                state.attributes.attempts = state
                    .attempts
                    .iter()
                    .map(|attempt| attempt.trace.clone())
                    .collect();
                let count = i32::try_from(state.attempts.len())
                    .ok()
                    .filter(|count| *count > 0);
                let observed_provider_request_id = state
                    .attempts
                    .iter()
                    .rev()
                    .find_map(|attempt| attempt.trace.provider_request_id.clone());
                (
                    state.attributes.clone(),
                    count,
                    observed_provider_request_id,
                )
            }
            Err(_) => (self.started.attributes.clone(), None, None),
        };
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
        recorder.record(TraceSignal::ToolCallStarted(started.clone()));
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

    pub fn record_input_shape(&mut self, input: &serde_json::Value) {
        self.attributes.input_top_level_key_count =
            input.as_object().map(|object| saturating_u64(object.len()));
    }

    pub fn record_validation_ms(&mut self, duration_ms: u64) {
        self.attributes.validation_ms = Some(duration_ms);
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

    pub fn record_permission_wait_ms(&mut self, duration_ms: i64) {
        self.permission_wait_ms = Some(duration_ms.max(0));
    }

    pub fn record_execution_ms(&mut self, duration_ms: u64) {
        self.attributes.execution_ms = Some(duration_ms);
    }

    pub fn record_progress_event(&mut self) {
        self.attributes.progress_event_count = Some(
            self.attributes
                .progress_event_count
                .unwrap_or_default()
                .saturating_add(1),
        );
    }

    pub fn finish_result(
        mut self,
        result: &ToolResult,
        result_persisted: bool,
        result_persist_ms: u64,
        result_persist_error_code: Option<&str>,
    ) {
        let output = result.text_content();
        self.attributes.output_bytes = Some(saturating_u64(output.len()));
        self.attributes.output_lines = Some(saturating_u64(output.lines().count()));
        self.attributes.artifact_count = Some(saturating_u64(result.artifacts.len()));
        self.attributes.error_retryable = result.error.as_ref().map(|error| error.retryable);
        self.attributes.result_persisted = Some(result_persisted);
        self.attributes.result_persist_ms = Some(result_persist_ms);
        self.attributes.result_persist_error_code =
            bounded_option(result_persist_error_code, MAX_TRACE_STRING_CHARS);
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
        );
    }

    fn finish(
        &mut self,
        status: TraceStatus,
        error_code: Option<String>,
        error_message: Option<String>,
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
        self.finish(status, Some(code.to_string()), Some(message.to_string()));
    }
}

#[derive(Debug, Clone)]
pub enum TraceSignal {
    ModelCallStarted(ModelCallStarted),
    ModelCallFinished(Box<ModelCallFinished>),
    ToolCallStarted(ToolCallStarted),
    ToolCallFinished(Box<ToolCallFinished>),
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

fn serialized_bytes<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut counter = ByteCounter::default();
    serde_json::to_writer(&mut counter, value)
        .map(|()| counter.bytes)
        .unwrap_or_default()
}

#[derive(Default)]
struct ByteCounter {
    bytes: u64,
}

impl Write for ByteCounter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(saturating_u64(buffer.len()));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
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
        ContentBlock, FinishReason, Message, ModelError, ModelRequest, ModelResponse,
        ModelTransportSignal, ModelTransportSignalKind, Role, ToolCallBlock, ToolCallState,
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
    fn model_guard_records_real_attempts_and_content_free_shapes() {
        let request = ModelRequest {
            model: "test-model".to_string(),
            messages: vec![
                Message::text(Role::System, "secret system prompt"),
                Message::text(Role::User, "secret user prompt"),
            ],
            temperature: None,
            max_output_tokens: Some(4096),
            thinking: None,
            tools: vec![openwork_models::model::ToolDefinition {
                name: "read".to_string(),
                description: "secret tool description".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        };
        let attributes = ModelTraceAttributesV1::from_request(2, 7, &request);
        let recorder = Arc::new(RecordingTrace::default());
        let recorder_port: Arc<dyn TraceRecorder> = recorder.clone();
        let mut guard = ModelCallTraceGuard::start(
            recorder_port,
            ModelCallStarted {
                span_id: "model-1".to_string(),
                turn_id: TurnId::new("turn-1"),
                sequence: 1,
                model_id: None,
                resolved_model_name: "test-model".to_string(),
                started_at: OffsetDateTime::now_utc(),
                attributes,
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
        assert_eq!(finished.attributes.attempts.len(), 2);
        assert_eq!(finished.attributes.attempts[0].status, "failed");
        assert_eq!(finished.attributes.attempts[0].retry_delay_ms, Some(25));
        assert_eq!(finished.attributes.attempts[1].status, "succeeded");
        assert_eq!(
            finished.attributes.finish_reason.as_deref(),
            Some("tool_use")
        );
        assert_eq!(finished.attributes.response_text_bytes, Some(15));
        assert_eq!(finished.attributes.response_tool_call_count, Some(1));
        assert!(finished.attributes.ttft_ms.is_some());

        let encoded = serde_json::to_string(&finished.attributes).expect("attributes serialize");
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
                turn_id: TurnId::new("turn-stream-error"),
                sequence: 1,
                model_id: None,
                resolved_model_name: "test-model".to_string(),
                started_at: OffsetDateTime::now_utc(),
                attributes,
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
        let error = ModelError::protocol("invalid stream event");
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
    }
}
