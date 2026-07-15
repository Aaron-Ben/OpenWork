use openwork_persistence::SessionError;
use openwork_protocol::{
    journal::EventJournalError, provider::ProviderRepositoryError, trace::TraceRepositoryError,
};
use serde::Serialize;
use thiserror::Error;

use crate::{AgentError, ChatRuntimeError, TurnSupervisorError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationErrorCode {
    InvalidRequest,
    ProviderNotFound,
    SessionNotFound,
    TurnNotFound,
    ApprovalNotFound,
    DatabaseUnavailable,
    SchemaNotReady,
    ConfigurationInvalid,
    OperationConflict,
    ModelRequestFailed,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ApplicationError {
    code: ApplicationErrorCode,
    message: String,
}

impl ApplicationError {
    pub fn code(&self) -> ApplicationErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn new(code: ApplicationErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<ProviderRepositoryError> for ApplicationError {
    fn from(error: ProviderRepositoryError) -> Self {
        match error {
            ProviderRepositoryError::NotFound { id } => Self::new(
                ApplicationErrorCode::ProviderNotFound,
                format!("Provider not found: {id}"),
            ),
            ProviderRepositoryError::CannotDeleteActive { id } => Self::new(
                ApplicationErrorCode::OperationConflict,
                format!("Cannot delete the active provider: {id}"),
            ),
            ProviderRepositoryError::InvalidInput { field } => Self::new(
                ApplicationErrorCode::InvalidRequest,
                format!("Provider field is invalid: {field}"),
            ),
            ProviderRepositoryError::CredentialEncryption { .. } => Self::new(
                ApplicationErrorCode::ConfigurationInvalid,
                "Provider credential encryption is not configured correctly",
            ),
            ProviderRepositoryError::Persistence { .. } => Self::new(
                ApplicationErrorCode::DatabaseUnavailable,
                "Provider persistence is unavailable",
            ),
        }
    }
}

impl From<SessionError> for ApplicationError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::NotFound { id } => Self::new(
                ApplicationErrorCode::SessionNotFound,
                format!("Session not found: {id}"),
            ),
            SessionError::TurnNotFound { id } => Self::new(
                ApplicationErrorCode::TurnNotFound,
                format!("Turn not found: {id}"),
            ),
            SessionError::TurnAlreadyTerminal { id } => Self::new(
                ApplicationErrorCode::OperationConflict,
                format!("Turn is already terminal: {id}"),
            ),
            SessionError::InvalidEvent { message } => {
                Self::new(ApplicationErrorCode::InvalidRequest, message)
            }
            SessionError::Journal(error) => error.into(),
            SessionError::Serialize(_) => Self::new(
                ApplicationErrorCode::InternalError,
                "Session data could not be encoded",
            ),
        }
    }
}

impl From<EventJournalError> for ApplicationError {
    fn from(error: EventJournalError) -> Self {
        match error {
            EventJournalError::VersionConflict { .. }
            | EventJournalError::DuplicateEvent { .. } => Self::new(
                ApplicationErrorCode::OperationConflict,
                "The stored state changed while the operation was running",
            ),
            EventJournalError::InvalidEvent { message } => {
                Self::new(ApplicationErrorCode::InvalidRequest, message)
            }
            EventJournalError::Persistence { .. } => Self::new(
                ApplicationErrorCode::DatabaseUnavailable,
                "Event journal persistence is unavailable",
            ),
        }
    }
}

impl From<TraceRepositoryError> for ApplicationError {
    fn from(error: TraceRepositoryError) -> Self {
        match error {
            TraceRepositoryError::InvalidSpan { message } => {
                Self::new(ApplicationErrorCode::InvalidRequest, message)
            }
            TraceRepositoryError::Persistence { .. } => Self::new(
                ApplicationErrorCode::DatabaseUnavailable,
                "Trace persistence is unavailable",
            ),
        }
    }
}

impl From<TurnSupervisorError> for ApplicationError {
    fn from(error: TurnSupervisorError) -> Self {
        match error {
            TurnSupervisorError::DuplicateTurn(turn_id) => Self::new(
                ApplicationErrorCode::OperationConflict,
                format!("Turn is already active: {turn_id}"),
            ),
            TurnSupervisorError::TurnNotFound(turn_id) => Self::new(
                ApplicationErrorCode::TurnNotFound,
                format!("Turn not found: {turn_id}"),
            ),
            TurnSupervisorError::LockPoisoned => Self::new(
                ApplicationErrorCode::InternalError,
                "Turn routing state is unavailable",
            ),
            TurnSupervisorError::Approval(error) => {
                Self::new(ApplicationErrorCode::ApprovalNotFound, error.to_string())
            }
        }
    }
}

impl From<AgentError> for ApplicationError {
    fn from(error: AgentError) -> Self {
        match error {
            AgentError::Provider(error) => {
                Self::new(ApplicationErrorCode::ModelRequestFailed, error.to_string())
            }
            AgentError::Capability(error) => Self::new(
                ApplicationErrorCode::ConfigurationInvalid,
                error.to_string(),
            ),
            AgentError::Record(error) => {
                Self::new(ApplicationErrorCode::DatabaseUnavailable, error.to_string())
            }
            AgentError::InvalidRecovery(message) => {
                Self::new(ApplicationErrorCode::OperationConflict, message)
            }
            AgentError::MaxStepsExceeded(limit) => Self::new(
                ApplicationErrorCode::OperationConflict,
                format!("Agent exceeded the maximum number of steps: {limit}"),
            ),
            AgentError::Cancelled(_) => Self::new(
                ApplicationErrorCode::OperationConflict,
                "Turn was cancelled",
            ),
            AgentError::DoomLoop(tool, _) => Self::new(
                ApplicationErrorCode::OperationConflict,
                format!("Doom loop detected for tool: {tool}"),
            ),
        }
    }
}

impl From<ChatRuntimeError> for ApplicationError {
    fn from(error: ChatRuntimeError) -> Self {
        match error {
            ChatRuntimeError::CapabilityCatalog(error) => Self::new(
                ApplicationErrorCode::ConfigurationInvalid,
                error.to_string(),
            ),
            ChatRuntimeError::ProviderNotFound(id) => Self::new(
                ApplicationErrorCode::ProviderNotFound,
                format!("Provider not found: {id}"),
            ),
            ChatRuntimeError::SessionNotFound(id) => Self::new(
                ApplicationErrorCode::SessionNotFound,
                format!("Session not found: {id}"),
            ),
            ChatRuntimeError::ProviderRepository(error) => error.into(),
            ChatRuntimeError::Session(error) => error.into(),
            ChatRuntimeError::Agent(error) => error.into(),
            ChatRuntimeError::TurnSupervisor(error) => error.into(),
            ChatRuntimeError::PendingApprovalNotFound(id) => Self::new(
                ApplicationErrorCode::ApprovalNotFound,
                format!("Pending approval not found: {id}"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use openwork_protocol::provider::ProviderRepositoryError;

    use super::{ApplicationError, ApplicationErrorCode};

    #[test]
    fn repository_details_are_mapped_to_a_safe_stable_error() {
        let error = ApplicationError::from(ProviderRepositoryError::Persistence {
            message: "postgres://user:secret@localhost/openwork".to_string(),
        });

        assert_eq!(error.code(), ApplicationErrorCode::DatabaseUnavailable);
        assert_eq!(error.message(), "Provider persistence is unavailable");
        assert!(!error.message().contains("secret"));
    }
}
