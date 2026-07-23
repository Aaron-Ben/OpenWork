use openwork_core::{CompactionError, OpenWorkCoreError, SessionError, StorageError};
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
            OpenWorkCoreError::FileChangeNotFound(id) => Self::new(
                CommandErrorCode::InvalidRequest,
                format!("File change not found: {id}"),
            ),
            OpenWorkCoreError::FileChangeAlreadyUndone(id) => Self::new(
                CommandErrorCode::OperationConflict,
                format!("File change has already been undone: {id}"),
            ),
            OpenWorkCoreError::FileChangeNotUndone(id) => Self::new(
                CommandErrorCode::OperationConflict,
                format!("File change has not been undone: {id}"),
            ),
            OpenWorkCoreError::FileChangeUndo(error) => {
                Self::new(CommandErrorCode::OperationConflict, error.to_string())
            }
            OpenWorkCoreError::FileChangeReapply(error) => {
                Self::new(CommandErrorCode::OperationConflict, error.to_string())
            }
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
            OpenWorkCoreError::Compaction(CompactionError::SessionActive(turn_id)) => Self::new(
                CommandErrorCode::OperationConflict,
                format!("Session has an active turn and cannot be compacted: {turn_id}"),
            ),
            OpenWorkCoreError::Compaction(CompactionError::EmptyConversation) => Self::new(
                CommandErrorCode::InvalidRequest,
                "Conversation is empty and cannot be compacted",
            ),
            OpenWorkCoreError::Compaction(CompactionError::Model(error))
            | OpenWorkCoreError::Compaction(CompactionError::Stream(error)) => Self::new(
                CommandErrorCode::ModelRequestFailed,
                format!("Conversation compaction model request failed: {error}"),
            ),
            OpenWorkCoreError::Compaction(CompactionError::InvalidResponse(message)) => Self::new(
                CommandErrorCode::ModelRequestFailed,
                format!("Conversation compaction returned an unusable summary: {message}"),
            ),
            OpenWorkCoreError::Compaction(CompactionError::Persistence(_)) => Self::new(
                CommandErrorCode::DatabaseUnavailable,
                "Conversation compaction could not be persisted",
            ),
            OpenWorkCoreError::Compaction(
                CompactionError::Context(_) | CompactionError::Request(_),
            ) => Self::new(CommandErrorCode::ConfigurationInvalid, error.to_string()),
            OpenWorkCoreError::Compaction(
                CompactionError::MessageCountOverflow
                | CompactionError::MissingResponse
                | CompactionError::DuplicateResponse
                | CompactionError::ChatState(_)
                | CompactionError::ActorStopped,
            ) => Self::new(CommandErrorCode::InternalError, error.to_string()),
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
    use openwork_core::{CompactionError, OpenWorkCoreError, RuntimeTurnId};
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

    #[test]
    fn reapply_of_an_active_change_is_an_operation_conflict() {
        let error = CommandError::from(OpenWorkCoreError::FileChangeNotUndone(
            "change-1".to_string(),
        ));

        assert_eq!(error.code, CommandErrorCode::OperationConflict);
        assert_eq!(error.message, "File change has not been undone: change-1");
    }

    #[test]
    fn active_session_compaction_is_an_operation_conflict() {
        let error = CommandError::from(OpenWorkCoreError::Compaction(
            CompactionError::SessionActive(RuntimeTurnId::new("turn-1")),
        ));

        assert_eq!(error.code, CommandErrorCode::OperationConflict);
        assert_eq!(
            error.message,
            "Session has an active turn and cannot be compacted: turn-1"
        );
    }

    #[test]
    fn compaction_persistence_details_are_redacted() {
        let error = CommandError::from(OpenWorkCoreError::Compaction(
            CompactionError::Persistence("postgres://user:secret@localhost/openwork".to_string()),
        ));

        assert_eq!(error.code, CommandErrorCode::DatabaseUnavailable);
        assert_eq!(
            error.message,
            "Conversation compaction could not be persisted"
        );
        assert!(!error.message.contains("secret"));
    }
}
