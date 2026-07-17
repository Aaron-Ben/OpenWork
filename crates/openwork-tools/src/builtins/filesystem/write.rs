use crate::policy::AccessKind;
use crate::{Observation, ObservationErrorCode};
use async_trait::async_trait;
use serde_json::Value;

use crate::ExecutionContext;
use crate::handler::ActionHandler;

use super::resolve;

#[derive(Default)]
pub struct Write;

#[async_trait]
impl ActionHandler for Write {
    fn name(&self) -> &'static str {
        "write"
    }

    async fn invoke(&self, input: Value, ctx: &ExecutionContext) -> Observation {
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return invalid_arguments("missing or invalid 'path' argument");
        };
        let Some(content) = input.get("content").and_then(Value::as_str) else {
            return invalid_arguments("missing or invalid 'content' argument");
        };
        let resolved = resolve(&ctx.working_directory, path);
        if let Err(message) = ctx.check_path(&resolved, AccessKind::Write) {
            return Observation::denied(message);
        }
        if let Some(parent) = resolved.parent()
            && let Err(err) = tokio::fs::create_dir_all(parent).await
        {
            return execution_failed(format!("failed to create parent dirs: {err}"));
        }
        match tokio::fs::write(&resolved, content).await {
            Ok(_) => Observation::succeeded(format!(
                "wrote {} bytes to {}",
                content.len(),
                resolved.display()
            )),
            Err(err) => execution_failed(format!("failed to write {}: {err}", resolved.display())),
        }
    }
}

fn invalid_arguments(message: impl Into<String>) -> Observation {
    Observation::failed(ObservationErrorCode::InvalidArguments, message, false)
}

fn execution_failed(message: impl Into<String>) -> Observation {
    Observation::failed(ObservationErrorCode::ExecutionFailed, message, false)
}
