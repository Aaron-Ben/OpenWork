use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::protocol::{
    AgendaDecisionRequest, AgendaDecisionResponse, AgendaPayload, AgentCommandEffect,
    AgentCommandRequest, AgentCommandResponse, AgentTokenResponse, ApiError,
    ComputerHeartbeatRequest, DesiredAgents, DesktopCommandRequest, DesktopCommandResult,
    EngineInventoryReport, FinishRunRequest, InboxResponse, InvalidationEvent, InvalidationKind,
    OpenRunRequest, RunView, TriagePayload, TriageReportRequest, entity_id,
};

use super::{
    agenda::Agenda, agent_commands::AgentCommands, agents::Agents, auth::AgentClaims,
    coordination::Coordination, desktop_commands::DesktopCommands, inventory::EngineInventory,
    messages::Messages, runs::Runs, runtime_session::RuntimeSession, scheduler::Scheduler,
    triage::InboxTriage,
};

#[derive(Clone)]
pub(crate) struct TransportState {
    pub(crate) agents: Agents,
    pub(crate) messages: Messages,
    pub(crate) runs: Runs,
    pub(crate) scheduler: Scheduler,
    pub(crate) coordination: Coordination,
    pub(crate) triage: InboxTriage,
    pub(crate) agenda: Agenda,
    pub(crate) agent_commands: AgentCommands,
    pub(crate) desktop_commands: DesktopCommands,
    pub(crate) inventory: EngineInventory,
    pub(crate) session: RuntimeSession,
}

pub(crate) fn router(state: TransportState) -> Router {
    let desktop = Router::new()
        .route("/commands", post(desktop_command))
        .route("/events", get(desktop_events));
    let computer = Router::new()
        .route("/heartbeat", post(computer_heartbeat))
        .route("/agents", get(desired_agents))
        .route("/agents/{agent_id}/token", post(mint_agent_token))
        .route("/inventory", post(report_inventory))
        .route("/events", get(management_events));
    let agent = Router::new()
        .route("/events", get(agent_events))
        .route("/inbox", get(inbox))
        .route("/inbox-triage/payload", get(triage_payload))
        .route("/triage", post(report_triage))
        .route("/agenda/payload", get(agenda_payload))
        .route("/agenda/decision", post(agenda_decision))
        .route("/runs", post(open_run))
        .route("/runs/{run_id}/heartbeat", post(heartbeat_run))
        .route("/runs/{run_id}/finish", post(finish_run))
        .route("/commands", post(agent_command));
    Router::new()
        .nest("/desktop", desktop)
        .nest("/computer", computer)
        .nest("/agent", agent)
        .with_state(state)
}

async fn desktop_command(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(request): Json<DesktopCommandRequest>,
) -> Result<Json<DesktopCommandResult>, TransportError> {
    authorize_desktop(&state, &headers)?;
    Ok(Json(state.desktop_commands.execute(request).await?))
}

async fn desktop_events(
    State(state): State<TransportState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TransportError> {
    authorize_desktop(&state, &headers)?;
    Ok(invalidation_stream(
        state.session.subscribe_desktop(),
        initial_event(InvalidationKind::RuntimeReady, None),
        "desktop",
    ))
}

async fn computer_heartbeat(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(request): Json<ComputerHeartbeatRequest>,
) -> Result<StatusCode, TransportError> {
    authorize_computer(&state, &headers)?;
    state.session.note_computer_heartbeat(request);
    Ok(StatusCode::NO_CONTENT)
}

async fn desired_agents(
    State(state): State<TransportState>,
    headers: HeaderMap,
) -> Result<Json<DesiredAgents>, TransportError> {
    authorize_computer(&state, &headers)?;
    state.session.touch_computer_heartbeat();
    Ok(Json(DesiredAgents {
        runtime_session_id: state.session.id().to_string(),
        agents: state.agents.assignments().await?,
    }))
}

async fn mint_agent_token(
    State(state): State<TransportState>,
    Path(agent_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AgentTokenResponse>, TransportError> {
    authorize_computer(&state, &headers)?;
    if !state.agents.is_active(&agent_id).await? {
        return Err(TransportError::not_found("Agent is missing or archived"));
    }
    let (token, expires_at) = state.session.signing_key().mint_agent_token(
        &agent_id,
        state.session.id(),
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    Ok(Json(AgentTokenResponse { token, expires_at }))
}

async fn report_inventory(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(report): Json<EngineInventoryReport>,
) -> Result<StatusCode, TransportError> {
    authorize_computer(&state, &headers)?;
    state
        .inventory
        .report(state.session.id(), &report.engines)
        .await?;
    state.session.touch_computer_heartbeat();
    for engine in &report.engines {
        state.session.publish_inventory(&engine.engine_id);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn management_events(
    State(state): State<TransportState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TransportError> {
    authorize_computer(&state, &headers)?;
    state.session.touch_computer_heartbeat();
    Ok(invalidation_stream(
        state.session.subscribe_management(),
        initial_event(InvalidationKind::RuntimeReady, None),
        "management",
    ))
}

async fn agent_events(
    State(state): State<TransportState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    let agent_id = claims.sub;
    let receiver = state.scheduler.subscribe_wakes();
    let initial = initial_event(InvalidationKind::Message, Some(agent_id.clone()));
    let stream = futures_util::stream::unfold(
        (receiver, true, agent_id),
        move |(mut receiver, initial_pending, agent_id)| {
            let initial = initial.clone();
            async move {
                if initial_pending {
                    return Some((
                        Ok::<_, Infallible>(sse_event("agent", &initial)),
                        (receiver, false, agent_id),
                    ));
                }
                loop {
                    match receiver.recv().await {
                        Ok(wake) if wake.agent_id == agent_id => {
                            let event = InvalidationEvent {
                                id: wake.id,
                                kind: InvalidationKind::Message,
                                subject_id: Some(wake.message_id),
                                revision: None,
                                published_at: wake.published_at,
                            };
                            return Some((
                                Ok::<_, Infallible>(sse_event("agent", &event)),
                                (receiver, false, agent_id),
                            ));
                        }
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(keep_alive()))
}

async fn inbox(
    State(state): State<TransportState>,
    headers: HeaderMap,
) -> Result<Json<InboxResponse>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    let mut response = state.messages.inbox(&claims).await?;
    if let Some(trigger) = &mut response.trigger {
        for delivery in &trigger.deliveries {
            if let Err(error) = state
                .coordination
                .record_seen(&claims.sub, &delivery.room_id, delivery.up_to_seq)
                .await
            {
                tracing::warn!(%error, agent_id = claims.sub, "inbox seen update failed open");
            }
        }
        state.session.signing_key().sign_trigger(trigger)?;
    }
    Ok(Json(response))
}

#[derive(Deserialize)]
struct RunIdQuery {
    run_id: String,
}

async fn triage_payload(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Query(query): Query<RunIdQuery>,
) -> Result<Json<TriagePayload>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    Ok(Json(state.triage.payload(&claims, &query.run_id).await?))
}

async fn report_triage(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(request): Json<TriageReportRequest>,
) -> Result<StatusCode, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    state.triage.report(&claims, &request).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn agenda_payload(
    State(state): State<TransportState>,
    headers: HeaderMap,
) -> Result<Json<AgendaPayload>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    Ok(Json(state.agenda.payload(&claims).await?))
}

async fn agenda_decision(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(request): Json<AgendaDecisionRequest>,
) -> Result<Json<AgendaDecisionResponse>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    Ok(Json(state.agenda.decide(&claims, request).await?))
}

async fn open_run(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(request): Json<OpenRunRequest>,
) -> Result<Json<RunView>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    state
        .session
        .signing_key()
        .verify_trigger(&request.trigger)?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if request.trigger.expires_at <= now
        || request.trigger.protocol_version != crate::protocol::COLLAB_PROTOCOL_VERSION
        || request.trigger.agent_id != claims.sub
        || request.trigger.runtime_session_id != claims.runtime_session_id
        || !valid_trigger_shape(&request.trigger)
    {
        return Err(TransportError::unauthorized("invalid trigger envelope"));
    }
    Ok(Json(state.runs.open(&claims, &request.trigger).await?))
}

fn valid_trigger_shape(trigger: &crate::protocol::TriggerEnvelope) -> bool {
    match trigger.trigger.as_str() {
        "message" | "rerun" | "reconnect" | "poll" => {
            !trigger.deliveries.is_empty() && trigger.agenda_focus.is_none()
        }
        "agenda" => trigger.deliveries.is_empty() && trigger.agenda_focus.is_some(),
        _ => false,
    }
}

async fn agent_command(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(request): Json<AgentCommandRequest>,
) -> Result<Json<AgentCommandResponse>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    let response = state.agent_commands.execute(&claims, request).await?;
    for effect in &response.effects {
        if let AgentCommandEffect::MessagePublished {
            room_id,
            message_id,
            ..
        } = effect
        {
            state
                .scheduler
                .message_committed(message_id, room_id, &claims.sub)
                .await;
        }
    }
    Ok(Json(response))
}

async fn finish_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<FinishRunRequest>,
) -> Result<Json<RunView>, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    Ok(Json(state.runs.finish(&claims, &run_id, request).await?))
}

async fn heartbeat_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, TransportError> {
    let claims = agent_claims(&state, &headers).await?;
    state.runs.heartbeat(&claims, &run_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn agent_claims(
    state: &TransportState,
    headers: &HeaderMap,
) -> Result<AgentClaims, TransportError> {
    let claims = state.session.signing_key().verify_agent_token(
        bearer(headers)?,
        state.session.id(),
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    if !state.agents.is_active(&claims.sub).await? {
        return Err(TransportError::unauthorized("Agent is archived or missing"));
    }
    Ok(claims)
}

fn authorize_desktop(state: &TransportState, headers: &HeaderMap) -> Result<(), TransportError> {
    if state.session.authorize_desktop(bearer(headers)?) {
        Ok(())
    } else {
        Err(TransportError::unauthorized("invalid Desktop credential"))
    }
}

fn authorize_computer(state: &TransportState, headers: &HeaderMap) -> Result<(), TransportError> {
    if state.session.authorize_computer(bearer(headers)?) {
        Ok(())
    } else {
        Err(TransportError::unauthorized("invalid Computer credential"))
    }
}

fn bearer(headers: &HeaderMap) -> Result<&str, TransportError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TransportError::unauthorized("missing bearer token"))
}

fn invalidation_stream(
    receiver: broadcast::Receiver<InvalidationEvent>,
    initial: InvalidationEvent,
    event_name: &'static str,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let stream = futures_util::stream::unfold(
        (receiver, Some(initial)),
        move |(mut receiver, initial)| async move {
            if let Some(initial) = initial {
                return Some((
                    Ok::<_, Infallible>(sse_event(event_name, &initial)),
                    (receiver, None),
                ));
            }
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        return Some((
                            Ok::<_, Infallible>(sse_event(event_name, &event)),
                            (receiver, None),
                        ));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    );
    Sse::new(stream).keep_alive(keep_alive())
}

fn initial_event(kind: InvalidationKind, subject_id: Option<String>) -> InvalidationEvent {
    InvalidationEvent {
        id: entity_id("event"),
        kind,
        subject_id,
        revision: None,
        published_at: time::OffsetDateTime::now_utc().unix_timestamp(),
    }
}

fn sse_event(name: &str, event: &InvalidationEvent) -> Event {
    Event::default()
        .event(name)
        .id(event.id.clone())
        .data(serde_json::to_string(event).expect("InvalidationEvent is serializable"))
}

fn keep_alive() -> KeepAlive {
    KeepAlive::new()
        .interval(Duration::from_secs(15))
        .text("keepalive")
}

#[derive(Debug)]
pub(crate) struct TransportError {
    status: StatusCode,
    code: String,
    message: String,
}

impl TransportError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHENTICATED".to_string(),
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for TransportError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Protocol(message) if message.starts_with("UNAUTHENTICATED:") => {
                Self::unauthorized(message.clone())
            }
            sqlx::Error::Protocol(message) if message.starts_with("NOT_FOUND:") => {
                Self::not_found(message.clone())
            }
            sqlx::Error::Protocol(message) if message.starts_with("CONFLICT:") => Self {
                status: StatusCode::CONFLICT,
                code: "CONFLICT".to_string(),
                message: message.clone(),
            },
            sqlx::Error::Protocol(message) if message.starts_with("INVALID_ARGUMENT:") => Self {
                status: StatusCode::BAD_REQUEST,
                code: "INVALID_ARGUMENT".to_string(),
                message: message.clone(),
            },
            sqlx::Error::RowNotFound => Self::not_found("not found"),
            _ => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL".to_string(),
                message: error.to_string(),
            },
        }
    }
}

impl From<super::auth::AuthError> for TransportError {
    fn from(error: super::auth::AuthError) -> Self {
        Self::unauthorized(error.to_string())
    }
}

impl IntoResponse for TransportError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}
