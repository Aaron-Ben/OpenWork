use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

use crate::protocol::{
    AgentRoster, AgentTokenResponse, CliRequest, CliResult, DeviceStartResponse, FinishRunRequest,
    HeartbeatRequest, InboxResponse, OpenRunRequest, RunView,
};

use super::{
    auth::{AgentClaims, SigningKey},
    storage::CollaborationStore,
};

#[derive(Clone)]
struct RuntimeState {
    store: CollaborationStore,
    signing_key: SigningKey,
}

pub fn router(store: CollaborationStore, signing_key: SigningKey) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/computers/me/start", post(start_computer))
        .route("/api/computers/me/heartbeat", post(heartbeat))
        .route("/api/computers/me/agents", get(roster))
        .route(
            "/api/computers/me/agents/{agent_id}/token",
            post(mint_agent_token),
        )
        .route("/runtime/inbox", get(inbox))
        .route("/runtime/runs", post(open_run))
        .route("/runtime/cli", post(run_cli))
        .route("/runtime/runs/{run_id}/finish", post(finish_run))
        .with_state(RuntimeState { store, signing_key })
}

async fn start_computer(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
) -> Result<Json<DeviceStartResponse>, RuntimeError> {
    let generation = state.store.start_computer(bearer(&headers)?).await?;
    Ok(Json(DeviceStartResponse { generation }))
}

async fn heartbeat(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<HeartbeatRequest>,
) -> Result<StatusCode, RuntimeError> {
    state.store.heartbeat(bearer(&headers)?, &request).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct GenerationQuery {
    generation: i64,
}

async fn roster(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Query(query): Query<GenerationQuery>,
) -> Result<Json<AgentRoster>, RuntimeError> {
    let agents = state
        .store
        .roster(bearer(&headers)?, query.generation)
        .await?;
    Ok(Json(AgentRoster { agents }))
}

#[derive(Deserialize)]
struct GenerationBody {
    generation: i64,
}

async fn mint_agent_token(
    State(state): State<RuntimeState>,
    Path(agent_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<GenerationBody>,
) -> Result<Json<AgentTokenResponse>, RuntimeError> {
    state
        .store
        .authorize_agent_token(bearer(&headers)?, body.generation, &agent_id)
        .await?;
    let (token, expires_at) = state.signing_key.mint_agent_token(
        &agent_id,
        body.generation,
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    Ok(Json(AgentTokenResponse { token, expires_at }))
}

async fn inbox(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
) -> Result<Json<InboxResponse>, RuntimeError> {
    let claims = agent_claims(&state, &headers).await?;
    let mut response = state.store.inbox(&claims).await?;
    if let Some(trigger) = &mut response.trigger {
        state.signing_key.sign_trigger(trigger)?;
    }
    Ok(Json(response))
}

async fn open_run(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<OpenRunRequest>,
) -> Result<Json<RunView>, RuntimeError> {
    let claims = agent_claims(&state, &headers).await?;
    state.signing_key.verify_trigger(&request.trigger)?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if request.trigger.expires_at <= now
        || request.trigger.protocol_version != crate::protocol::COLLAB_PROTOCOL_VERSION
        || request.trigger.agent_id != claims.sub
        || request.trigger.computer_generation != claims.generation
    {
        return Err(RuntimeError::unauthorized("invalid trigger envelope"));
    }
    Ok(Json(state.store.open_run(&claims, &request.trigger).await?))
}

async fn run_cli(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<CliRequest>,
) -> Result<Json<CliResult>, RuntimeError> {
    let claims = agent_claims(&state, &headers).await?;
    Ok(Json(state.store.run_cli(&claims, request.argv).await?))
}

async fn finish_run(
    State(state): State<RuntimeState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<FinishRunRequest>,
) -> Result<Json<RunView>, RuntimeError> {
    let claims = agent_claims(&state, &headers).await?;
    Ok(Json(
        state.store.finish_run(&claims, &run_id, request).await?,
    ))
}

async fn agent_claims(
    state: &RuntimeState,
    headers: &HeaderMap,
) -> Result<AgentClaims, RuntimeError> {
    let claims = state.signing_key.verify_agent_token(
        bearer(headers)?,
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    state.store.authorize_agent(&claims).await?;
    Ok(claims)
}

fn bearer(headers: &HeaderMap) -> Result<&str, RuntimeError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RuntimeError::unauthorized("missing bearer token"))
}

#[derive(Debug)]
struct RuntimeError {
    status: StatusCode,
    message: String,
}

impl RuntimeError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for RuntimeError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Protocol(message) if message.starts_with("FENCED:") => Self {
                status: StatusCode::CONFLICT,
                message: message.clone(),
            },
            sqlx::Error::Protocol(message) if message.starts_with("UNAUTHENTICATED:") => {
                Self::unauthorized(message.clone())
            }
            sqlx::Error::RowNotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: "not found".to_string(),
            },
            _ => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            },
        }
    }
}

impl From<super::auth::AuthError> for RuntimeError {
    fn from(error: super::auth::AuthError) -> Self {
        Self::unauthorized(error.to_string())
    }
}

impl IntoResponse for RuntimeError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}
