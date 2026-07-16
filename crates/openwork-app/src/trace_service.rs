use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::SystemTime,
};

use openwork_persistence::{SessionMessage, SessionStore, SessionSummary};
use openwork_protocol::{
    capability::Observation,
    model::{ContentBlock, Role},
    trace::{TraceRepository, TraceRootFilter, TraceSpan, TraceSpanKind, TraceSpanStatus},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ApplicationError, ApplicationErrorCode};

const INPUT_PREVIEW_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDiagnosisStatus {
    Healthy,
    Attention,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDiagnosisReason {
    Healthy,
    WaitingApproval,
    ModelError,
    TransportExhausted,
    ToolError,
    ApprovalDenied,
    OutcomeUnknown,
    Cancelled,
    Recovered,
    PartialTrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDataCompleteness {
    Complete,
    Partial,
    Legacy,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceDiagnosis {
    pub status: TraceDiagnosisStatus,
    pub reason: TraceDiagnosisReason,
    pub focus_span_id: Option<String>,
    pub evidence_span_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTraceSummary {
    pub trace_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub session_title: Option<String>,
    pub working_dir: Option<String>,
    pub input_preview: Option<String>,
    pub status: TraceSpanStatus,
    pub model: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: i64,
    pub step_count: usize,
    pub model_attempt_count: usize,
    pub transport_attempt_count: usize,
    pub tool_run_count: usize,
    pub approval_count: usize,
    pub retry_count: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub error_count: usize,
    pub recovered: bool,
    pub diagnosis: TraceDiagnosis,
    pub data_completeness: TraceDataCompleteness,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceListPage {
    pub items: Vec<TurnTraceSummary>,
    pub next_offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TraceListQuery {
    pub limit: u32,
    pub offset: u32,
    pub query: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub status: Option<TraceSpanStatus>,
    pub started_after: Option<i64>,
    pub started_before: Option<i64>,
    pub has_error: Option<bool>,
    pub has_retry: Option<bool>,
}

impl Default for TraceListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
            query: None,
            session_id: None,
            project: None,
            model: None,
            status: None,
            started_after: None,
            started_before: None,
            has_error: None,
            has_retry: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTrace {
    pub summary: TurnTraceSummary,
    pub spans: Vec<TraceSpanView>,
}

/// Host-facing view adds the derived duration without changing the protocol
/// persistence model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpanView {
    #[serde(flatten)]
    pub span: TraceSpan,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpanDetailView {
    pub span: TraceSpanView,
    pub detail: TraceSpanDetail,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum TraceSpanDetail {
    Turn(TraceTurnDetail),
    Step(TraceStepDetail),
    ModelAttempt(TraceModelAttemptDetail),
    TransportAttempt(TraceTransportAttemptDetail),
    ToolRun(TraceToolRunDetail),
    Approval(TraceApprovalDetail),
    Recovery(TraceRecoveryDetail),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceTurnDetail {
    pub trace_schema_version: Option<String>,
    pub instrumentation_version: Option<String>,
    pub app_version: Option<String>,
    pub capture_mode: Option<String>,
    pub outcome: Option<String>,
    pub diagnosis: TraceDiagnosis,
    pub data_completeness: TraceDataCompleteness,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStepDetail {
    pub step_index: Option<u64>,
    pub tool_count: usize,
    pub messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceModelRequestSummary {
    pub version: Option<String>,
    pub message_count: Option<u64>,
    pub message_text_chars: Option<u64>,
    pub system_prompt_chars: Option<u64>,
    pub tool_definition_count: Option<u64>,
    pub tool_names: Vec<String>,
    pub temperature: Option<f64>,
    pub max_output_tokens: Option<u64>,
    pub thinking_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceModelAttemptDetail {
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub finish_reason: Option<String>,
    pub raw_finish_reason: Option<String>,
    pub response_id: Option<String>,
    pub provider_request_id: Option<String>,
    /// Time until Core observed the first semantic Model event; not network TTFT.
    pub first_output_ms: Option<i64>,
    pub usage: TraceTokenUsage,
    /// Content-free metadata captured at the actual provider request boundary.
    pub request_summary: TraceModelRequestSummary,
    pub messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceTransportAttemptDetail {
    pub provider_id: Option<String>,
    pub transport_attempt: Option<u64>,
    pub http_status: Option<u64>,
    pub provider_code: Option<String>,
    pub provider_request_id: Option<String>,
    pub retry_delay_ms: Option<i64>,
    pub will_retry: Option<bool>,
    pub failure_phase: Option<String>,
    pub delivery_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolRunDetail {
    pub tool_name: Option<String>,
    pub provider_tool_call_id: Option<String>,
    pub input: Option<Value>,
    pub observation: Option<Observation>,
    pub approval_required: bool,
    pub requested_at: Option<i64>,
    pub execution_started_at: Option<i64>,
    pub request_to_end_ms: Option<i64>,
    pub approval_wait_ms: Option<i64>,
    pub execution_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceApprovalDetail {
    pub reason: Option<String>,
    pub tool_name: Option<String>,
    pub resolution: Option<String>,
    pub wait_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceRecoveryDetail {
    pub reason: Option<String>,
    pub approval_id: Option<String>,
    pub step_index: Option<u64>,
}

pub struct TraceApplicationService {
    repository: Arc<dyn TraceRepository>,
    session_store: Option<SessionStore>,
}

impl TraceApplicationService {
    pub fn new(repository: Arc<dyn TraceRepository>) -> Self {
        Self {
            repository,
            session_store: None,
        }
    }

    pub fn with_session_store(
        repository: Arc<dyn TraceRepository>,
        session_store: SessionStore,
    ) -> Self {
        Self {
            repository,
            session_store: Some(session_store),
        }
    }

    pub async fn load_turn(&self, turn_id: &str) -> Result<TurnTrace, ApplicationError> {
        let spans = self.repository.load_turn(turn_id).await?;
        if spans.is_empty() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::TurnNotFound,
                format!("Trace turn not found: {turn_id}"),
            ));
        }
        let mut summary = summarize(turn_id, &spans);
        self.decorate_turn_summary(&mut summary).await?;
        let mut views = spans.into_iter().map(span_view).collect::<Vec<_>>();
        views.sort_by(|left, right| {
            left.span
                .started_at_unix_ms
                .cmp(&right.span.started_at_unix_ms)
                .then_with(|| left.span.span_id.cmp(&right.span.span_id))
        });
        Ok(TurnTrace {
            summary,
            spans: views,
        })
    }

    pub async fn load_span_detail(
        &self,
        turn_id: &str,
        span_id: &str,
    ) -> Result<TraceSpanDetailView, ApplicationError> {
        let spans = self.repository.load_turn(turn_id).await?;
        if spans.is_empty() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::TurnNotFound,
                format!("Trace turn not found: {turn_id}"),
            ));
        }
        let span = spans
            .iter()
            .find(|span| span.span_id == span_id)
            .cloned()
            .ok_or_else(|| {
                ApplicationError::new(
                    ApplicationErrorCode::InvalidRequest,
                    format!("Trace span {span_id} does not belong to turn {turn_id}"),
                )
            })?;
        let messages = if matches!(
            span.span_kind,
            TraceSpanKind::Step | TraceSpanKind::ModelAttempt
        ) {
            match &self.session_store {
                Some(store) => store.load_turn_messages(turn_id).await?,
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let lifecycle = if span.span_kind == TraceSpanKind::ToolRun {
            match &self.session_store {
                Some(store) => store.load_turn_lifecycle(turn_id).await?,
                None => None,
            }
        } else {
            None
        };
        let detail = build_detail(&span, &spans, &messages, lifecycle.as_ref());
        Ok(TraceSpanDetailView {
            span: span_view(span),
            detail,
        })
    }

    pub async fn list_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<TurnTraceSummary>, ApplicationError> {
        let mut summaries = summarize_by_turn(self.repository.load_session(session_id).await?);
        let context = self.load_context().await?;
        for summary in &mut summaries {
            decorate_summary(summary, &context);
        }
        Ok(summaries)
    }

    pub async fn list_recent(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<TraceListPage, ApplicationError> {
        self.query_recent(TraceListQuery {
            limit,
            offset,
            ..TraceListQuery::default()
        })
        .await
    }

    pub async fn query_recent(
        &self,
        query: TraceListQuery,
    ) -> Result<TraceListPage, ApplicationError> {
        let limit = query.limit.clamp(1, 100);
        let requested = limit.saturating_add(1);
        let context = self.load_context().await?;
        let filter = build_filter(&query, &context);
        let mut summaries = summarize_by_turn(
            self.repository
                .load_recent_turns_filtered(&filter, requested, query.offset)
                .await?,
        );
        let has_more = summaries.len() > limit as usize;
        summaries.truncate(limit as usize);
        for summary in &mut summaries {
            decorate_summary(summary, &context);
        }
        Ok(TraceListPage {
            items: summaries,
            next_offset: has_more.then(|| query.offset.saturating_add(limit)),
        })
    }

    async fn load_context(&self) -> Result<TraceContextIndex, ApplicationError> {
        let Some(store) = &self.session_store else {
            return Ok(TraceContextIndex::default());
        };
        let sessions = store.list_sessions().await?;
        let messages = store.load_all_messages().await?;
        Ok(TraceContextIndex::new(sessions, messages))
    }

    async fn decorate_turn_summary(
        &self,
        summary: &mut TurnTraceSummary,
    ) -> Result<(), ApplicationError> {
        let Some(store) = &self.session_store else {
            return Ok(());
        };
        if let Some(session) = store.load_session(&summary.session_id).await? {
            summary.session_title = Some(session.title);
            summary.working_dir = session.working_dir;
        }
        Ok(())
    }
}

#[derive(Default)]
struct TraceContextIndex {
    sessions: HashMap<String, SessionSummary>,
    messages_by_turn: HashMap<String, Vec<SessionMessage>>,
}

impl TraceContextIndex {
    fn new(sessions: Vec<SessionSummary>, messages: Vec<SessionMessage>) -> Self {
        let mut messages_by_turn = HashMap::<String, Vec<SessionMessage>>::new();
        for message in messages {
            messages_by_turn
                .entry(message.turn_id.clone())
                .or_default()
                .push(message);
        }
        Self {
            sessions: sessions
                .into_iter()
                .map(|session| (session.id.clone(), session))
                .collect(),
            messages_by_turn,
        }
    }
}

fn build_filter(query: &TraceListQuery, context: &TraceContextIndex) -> TraceRootFilter {
    let mut session_ids = query
        .session_id
        .as_deref()
        .and_then(non_blank)
        .map(|id| vec![id.to_string()]);
    if let Some(project) = query.project.as_deref().and_then(non_blank) {
        let project = project.to_lowercase();
        let matching = context
            .sessions
            .values()
            .filter(|session| {
                session
                    .working_dir
                    .as_deref()
                    .is_some_and(|path| path.to_lowercase().contains(&project))
            })
            .map(|session| session.id.clone())
            .collect::<HashSet<_>>();
        session_ids = Some(match session_ids {
            Some(ids) => ids.into_iter().filter(|id| matching.contains(id)).collect(),
            None => matching.into_iter().collect(),
        });
    }

    let normalized_query = query
        .query
        .as_deref()
        .and_then(non_blank)
        .map(str::to_lowercase);
    let mut search_session_ids = Vec::new();
    let mut search_turn_ids = Vec::new();
    if let Some(search) = &normalized_query {
        search_session_ids.extend(
            context
                .sessions
                .values()
                .filter(|session| {
                    format!(
                        "{} {}",
                        session.title,
                        session.working_dir.as_deref().unwrap_or_default()
                    )
                    .to_lowercase()
                    .contains(search)
                })
                .map(|session| session.id.clone()),
        );
        search_turn_ids.extend(
            context
                .messages_by_turn
                .iter()
                .filter(|(_, messages)| {
                    messages
                        .iter()
                        .any(|message| message_text(message).to_lowercase().contains(search))
                })
                .map(|(turn_id, _)| turn_id.clone()),
        );
    }

    TraceRootFilter {
        session_ids,
        query: normalized_query,
        search_session_ids,
        search_turn_ids,
        model: query.model.clone(),
        status: query.status,
        started_after_unix_ms: query.started_after,
        started_before_unix_ms: query.started_before,
        has_error: query.has_error,
        has_retry: query.has_retry,
    }
}

fn decorate_summary(summary: &mut TurnTraceSummary, context: &TraceContextIndex) {
    if let Some(session) = context.sessions.get(&summary.session_id) {
        summary.session_title = Some(session.title.clone());
        summary.working_dir = session.working_dir.clone();
    }
    summary.input_preview = context
        .messages_by_turn
        .get(&summary.turn_id)
        .and_then(|messages| messages.iter().find(|message| message.role == Role::User))
        .map(message_text)
        .and_then(|text| preview(&text));
}

fn summarize_by_turn(spans: Vec<TraceSpan>) -> Vec<TurnTraceSummary> {
    let mut by_turn = HashMap::<String, Vec<TraceSpan>>::new();
    for span in spans {
        by_turn.entry(span.turn_id.clone()).or_default().push(span);
    }
    let mut summaries = by_turn
        .into_iter()
        .map(|(turn_id, spans)| summarize(&turn_id, &spans))
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| left.turn_id.cmp(&right.turn_id))
    });
    summaries
}

fn summarize(turn_id: &str, spans: &[TraceSpan]) -> TurnTraceSummary {
    let root = spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn);
    let started_at = root
        .map(|span| span.started_at_unix_ms)
        .or_else(|| spans.iter().map(|span| span.started_at_unix_ms).min())
        .unwrap_or(0);
    let ended_at = root
        .and_then(|span| span.ended_at_unix_ms)
        .or_else(|| spans.iter().filter_map(|span| span.ended_at_unix_ms).max());
    let duration_end = ended_at.unwrap_or_else(now_unix_ms);
    let model_attempt_count = count_kind(spans, TraceSpanKind::ModelAttempt);
    let transport_attempt_count = count_kind(spans, TraceSpanKind::TransportAttempt);
    let (input_tokens, output_tokens) = spans
        .iter()
        .filter(|span| span.span_kind == TraceSpanKind::ModelAttempt)
        .fold((0_u64, 0_u64), |(inputs, outputs), span| {
            (
                inputs.saturating_add(attribute_u64(span, "inputTokens")),
                outputs.saturating_add(attribute_u64(span, "outputTokens")),
            )
        });
    let data_completeness = completeness(spans);
    let diagnosis = diagnose(spans, data_completeness);

    TurnTraceSummary {
        trace_id: root
            .map(|span| span.trace_id.clone())
            .unwrap_or_else(|| turn_id.to_string()),
        turn_id: turn_id.to_string(),
        session_id: root
            .map(|span| span.session_id.clone())
            .or_else(|| spans.first().map(|span| span.session_id.clone()))
            .unwrap_or_default(),
        session_title: None,
        working_dir: None,
        input_preview: None,
        status: root
            .map(|span| span.status)
            .unwrap_or(TraceSpanStatus::Running),
        model: root.and_then(|span| attribute_string(span, "model")),
        started_at,
        ended_at,
        duration_ms: duration_end.saturating_sub(started_at).max(0),
        step_count: count_kind(spans, TraceSpanKind::Step),
        model_attempt_count,
        transport_attempt_count,
        tool_run_count: count_kind(spans, TraceSpanKind::ToolRun),
        approval_count: count_kind(spans, TraceSpanKind::Approval),
        retry_count: transport_attempt_count.saturating_sub(model_attempt_count),
        input_tokens,
        output_tokens,
        error_count: spans
            .iter()
            .filter(|span| span.status == TraceSpanStatus::Failed)
            .count(),
        recovered: spans
            .iter()
            .any(|span| span.span_kind == TraceSpanKind::Recovery),
        diagnosis,
        data_completeness,
    }
}

fn completeness(spans: &[TraceSpan]) -> TraceDataCompleteness {
    if spans.is_empty() {
        return TraceDataCompleteness::Unknown;
    }
    let Some(root) = spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn)
    else {
        return TraceDataCompleteness::Partial;
    };
    let span_ids = spans
        .iter()
        .map(|span| span.span_id.as_str())
        .collect::<HashSet<_>>();
    if spans.iter().any(|span| {
        span.parent_span_id
            .as_deref()
            .is_some_and(|parent| !span_ids.contains(parent))
    }) {
        return TraceDataCompleteness::Partial;
    }
    if attribute_string(root, "traceSchemaVersion").is_none() {
        TraceDataCompleteness::Legacy
    } else {
        TraceDataCompleteness::Complete
    }
}

fn diagnose(spans: &[TraceSpan], completeness: TraceDataCompleteness) -> TraceDiagnosis {
    let root = spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Turn);
    let span_ids = spans
        .iter()
        .map(|span| span.span_id.as_str())
        .collect::<HashSet<_>>();
    let orphan_ids = spans
        .iter()
        .filter(|span| {
            span.parent_span_id
                .as_deref()
                .is_some_and(|parent| !span_ids.contains(parent))
        })
        .map(|span| span.span_id.clone())
        .collect::<Vec<_>>();
    if matches!(
        completeness,
        TraceDataCompleteness::Partial | TraceDataCompleteness::Unknown
    ) {
        let evidence = if orphan_ids.is_empty() {
            spans
                .first()
                .map(|span| span.span_id.clone())
                .into_iter()
                .collect()
        } else {
            orphan_ids
        };
        return diagnosis(
            TraceDiagnosisStatus::Attention,
            TraceDiagnosisReason::PartialTrace,
            evidence,
        );
    }
    if let Some(span) = spans
        .iter()
        .find(|span| span.status == TraceSpanStatus::OutcomeUnknown)
    {
        return diagnosis(
            TraceDiagnosisStatus::Blocked,
            TraceDiagnosisReason::OutcomeUnknown,
            vec![span.span_id.clone()],
        );
    }
    if let Some(span) = spans.iter().find(|span| {
        span.span_kind == TraceSpanKind::Approval && span.status == TraceSpanStatus::Waiting
    }) {
        return diagnosis(
            TraceDiagnosisStatus::Attention,
            TraceDiagnosisReason::WaitingApproval,
            vec![span.span_id.clone()],
        );
    }
    if root.is_some_and(|span| span.status == TraceSpanStatus::Succeeded)
        && let Some(span) = spans
            .iter()
            .find(|span| span.span_kind == TraceSpanKind::Recovery)
    {
        return diagnosis(
            TraceDiagnosisStatus::Attention,
            TraceDiagnosisReason::Recovered,
            vec![span.span_id.clone()],
        );
    }
    if let Some(span) = spans.iter().find(|span| {
        span.span_kind == TraceSpanKind::Approval && span.status == TraceSpanStatus::Denied
    }) {
        let status = if root.is_some_and(|root| {
            matches!(
                root.status,
                TraceSpanStatus::Failed | TraceSpanStatus::Denied
            )
        }) {
            TraceDiagnosisStatus::Blocked
        } else {
            TraceDiagnosisStatus::Attention
        };
        return diagnosis(
            status,
            TraceDiagnosisReason::ApprovalDenied,
            vec![span.span_id.clone()],
        );
    }
    if root.is_some_and(|span| span.status == TraceSpanStatus::Failed) {
        for (kind, reason) in [
            (TraceSpanKind::ToolRun, TraceDiagnosisReason::ToolError),
            (
                TraceSpanKind::ModelAttempt,
                TraceDiagnosisReason::ModelError,
            ),
            (
                TraceSpanKind::TransportAttempt,
                TraceDiagnosisReason::TransportExhausted,
            ),
        ] {
            if let Some(span) = spans
                .iter()
                .find(|span| span.span_kind == kind && span.status == TraceSpanStatus::Failed)
            {
                return diagnosis(
                    TraceDiagnosisStatus::Blocked,
                    reason,
                    vec![span.span_id.clone()],
                );
            }
        }
        return diagnosis(
            TraceDiagnosisStatus::Blocked,
            TraceDiagnosisReason::ModelError,
            root.map(|span| span.span_id.clone()).into_iter().collect(),
        );
    }
    if root.is_some_and(|span| span.status == TraceSpanStatus::Cancelled) {
        return diagnosis(
            TraceDiagnosisStatus::Attention,
            TraceDiagnosisReason::Cancelled,
            root.map(|span| span.span_id.clone()).into_iter().collect(),
        );
    }
    if let Some(span) = spans
        .iter()
        .find(|span| span.span_kind == TraceSpanKind::Recovery)
    {
        return diagnosis(
            TraceDiagnosisStatus::Attention,
            TraceDiagnosisReason::Recovered,
            vec![span.span_id.clone()],
        );
    }
    diagnosis(
        TraceDiagnosisStatus::Healthy,
        TraceDiagnosisReason::Healthy,
        root.map(|span| span.span_id.clone()).into_iter().collect(),
    )
}

fn diagnosis(
    status: TraceDiagnosisStatus,
    reason: TraceDiagnosisReason,
    evidence_span_ids: Vec<String>,
) -> TraceDiagnosis {
    TraceDiagnosis {
        status,
        reason,
        focus_span_id: evidence_span_ids.first().cloned(),
        evidence_span_ids,
    }
}

fn build_detail(
    span: &TraceSpan,
    spans: &[TraceSpan],
    messages: &[SessionMessage],
    lifecycle: Option<&openwork_persistence::TurnLifecycleSnapshot>,
) -> TraceSpanDetail {
    match span.span_kind {
        TraceSpanKind::Turn => {
            let completeness = completeness(spans);
            TraceSpanDetail::Turn(TraceTurnDetail {
                trace_schema_version: attribute_string(span, "traceSchemaVersion"),
                instrumentation_version: attribute_string(span, "instrumentationVersion"),
                app_version: attribute_string(span, "appVersion"),
                capture_mode: attribute_string(span, "captureMode"),
                outcome: attribute_string(span, "outcome"),
                diagnosis: diagnose(spans, completeness),
                data_completeness: completeness,
            })
        }
        TraceSpanKind::Step => {
            let step_messages = messages
                .iter()
                .filter(|message| message.step_id == span.step_id)
                .cloned()
                .collect();
            TraceSpanDetail::Step(TraceStepDetail {
                step_index: attribute_u64_opt(span, "stepIndex"),
                tool_count: spans
                    .iter()
                    .filter(|child| {
                        child.span_kind == TraceSpanKind::ToolRun && child.step_id == span.step_id
                    })
                    .count(),
                messages: step_messages,
            })
        }
        TraceSpanKind::ModelAttempt => {
            let context_messages = messages
                .iter()
                .filter(|message| {
                    message.role == Role::User
                        || span
                            .step_id
                            .as_ref()
                            .is_some_and(|step_id| message.step_id.as_ref() == Some(step_id))
                })
                .cloned()
                .collect();
            TraceSpanDetail::ModelAttempt(TraceModelAttemptDetail {
                provider_id: attribute_string(span, "providerId"),
                model: attribute_string(span, "model"),
                finish_reason: attribute_string(span, "finishReason"),
                raw_finish_reason: attribute_string(span, "rawFinishReason"),
                response_id: attribute_string(span, "responseId"),
                provider_request_id: attribute_string(span, "providerRequestId"),
                first_output_ms: attribute_i64(span, "firstOutputMs"),
                usage: TraceTokenUsage {
                    input_tokens: attribute_u64(span, "inputTokens"),
                    output_tokens: attribute_u64(span, "outputTokens"),
                    total_tokens: attribute_u64(span, "totalTokens"),
                    cached_input_tokens: attribute_u64(span, "cachedInputTokens"),
                    reasoning_tokens: attribute_u64(span, "reasoningTokens"),
                },
                request_summary: TraceModelRequestSummary {
                    version: attribute_string(span, "requestSummaryVersion"),
                    message_count: attribute_u64_opt(span, "messageCount"),
                    message_text_chars: attribute_u64_opt(span, "messageTextChars"),
                    system_prompt_chars: attribute_u64_opt(span, "systemPromptChars"),
                    tool_definition_count: attribute_u64_opt(span, "toolDefinitionCount"),
                    tool_names: attribute_strings(span, "toolNames"),
                    temperature: attribute_f64(span, "temperature"),
                    max_output_tokens: attribute_u64_opt(span, "maxOutputTokens"),
                    thinking_mode: attribute_string(span, "thinkingMode"),
                },
                messages: context_messages,
            })
        }
        TraceSpanKind::TransportAttempt => {
            TraceSpanDetail::TransportAttempt(TraceTransportAttemptDetail {
                provider_id: attribute_string(span, "providerId"),
                transport_attempt: attribute_u64_opt(span, "transportAttempt"),
                http_status: attribute_u64_opt(span, "httpStatus"),
                provider_code: attribute_string(span, "providerCode"),
                provider_request_id: attribute_string(span, "providerRequestId"),
                retry_delay_ms: attribute_i64(span, "retryDelayMs"),
                will_retry: attribute_bool(span, "willRetry"),
                failure_phase: attribute_string(span, "failurePhase"),
                delivery_state: attribute_string(span, "deliveryState"),
            })
        }
        TraceSpanKind::ToolRun => {
            let lifecycle_tool = lifecycle.and_then(|turn| {
                turn.steps
                    .iter()
                    .flat_map(|step| step.tool_runs.iter())
                    .find(|tool| span.tool_run_id.as_deref() == Some(tool.id.as_str()))
            });
            let approval_wait_ms = spans
                .iter()
                .filter(|child| {
                    child.span_kind == TraceSpanKind::Approval
                        && child.parent_span_id.as_deref() == Some(span.span_id.as_str())
                })
                .filter_map(TraceSpan::duration_ms)
                .max();
            let execution_started_at = attribute_i64(span, "executionStartedAt");
            TraceSpanDetail::ToolRun(TraceToolRunDetail {
                tool_name: lifecycle_tool
                    .map(|tool| tool.tool_name.clone())
                    .or_else(|| attribute_string(span, "toolName")),
                provider_tool_call_id: lifecycle_tool
                    .map(|tool| tool.provider_tool_call_id.clone())
                    .or_else(|| attribute_string(span, "providerToolCallId")),
                input: lifecycle_tool.map(|tool| tool.input.clone()),
                observation: lifecycle_tool.and_then(|tool| tool.observation.clone()),
                approval_required: attribute_bool(span, "approvalRequired").unwrap_or(false)
                    || approval_wait_ms.is_some(),
                requested_at: attribute_i64(span, "requestedAt").or(Some(span.started_at_unix_ms)),
                execution_started_at,
                request_to_end_ms: span.duration_ms(),
                approval_wait_ms,
                execution_ms: execution_started_at.and_then(|started| {
                    span.ended_at_unix_ms
                        .map(|ended| ended.saturating_sub(started).max(0))
                }),
            })
        }
        TraceSpanKind::Approval => TraceSpanDetail::Approval(TraceApprovalDetail {
            reason: attribute_string(span, "reason"),
            tool_name: attribute_string(span, "toolName"),
            resolution: attribute_string(span, "resolution"),
            wait_ms: span.duration_ms(),
        }),
        TraceSpanKind::Recovery => TraceSpanDetail::Recovery(TraceRecoveryDetail {
            reason: attribute_string(span, "reason"),
            approval_id: attribute_string(span, "approvalId"),
            step_index: attribute_u64_opt(span, "stepIndex"),
        }),
    }
}

fn span_view(span: TraceSpan) -> TraceSpanView {
    TraceSpanView {
        duration_ms: span.duration_ms(),
        span,
    }
}

fn count_kind(spans: &[TraceSpan], kind: TraceSpanKind) -> usize {
    spans.iter().filter(|span| span.span_kind == kind).count()
}

fn attribute_u64(span: &TraceSpan, key: &str) -> u64 {
    attribute_u64_opt(span, key).unwrap_or(0)
}

fn attribute_u64_opt(span: &TraceSpan, key: &str) -> Option<u64> {
    span.attributes.get(key).and_then(Value::as_u64)
}

fn attribute_i64(span: &TraceSpan, key: &str) -> Option<i64> {
    span.attributes.get(key).and_then(Value::as_i64)
}

fn attribute_f64(span: &TraceSpan, key: &str) -> Option<f64> {
    span.attributes.get(key).and_then(Value::as_f64)
}

fn attribute_bool(span: &TraceSpan, key: &str) -> Option<bool> {
    span.attributes.get(key).and_then(Value::as_bool)
}

fn attribute_string(span: &TraceSpan, key: &str) -> Option<String> {
    span.attributes
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn attribute_strings(span: &TraceSpan, key: &str) -> Vec<String> {
    span.attributes
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn message_text(message: &SessionMessage) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            ContentBlock::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn preview(value: &str) -> Option<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let mut chars = normalized.chars();
    let shortened = chars.by_ref().take(INPUT_PREVIEW_CHARS).collect::<String>();
    Some(if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    })
}

fn non_blank(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}
