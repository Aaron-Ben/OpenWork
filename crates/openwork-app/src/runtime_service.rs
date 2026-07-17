use openwork_core::{
    ClientRequestId, LoadedSession, ModelInput, OpenWorkCore, OpenWorkCoreError,
    PermissionDecision, SessionId, SessionInput, SessionRecord, SessionSnapshot,
    SessionUpdateEnvelope, ToolCallId, TraceSpanRecord, TraceTurnSummary, TurnAccepted,
    session::TurnId,
};
use openwork_protocol::model::ContentBlock;
use tokio::sync::broadcast;

use crate::{ApplicationError, ApplicationErrorCode};

pub struct RuntimeApplicationService {
    core: OpenWorkCore,
}

impl RuntimeApplicationService {
    pub(crate) fn new(core: OpenWorkCore) -> Self {
        Self { core }
    }

    pub async fn register_model(&self, input: &ModelInput) -> Result<(), ApplicationError> {
        self.core
            .register_model(input)
            .await
            .map_err(map_core_error)
    }

    pub async fn create_session(
        &self,
        input: &SessionInput,
    ) -> Result<SessionRecord, ApplicationError> {
        self.core
            .create_session(input)
            .await
            .map_err(map_core_error)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, ApplicationError> {
        self.core.list_sessions().await.map_err(map_core_error)
    }

    pub async fn load_session(&self, session_id: &str) -> Result<LoadedSession, ApplicationError> {
        self.core
            .load_session(&SessionId::new(session_id))
            .await
            .map_err(map_core_error)
    }

    pub async fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<SessionRecord, ApplicationError> {
        self.core
            .rename_session(&SessionId::new(session_id), title)
            .await
            .map_err(map_core_error)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), ApplicationError> {
        self.core
            .delete_session(&SessionId::new(session_id))
            .await
            .map_err(map_core_error)
    }

    pub async fn start_turn(
        &self,
        session_id: &str,
        client_request_id: &str,
        text: &str,
    ) -> Result<TurnAccepted, ApplicationError> {
        self.core
            .start_turn(
                &SessionId::new(session_id),
                ClientRequestId::new(client_request_id),
                vec![ContentBlock::text(text)],
            )
            .await
            .map_err(map_core_error)
    }

    pub async fn cancel_turn(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<bool, ApplicationError> {
        self.core
            .cancel_turn(&SessionId::new(session_id), TurnId::new(turn_id))
            .await
            .map_err(map_core_error)
    }

    pub async fn resolve_permission(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_call_id: &str,
        allow: bool,
    ) -> Result<(), ApplicationError> {
        self.core
            .resolve_permission(
                &SessionId::new(session_id),
                TurnId::new(turn_id),
                ToolCallId::new(tool_call_id),
                if allow {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::Deny
                },
            )
            .await
            .map_err(map_core_error)
    }

    pub async fn subscribe_updates(
        &self,
        session_id: &str,
    ) -> Result<broadcast::Receiver<SessionUpdateEnvelope>, ApplicationError> {
        self.core
            .subscribe_updates(&SessionId::new(session_id))
            .await
            .map_err(map_core_error)
    }

    pub async fn snapshot(&self, session_id: &str) -> Result<SessionSnapshot, ApplicationError> {
        self.core
            .get_session_snapshot(&SessionId::new(session_id))
            .await
            .map_err(map_core_error)
    }

    pub async fn replay_updates(
        &self,
        session_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<SessionUpdateEnvelope>, ApplicationError> {
        self.core
            .replay_updates(&SessionId::new(session_id), after_sequence)
            .await
            .map_err(map_core_error)
    }

    pub async fn list_traces(
        &self,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<TraceTurnSummary>, ApplicationError> {
        let session_id = session_id.map(SessionId::new);
        self.core
            .list_traces(session_id.as_ref(), limit)
            .await
            .map_err(map_core_error)
    }

    pub async fn get_trace(&self, turn_id: &str) -> Result<Vec<TraceSpanRecord>, ApplicationError> {
        self.core
            .get_trace(&TurnId::new(turn_id))
            .await
            .map_err(map_core_error)
    }
}

fn map_core_error(error: OpenWorkCoreError) -> ApplicationError {
    use openwork_core::{SessionError, StorageError};

    match error {
        OpenWorkCoreError::SessionNotFound(id) => ApplicationError::new(
            ApplicationErrorCode::SessionNotFound,
            format!("Session not found: {id}"),
        ),
        OpenWorkCoreError::ModelNotFound(id) => ApplicationError::new(
            ApplicationErrorCode::ConfigurationInvalid,
            format!("Model not found: {id}"),
        ),
        OpenWorkCoreError::SessionActive(id) => ApplicationError::new(
            ApplicationErrorCode::OperationConflict,
            format!("Session has an active turn: {id}"),
        ),
        OpenWorkCoreError::Session(SessionError::Busy(turn_id)) => ApplicationError::new(
            ApplicationErrorCode::OperationConflict,
            format!("Session already has an active turn: {turn_id}"),
        ),
        OpenWorkCoreError::Session(SessionError::TurnNotActive(turn_id)) => ApplicationError::new(
            ApplicationErrorCode::TurnNotFound,
            format!("Turn is not active: {turn_id}"),
        ),
        OpenWorkCoreError::Session(SessionError::PermissionNotPending(tool_call_id)) => {
            ApplicationError::new(
                ApplicationErrorCode::ApprovalNotFound,
                format!("Permission request is not pending: {tool_call_id}"),
            )
        }
        OpenWorkCoreError::Session(SessionError::EmptyInput) => ApplicationError::new(
            ApplicationErrorCode::InvalidRequest,
            "Turn input must not be empty",
        ),
        OpenWorkCoreError::Storage(StorageError::InvalidInput(message)) => {
            ApplicationError::new(ApplicationErrorCode::InvalidRequest, message)
        }
        OpenWorkCoreError::Storage(StorageError::SessionNotFound(id)) => ApplicationError::new(
            ApplicationErrorCode::SessionNotFound,
            format!("Session not found: {id}"),
        ),
        OpenWorkCoreError::Storage(StorageError::TurnNotFound(id)) => ApplicationError::new(
            ApplicationErrorCode::TurnNotFound,
            format!("Turn not found: {id}"),
        ),
        OpenWorkCoreError::Storage(StorageError::Database(_)) => ApplicationError::new(
            ApplicationErrorCode::DatabaseUnavailable,
            "Runtime persistence is unavailable",
        ),
        OpenWorkCoreError::Storage(StorageError::MigrationDrift { .. }) => ApplicationError::new(
            ApplicationErrorCode::SchemaNotReady,
            "Runtime database migration checksum does not match",
        ),
        OpenWorkCoreError::Storage(StorageError::Serialization(_)) => ApplicationError::new(
            ApplicationErrorCode::InternalError,
            "Runtime data could not be encoded",
        ),
        OpenWorkCoreError::Session(SessionError::ActorStopped) => ApplicationError::new(
            ApplicationErrorCode::InternalError,
            "Session runtime stopped unexpectedly",
        ),
        OpenWorkCoreError::DefaultModelMissing(_)
        | OpenWorkCoreError::ModelDisabled(_)
        | OpenWorkCoreError::CredentialReferenceMissing(_)
        | OpenWorkCoreError::CredentialUnavailable(_)
        | OpenWorkCoreError::UnsupportedProvider(_)
        | OpenWorkCoreError::RuntimeComponent(_) => ApplicationError::new(
            ApplicationErrorCode::ConfigurationInvalid,
            error.to_string(),
        ),
    }
}
