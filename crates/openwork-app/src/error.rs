use openwork_models::provider::ProviderRepositoryError;
use serde::Serialize;
use thiserror::Error;

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

#[cfg(test)]
mod tests {
    use super::{ApplicationError, ApplicationErrorCode};
    use openwork_models::provider::ProviderRepositoryError;

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
