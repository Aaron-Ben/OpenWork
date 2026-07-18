use openwork_core::{OpenWorkCoreError, SessionError, StorageError};
use openwork_models::provider::ProviderRepositoryError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorCode {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: String,
}

impl CommandError {
    pub fn new(code: CommandErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<OpenWorkCoreError> for CommandError {
    fn from(error: OpenWorkCoreError) -> Self {
        match error {
            OpenWorkCoreError::SessionNotFound(id) => Self::new(
                CommandErrorCode::SessionNotFound,
                format!("Session not found: {id}"),
            ),
            OpenWorkCoreError::ModelNotFound(id) => Self::new(
                CommandErrorCode::ConfigurationInvalid,
                format!("Model not found: {id}"),
            ),
            OpenWorkCoreError::SessionActive(id) => Self::new(
                CommandErrorCode::OperationConflict,
                format!("Session has an active turn: {id}"),
            ),
            OpenWorkCoreError::Session(SessionError::Busy(turn_id)) => Self::new(
                CommandErrorCode::OperationConflict,
                format!("Session already has an active turn: {turn_id}"),
            ),
            OpenWorkCoreError::Session(SessionError::TurnNotActive(turn_id)) => Self::new(
                CommandErrorCode::TurnNotFound,
                format!("Turn is not active: {turn_id}"),
            ),
            OpenWorkCoreError::Session(SessionError::PermissionNotPending(tool_call_id)) => {
                Self::new(
                    CommandErrorCode::ApprovalNotFound,
                    format!("Permission request is not pending: {tool_call_id}"),
                )
            }
            OpenWorkCoreError::Session(SessionError::EmptyInput) => Self::new(
                CommandErrorCode::InvalidRequest,
                "Turn input must not be empty",
            ),
            OpenWorkCoreError::Session(SessionError::ActorStopped) => Self::new(
                CommandErrorCode::InternalError,
                "Session runtime stopped unexpectedly",
            ),
            OpenWorkCoreError::Storage(StorageError::InvalidInput(message)) => {
                Self::new(CommandErrorCode::InvalidRequest, message)
            }
            OpenWorkCoreError::Storage(StorageError::SessionNotFound(id)) => Self::new(
                CommandErrorCode::SessionNotFound,
                format!("Session not found: {id}"),
            ),
            OpenWorkCoreError::Storage(StorageError::TurnNotFound(id)) => Self::new(
                CommandErrorCode::TurnNotFound,
                format!("Turn not found: {id}"),
            ),
            OpenWorkCoreError::Storage(StorageError::Database(_))
            | OpenWorkCoreError::Provider(ProviderRepositoryError::Persistence { .. }) => {
                Self::new(
                    CommandErrorCode::DatabaseUnavailable,
                    "Runtime persistence is unavailable",
                )
            }
            OpenWorkCoreError::Storage(StorageError::Migration(_)) => Self::new(
                CommandErrorCode::SchemaNotReady,
                "Runtime database migration failed",
            ),
            OpenWorkCoreError::Storage(StorageError::Serialization(_)) => Self::new(
                CommandErrorCode::InternalError,
                "Runtime data could not be encoded",
            ),
            OpenWorkCoreError::Provider(ProviderRepositoryError::NotFound { id }) => Self::new(
                CommandErrorCode::ProviderNotFound,
                format!("Provider not found: {id}"),
            ),
            OpenWorkCoreError::Provider(ProviderRepositoryError::InvalidInput { field }) => {
                Self::new(
                    CommandErrorCode::InvalidRequest,
                    format!("Provider field is invalid: {field}"),
                )
            }
            OpenWorkCoreError::Provider(ProviderRepositoryError::CredentialEncryption {
                ..
            })
            | OpenWorkCoreError::CredentialBootstrap(_)
            | OpenWorkCoreError::ProviderRepositoryUnavailable => Self::new(
                CommandErrorCode::ConfigurationInvalid,
                "Provider credential storage is not configured correctly",
            ),
            OpenWorkCoreError::DefaultModelMissing(_)
            | OpenWorkCoreError::ModelDisabled(_)
            | OpenWorkCoreError::CredentialReferenceMissing(_)
            | OpenWorkCoreError::CredentialUnavailable(_)
            | OpenWorkCoreError::UnsupportedProvider(_)
            | OpenWorkCoreError::RuntimeComponent(_) => {
                Self::new(CommandErrorCode::ConfigurationInvalid, error.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use openwork_core::OpenWorkCoreError;
    use openwork_models::provider::ProviderRepositoryError;

    use super::{CommandError, CommandErrorCode};

    #[test]
    fn provider_persistence_details_are_redacted() {
        let error = CommandError::from(OpenWorkCoreError::Provider(
            ProviderRepositoryError::Persistence {
                message: "postgres://user:secret@localhost/openwork".to_string(),
            },
        ));
        assert_eq!(error.code, CommandErrorCode::DatabaseUnavailable);
        assert_eq!(error.message, "Runtime persistence is unavailable");
        assert!(!error.message.contains("secret"));
    }
}
