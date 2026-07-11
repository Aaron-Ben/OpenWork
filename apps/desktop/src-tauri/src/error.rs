use openwork_app::{ApplicationError, ApplicationErrorCode};
use serde::Serialize;

pub type CommandErrorCode = ApplicationErrorCode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: String,
}

impl CommandError {
    pub fn new(code: CommandErrorCode, message: String) -> Self {
        Self { code, message }
    }
}

impl From<ApplicationError> for CommandError {
    fn from(error: ApplicationError) -> Self {
        Self::new(error.code(), error.message().to_string())
    }
}
