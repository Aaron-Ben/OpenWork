use std::{collections::HashMap, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use openwork_protocol::{
    approval::{ApprovalRequested, ApprovalResolved},
    capability::{Observation, ObservationStatus},
    domain::EventId,
    journal::{
        AggregateType, EventJournal, EventJournalError, ExpectedVersion, NewRecordedEventV1,
        RecordedEventV1,
    },
    model::Role,
    turn::{
        AssistantMessageRecorded, StepCompleted, StepFailed, StepStarted, ToolMessageRecorded,
        ToolRunFinished, ToolRunRequested, ToolRunStarted, TurnRecordError, TurnRecordedEvent,
        TurnRecorderPort,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::store::SessionError;

const TURN_STARTED: &str = "turn_started";
const TURN_COMPLETED: &str = "turn_completed";
const TURN_CANCELLED: &str = "turn_cancelled";
const TURN_DOOM_LOOP: &str = "turn_doom_loop_detected";
const TURN_FAILED: &str = "turn_failed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnLifecycleStatus {
    Running,
    WaitingApproval,
    OutcomeUnknown,
    Interrupted,
    Completed,
    Cancelled,
    DoomLoop,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepLifecycleStatus {
    Running,
    WaitingApproval,
    OutcomeUnknown,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRunLifecycleStatus {
    Requested,
    WaitingApproval,
    Running,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalSnapshot {
    pub approval_id: String,
    pub turn_id: String,
    pub step_id: String,
    pub step_index: usize,
    pub tool_run_id: String,
    pub provider_tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunLifecycleSnapshot {
    pub id: String,
    pub provider_tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub status: ToolRunLifecycleStatus,
    pub observation: Option<Observation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepLifecycleSnapshot {
    pub id: String,
    pub index: usize,
    pub status: StepLifecycleStatus,
    pub tool_runs: Vec<ToolRunLifecycleSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnLifecycleSnapshot {
    pub id: String,
    pub session_id: String,
    pub provider_id: String,
    pub model: String,
    pub status: TurnLifecycleStatus,
    pub steps: Vec<StepLifecycleSnapshot>,
    pub pending_approval: Option<PendingApprovalSnapshot>,
    pub started_at: i64,
    pub updated_at: i64,
}

/// A recorder bound to one existing Turn and its owning Session.
#[derive(Clone)]
pub struct JournalTurnRecorder {
    journal: Arc<dyn EventJournal>,
    turn_id: String,
    session_id: String,
}

impl JournalTurnRecorder {
    pub(crate) fn new(
        journal: Arc<dyn EventJournal>,
        turn_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            journal,
            turn_id: turn_id.into(),
            session_id: session_id.into(),
        }
    }
}

#[async_trait]
impl TurnRecorderPort for JournalTurnRecorder {
    async fn append(&self, events: Vec<TurnRecordedEvent>) -> Result<(), TurnRecordError> {
        if events.is_empty() {
            return Ok(());
        }
        let existing = self
            .journal
            .load_aggregate(AggregateType::Turn, &self.turn_id, 0)
            .await
            .map_err(map_record_error)?;
        let Some(last) = existing.last() else {
            return Err(TurnRecordError::InvalidEvent {
                message: format!("turn {} has not started", self.turn_id),
            });
        };
        let started = existing
            .iter()
            .find(|event| event.event_type == TURN_STARTED)
            .ok_or_else(|| TurnRecordError::InvalidEvent {
                message: format!("turn {} has no turn_started event", self.turn_id),
            })?;
        let started: TurnStartedPayload =
            serde_json::from_value(started.payload.clone()).map_err(|error| {
                TurnRecordError::InvalidEvent {
                    message: error.to_string(),
                }
            })?;
        if started.session_id != self.session_id {
            return Err(TurnRecordError::InvalidEvent {
                message: format!(
                    "turn {} belongs to session {}, not {}",
                    self.turn_id, started.session_id, self.session_id
                ),
            });
        }
        if existing.iter().any(|event| is_terminal(&event.event_type)) {
            return Err(TurnRecordError::Conflict {
                message: format!("turn {} is already terminal", self.turn_id),
            });
        }

        let now_ms = now_unix_ms();
        let records = events
            .into_iter()
            .map(|event| new_recorded_event(event, &self.session_id, now_ms))
            .collect::<Result<Vec<_>, _>>()?;
        self.journal
            .append(
                AggregateType::Turn,
                &self.turn_id,
                ExpectedVersion::Exact(last.aggregate_version),
                records,
            )
            .await
            .map_err(map_record_error)?;
        Ok(())
    }
}

/// Rebuilds the recovery-oriented state of one Turn from its ordered facts.
pub fn replay_turn_lifecycle(
    events: &[RecordedEventV1],
) -> Result<Option<TurnLifecycleSnapshot>, SessionError> {
    let Some(started_event) = events.iter().find(|event| {
        event.aggregate_type == AggregateType::Turn && event.event_type == TURN_STARTED
    }) else {
        return Ok(None);
    };
    let started: TurnStartedPayload = serde_json::from_value(started_event.payload.clone())?;
    let turn_id = started_event.aggregate_id.clone();
    let mut snapshot = TurnLifecycleSnapshot {
        id: turn_id.clone(),
        session_id: started.session_id,
        provider_id: started.provider_id,
        model: started.model,
        status: TurnLifecycleStatus::Running,
        steps: Vec::new(),
        pending_approval: None,
        started_at: started_event.occurred_at_unix_ms / 1_000,
        updated_at: started_event.occurred_at_unix_ms / 1_000,
    };

    for event in events.iter().filter(|event| {
        event.aggregate_type == AggregateType::Turn && event.aggregate_id == turn_id
    }) {
        snapshot.updated_at = snapshot.updated_at.max(event.occurred_at_unix_ms / 1_000);
        match event.event_type.as_str() {
            "step_started" => {
                let payload: StepStarted = serde_json::from_value(event.payload.clone())?;
                if snapshot
                    .steps
                    .iter()
                    .all(|step| step.id != payload.step_id.as_str())
                {
                    snapshot.steps.push(StepLifecycleSnapshot {
                        id: payload.step_id.to_string(),
                        index: payload.step_index,
                        status: StepLifecycleStatus::Running,
                        tool_runs: Vec::new(),
                    });
                }
            }
            "tool_run_requested" => {
                let payload: ToolRunRequested = serde_json::from_value(event.payload.clone())?;
                let step = find_step_mut(&mut snapshot, payload.step_id.as_str())?;
                if step
                    .tool_runs
                    .iter()
                    .all(|run| run.id != payload.tool_run_id.as_str())
                {
                    step.tool_runs.push(ToolRunLifecycleSnapshot {
                        id: payload.tool_run_id.to_string(),
                        provider_tool_call_id: payload.provider_tool_call_id,
                        tool_name: payload.tool_name,
                        input: payload.input,
                        status: ToolRunLifecycleStatus::Requested,
                        observation: None,
                    });
                }
            }
            "approval_requested" => {
                let payload: ApprovalRequested = serde_json::from_value(event.payload.clone())?;
                let step = find_step_mut(&mut snapshot, payload.step_id.as_str())?;
                let step_index = step.index;
                let run = find_tool_run_mut(step, payload.tool_run_id.as_str())?;
                run.status = ToolRunLifecycleStatus::WaitingApproval;
                snapshot.pending_approval = Some(PendingApprovalSnapshot {
                    approval_id: payload.approval_id.to_string(),
                    turn_id: payload.turn_id.to_string(),
                    step_id: payload.step_id.to_string(),
                    step_index,
                    tool_run_id: payload.tool_run_id.to_string(),
                    provider_tool_call_id: run.provider_tool_call_id.clone(),
                    tool_name: payload.tool_name,
                    input: payload.input,
                    reason: payload.reason,
                });
            }
            "approval_resolved" => {
                let payload: ApprovalResolved = serde_json::from_value(event.payload.clone())?;
                if snapshot
                    .pending_approval
                    .as_ref()
                    .is_some_and(|pending| pending.approval_id == payload.approval_id.as_str())
                {
                    snapshot.pending_approval = None;
                }
                let step = find_step_mut(&mut snapshot, payload.step_id.as_str())?;
                let run = find_tool_run_mut(step, payload.tool_run_id.as_str())?;
                run.status = ToolRunLifecycleStatus::Requested;
            }
            "tool_run_started" => {
                let payload: ToolRunStarted = serde_json::from_value(event.payload.clone())?;
                let step = find_step_mut(&mut snapshot, payload.step_id.as_str())?;
                find_tool_run_mut(step, payload.tool_run_id.as_str())?.status =
                    ToolRunLifecycleStatus::Running;
            }
            "tool_run_completed"
            | "tool_run_failed"
            | "tool_run_denied"
            | "tool_run_cancelled"
            | "tool_run_outcome_unknown" => {
                let payload: ToolRunFinished = serde_json::from_value(event.payload.clone())?;
                let tool_run_id = payload.tool_run_id.to_string();
                {
                    let step = find_step_mut(&mut snapshot, payload.step_id.as_str())?;
                    let run = find_tool_run_mut(step, &tool_run_id)?;
                    run.status = tool_status(payload.observation.status);
                    run.observation = Some(payload.observation);
                }
                if snapshot
                    .pending_approval
                    .as_ref()
                    .is_some_and(|pending| pending.tool_run_id == tool_run_id)
                {
                    snapshot.pending_approval = None;
                }
            }
            "step_completed" => {
                let payload: StepCompleted = serde_json::from_value(event.payload.clone())?;
                find_step_mut(&mut snapshot, payload.step_id.as_str())?.status =
                    StepLifecycleStatus::Completed;
            }
            "step_failed" => {
                let payload: StepFailed = serde_json::from_value(event.payload.clone())?;
                find_step_mut(&mut snapshot, payload.step_id.as_str())?.status =
                    StepLifecycleStatus::Failed;
            }
            TURN_COMPLETED => snapshot.status = TurnLifecycleStatus::Completed,
            TURN_CANCELLED => snapshot.status = TurnLifecycleStatus::Cancelled,
            TURN_DOOM_LOOP => snapshot.status = TurnLifecycleStatus::DoomLoop,
            TURN_FAILED => snapshot.status = TurnLifecycleStatus::Failed,
            _ => {}
        }
    }

    // A Turn terminal fact cannot prove that a started external effect reached
    // its terminal outcome. Keep this check independent from Turn status so a
    // later `turn_failed` never makes the ToolRun look safe to retry.
    let mut has_unknown = false;
    for step in &mut snapshot.steps {
        for run in &mut step.tool_runs {
            if run.status == ToolRunLifecycleStatus::Running {
                run.status = ToolRunLifecycleStatus::OutcomeUnknown;
                has_unknown = true;
            }
        }
        if step
            .tool_runs
            .iter()
            .any(|run| run.status == ToolRunLifecycleStatus::OutcomeUnknown)
        {
            step.status = StepLifecycleStatus::OutcomeUnknown;
        }
    }

    if has_unknown {
        snapshot.status = TurnLifecycleStatus::OutcomeUnknown;
        snapshot.pending_approval = None;
    } else if is_turn_terminal(snapshot.status) {
        snapshot.pending_approval = None;
    } else if snapshot.pending_approval.is_some() {
        snapshot.status = TurnLifecycleStatus::WaitingApproval;
        if let Some(step) = snapshot
            .steps
            .iter_mut()
            .find(|step| step.status == StepLifecycleStatus::Running)
        {
            step.status = StepLifecycleStatus::WaitingApproval;
        }
    } else if snapshot
        .steps
        .iter()
        .any(|step| step.status == StepLifecycleStatus::Failed)
    {
        snapshot.status = TurnLifecycleStatus::Failed;
    } else if !snapshot.steps.is_empty()
        && snapshot
            .steps
            .iter()
            .any(|step| step.status != StepLifecycleStatus::Completed)
    {
        snapshot.status = TurnLifecycleStatus::Interrupted;
    }

    Ok(Some(snapshot))
}

pub(crate) fn replay_session_turns(
    events: &[RecordedEventV1],
    session_id: &str,
) -> Result<Vec<TurnLifecycleSnapshot>, SessionError> {
    let mut by_turn = HashMap::<String, Vec<RecordedEventV1>>::new();
    for event in events
        .iter()
        .filter(|event| event.aggregate_type == AggregateType::Turn)
    {
        by_turn
            .entry(event.aggregate_id.clone())
            .or_default()
            .push(event.clone());
    }
    let mut turns = by_turn
        .into_values()
        .filter_map(|mut turn_events| {
            turn_events.sort_by_key(|event| event.aggregate_version);
            replay_turn_lifecycle(&turn_events).transpose()
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|turn| turn.session_id == session_id)
        .collect::<Vec<_>>();
    turns.sort_by_key(|turn| turn.started_at);
    Ok(turns)
}

fn new_recorded_event(
    event: TurnRecordedEvent,
    session_id: &str,
    occurred_at_unix_ms: i64,
) -> Result<NewRecordedEventV1, TurnRecordError> {
    let event_type = event.event_type();
    let payload = match event {
        TurnRecordedEvent::StepStarted(payload) => encode(&payload)?,
        TurnRecordedEvent::AssistantMessageRecorded(payload) => {
            encode(&MessageRecordedPayload::assistant(session_id, payload))?
        }
        TurnRecordedEvent::ToolRunRequested(payload) => encode(&payload)?,
        TurnRecordedEvent::ApprovalRequested(payload) => encode(&payload)?,
        TurnRecordedEvent::ApprovalResolved(payload) => encode(&payload)?,
        TurnRecordedEvent::ToolRunStarted(payload) => encode(&payload)?,
        TurnRecordedEvent::ToolRunFinished(payload) => encode(&payload)?,
        TurnRecordedEvent::ToolMessageRecorded(payload) => {
            encode(&MessageRecordedPayload::tool(session_id, payload))?
        }
        TurnRecordedEvent::StepCompleted(payload) => encode(&payload)?,
        TurnRecordedEvent::StepFailed(payload) => encode(&payload)?,
    };
    Ok(NewRecordedEventV1::new(
        EventId::new(format!("evt-{}", Uuid::new_v4().simple())),
        event_type,
        payload,
        occurred_at_unix_ms,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnStartedPayload {
    session_id: String,
    provider_id: String,
    model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageRecordedPayload {
    message_id: String,
    session_id: String,
    role: Role,
    parts: Vec<openwork_protocol::model::ContentBlock>,
    step_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_run_id: Option<String>,
}

impl MessageRecordedPayload {
    fn assistant(session_id: &str, payload: AssistantMessageRecorded) -> Self {
        Self {
            message_id: payload.message_id,
            session_id: session_id.to_string(),
            role: Role::Assistant,
            parts: payload.parts,
            step_id: payload.step_id.to_string(),
            tool_run_id: None,
        }
    }

    fn tool(session_id: &str, payload: ToolMessageRecorded) -> Self {
        Self {
            message_id: payload.message_id,
            session_id: session_id.to_string(),
            role: Role::Tool,
            parts: payload.parts,
            step_id: payload.step_id.to_string(),
            tool_run_id: Some(payload.tool_run_id.to_string()),
        }
    }
}

fn find_step_mut<'a>(
    snapshot: &'a mut TurnLifecycleSnapshot,
    step_id: &str,
) -> Result<&'a mut StepLifecycleSnapshot, SessionError> {
    snapshot
        .steps
        .iter_mut()
        .find(|step| step.id == step_id)
        .ok_or_else(|| SessionError::InvalidEvent {
            message: format!("lifecycle event references unknown step {step_id}"),
        })
}

fn find_tool_run_mut<'a>(
    step: &'a mut StepLifecycleSnapshot,
    tool_run_id: &str,
) -> Result<&'a mut ToolRunLifecycleSnapshot, SessionError> {
    step.tool_runs
        .iter_mut()
        .find(|run| run.id == tool_run_id)
        .ok_or_else(|| SessionError::InvalidEvent {
            message: format!("lifecycle event references unknown tool run {tool_run_id}"),
        })
}

fn tool_status(status: ObservationStatus) -> ToolRunLifecycleStatus {
    match status {
        ObservationStatus::Succeeded => ToolRunLifecycleStatus::Succeeded,
        ObservationStatus::Failed => ToolRunLifecycleStatus::Failed,
        ObservationStatus::Denied => ToolRunLifecycleStatus::Denied,
        ObservationStatus::Cancelled => ToolRunLifecycleStatus::Cancelled,
        ObservationStatus::OutcomeUnknown => ToolRunLifecycleStatus::OutcomeUnknown,
    }
}

fn is_turn_terminal(status: TurnLifecycleStatus) -> bool {
    matches!(
        status,
        TurnLifecycleStatus::Completed
            | TurnLifecycleStatus::Cancelled
            | TurnLifecycleStatus::DoomLoop
            | TurnLifecycleStatus::Failed
    )
}

fn is_terminal(event_type: &str) -> bool {
    matches!(
        event_type,
        TURN_COMPLETED | TURN_CANCELLED | TURN_DOOM_LOOP | TURN_FAILED
    )
}

fn encode(payload: &impl Serialize) -> Result<Value, TurnRecordError> {
    serde_json::to_value(payload).map_err(|error| TurnRecordError::InvalidEvent {
        message: error.to_string(),
    })
}

fn map_record_error(error: EventJournalError) -> TurnRecordError {
    match error {
        EventJournalError::VersionConflict { .. } | EventJournalError::DuplicateEvent { .. } => {
            TurnRecordError::Conflict {
                message: error.to_string(),
            }
        }
        EventJournalError::InvalidEvent { message } => TurnRecordError::InvalidEvent { message },
        EventJournalError::Persistence { message } => TurnRecordError::Unavailable { message },
    }
}

fn now_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use openwork_protocol::{
        domain::{StepId, ToolRunId},
        model::ContentBlock,
        turn::{ToolMessageRecorded, TurnRecordedEvent},
    };

    use super::new_recorded_event;

    #[test]
    fn tool_message_payload_keeps_its_tool_run_identity() {
        let recorded = new_recorded_event(
            TurnRecordedEvent::ToolMessageRecorded(ToolMessageRecorded {
                message_id: "message-1".to_string(),
                step_id: StepId::new("step-1"),
                tool_run_id: ToolRunId::new("tool-run-1"),
                parts: vec![ContentBlock::text("done")],
            }),
            "session-1",
            1,
        )
        .expect("tool message should serialize");

        assert_eq!(recorded.payload["toolRunId"], "tool-run-1");
    }
}
