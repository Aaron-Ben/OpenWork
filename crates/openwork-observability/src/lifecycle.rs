use std::{sync::Arc, time::SystemTime};

use async_trait::async_trait;
use openwork_protocol::{
    approval::ApprovalResolution,
    capability::ObservationStatus,
    trace::{
        TraceRecorderPort, TraceSignal, TraceSpanKind, TraceSpanStart, TraceSpanStatus,
        TraceSpanUpdate,
    },
    turn::{TurnRecordError, TurnRecordedEvent, TurnRecorderPort},
};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct TraceContext {
    pub trace_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub provider_id: String,
    pub model: String,
}

/// Decorates the durable Turn recorder. Trace signals are emitted only after
/// the corresponding Recorded Events have been appended successfully.
pub struct TracingTurnRecorder {
    inner: Arc<dyn TurnRecorderPort>,
    trace: Arc<dyn TraceRecorderPort>,
    context: TraceContext,
}

impl TracingTurnRecorder {
    pub fn new(
        inner: Arc<dyn TurnRecorderPort>,
        trace: Arc<dyn TraceRecorderPort>,
        context: TraceContext,
    ) -> Self {
        Self {
            inner,
            trace,
            context,
        }
    }
}

#[async_trait]
impl TurnRecorderPort for TracingTurnRecorder {
    async fn append(&self, events: Vec<TurnRecordedEvent>) -> Result<(), TurnRecordError> {
        let trace_events = events.clone();
        let started_at = now_unix_ms();
        self.inner.append(events).await?;
        let finished_at = now_unix_ms();
        for event in trace_events {
            self.record_event(event, started_at, finished_at);
        }
        Ok(())
    }
}

impl TracingTurnRecorder {
    fn record_event(&self, event: TurnRecordedEvent, started_at: i64, finished_at: i64) {
        match event {
            TurnRecordedEvent::StepStarted(event) => self.start(
                event.step_id.as_str(),
                Some(self.context.turn_id.clone()),
                TraceSpanKind::Step,
                "step.run",
                Some(event.step_id.to_string()),
                None,
                TraceSpanStatus::Running,
                started_at,
                json!({"stepIndex": event.step_index}),
            ),
            TurnRecordedEvent::StepCompleted(event) => self.finish(
                event.step_id.as_str(),
                TraceSpanStatus::Succeeded,
                finished_at,
                json!({"stepIndex": event.step_index}),
                None,
            ),
            TurnRecordedEvent::StepFailed(event) => self.finish(
                event.step_id.as_str(),
                TraceSpanStatus::Failed,
                finished_at,
                json!({"stepIndex": event.step_index}),
                Some(("step_failed", None, event.message)),
            ),
            TurnRecordedEvent::ToolRunRequested(event) => self.start(
                event.tool_run_id.as_str(),
                Some(event.step_id.to_string()),
                TraceSpanKind::ToolRun,
                "tool.run",
                Some(event.step_id.to_string()),
                Some(event.tool_run_id.to_string()),
                TraceSpanStatus::Running,
                started_at,
                json!({
                    "toolName": event.tool_name,
                    "providerToolCallId": event.provider_tool_call_id,
                    "requestedAt": started_at
                }),
            ),
            TurnRecordedEvent::ToolRunStarted(event) => self.update(
                event.tool_run_id.as_str(),
                TraceSpanStatus::Running,
                finished_at,
                false,
                json!({"executionStartedAt": finished_at}),
                None,
            ),
            TurnRecordedEvent::ToolRunFinished(event) => {
                let status = observation_status(event.observation.status);
                let error = event.observation.error.map(|error| {
                    (
                        "tool_error",
                        Some(format!("{:?}", error.code).to_ascii_lowercase()),
                        error.message,
                    )
                });
                self.finish(
                    event.tool_run_id.as_str(),
                    status,
                    finished_at,
                    json!({"observationStatus": event.observation.status}),
                    error,
                );
            }
            TurnRecordedEvent::ApprovalRequested(event) => {
                self.update(
                    &self.context.turn_id,
                    TraceSpanStatus::Waiting,
                    finished_at,
                    false,
                    json!({"waitingForApproval": true}),
                    None,
                );
                self.update(
                    event.tool_run_id.as_str(),
                    TraceSpanStatus::Waiting,
                    finished_at,
                    false,
                    json!({"approvalRequired": true}),
                    None,
                );
                self.start(
                    event.approval_id.as_str(),
                    Some(event.tool_run_id.to_string()),
                    TraceSpanKind::Approval,
                    "approval.wait",
                    Some(event.step_id.to_string()),
                    Some(event.tool_run_id.to_string()),
                    TraceSpanStatus::Waiting,
                    started_at,
                    json!({"reason": event.reason, "toolName": event.tool_name}),
                );
            }
            TurnRecordedEvent::ApprovalResolved(event) => {
                let (status, resolution) = match event.resolution {
                    ApprovalResolution::Allow => (TraceSpanStatus::Succeeded, "allow"),
                    ApprovalResolution::Deny { .. } => (TraceSpanStatus::Denied, "deny"),
                };
                self.finish(
                    event.approval_id.as_str(),
                    status,
                    finished_at,
                    json!({"resolution": resolution}),
                    None,
                );
                self.update(
                    event.tool_run_id.as_str(),
                    if status == TraceSpanStatus::Denied {
                        TraceSpanStatus::Denied
                    } else {
                        TraceSpanStatus::Running
                    },
                    finished_at,
                    false,
                    json!({}),
                    None,
                );
                self.update(
                    &self.context.turn_id,
                    TraceSpanStatus::Running,
                    finished_at,
                    false,
                    json!({"waitingForApproval": false}),
                    None,
                );
            }
            TurnRecordedEvent::AssistantMessageRecorded(_)
            | TurnRecordedEvent::ToolMessageRecorded(_) => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        span_id: &str,
        parent_span_id: Option<String>,
        span_kind: TraceSpanKind,
        span_name: &str,
        step_id: Option<String>,
        tool_run_id: Option<String>,
        status: TraceSpanStatus,
        started_at_unix_ms: i64,
        attributes: Value,
    ) {
        self.trace.record(TraceSignal::Start(TraceSpanStart {
            trace_id: self.context.trace_id.clone(),
            span_id: span_id.to_string(),
            parent_span_id,
            span_kind,
            span_name: span_name.to_string(),
            status,
            session_id: self.context.session_id.clone(),
            turn_id: self.context.turn_id.clone(),
            step_id,
            tool_run_id,
            started_at_unix_ms,
            attributes,
        }));
    }

    fn finish(
        &self,
        span_id: &str,
        status: TraceSpanStatus,
        occurred_at_unix_ms: i64,
        attributes: Value,
        error: Option<(&str, Option<String>, String)>,
    ) {
        self.update(
            span_id,
            status,
            occurred_at_unix_ms,
            true,
            attributes,
            error,
        );
    }

    fn update(
        &self,
        span_id: &str,
        status: TraceSpanStatus,
        occurred_at_unix_ms: i64,
        ended: bool,
        attributes: Value,
        error: Option<(&str, Option<String>, String)>,
    ) {
        let (error_type, error_code, error_message) = error
            .map(|(kind, code, message)| (Some(kind.to_string()), code, Some(message)))
            .unwrap_or((None, None, None));
        self.trace.record(TraceSignal::Update(TraceSpanUpdate {
            span_id: span_id.to_string(),
            status,
            occurred_at_unix_ms,
            ended,
            attributes,
            error_type,
            error_code,
            error_message,
        }));
    }
}

fn observation_status(status: ObservationStatus) -> TraceSpanStatus {
    match status {
        ObservationStatus::Succeeded => TraceSpanStatus::Succeeded,
        ObservationStatus::Failed => TraceSpanStatus::Failed,
        ObservationStatus::Denied => TraceSpanStatus::Denied,
        ObservationStatus::Cancelled => TraceSpanStatus::Cancelled,
        ObservationStatus::OutcomeUnknown => TraceSpanStatus::OutcomeUnknown,
    }
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}
