use crate::policy::AccessKind;
use crate::{Observation, ObservationErrorCode};
use async_trait::async_trait;
use serde_json::Value;

use crate::ExecutionContext;
use crate::handler::ActionHandler;

use super::resolve;

const MAX_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct Read;

#[async_trait]
impl ActionHandler for Read {
    fn name(&self) -> &'static str {
        "read"
    }

    async fn invoke(&self, input: Value, ctx: &ExecutionContext) -> Observation {
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return Observation::failed(
                ObservationErrorCode::InvalidArguments,
                "missing or invalid 'path' argument",
                false,
            );
        };
        let resolved = resolve(&ctx.working_directory, path);
        if let Err(message) = ctx.check_path(&resolved, AccessKind::Read) {
            return Observation::denied(message);
        }
        match tokio::fs::read_to_string(&resolved).await {
            Ok(content) => {
                if content.len() > MAX_BYTES {
                    return Observation::failed(
                        ObservationErrorCode::ExecutionFailed,
                        format!(
                            "file too large: {} bytes (max {})",
                            content.len(),
                            MAX_BYTES
                        ),
                        false,
                    );
                }
                let numbered = content
                    .lines()
                    .enumerate()
                    .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
                    .collect::<Vec<_>>()
                    .join("\n");
                Observation::succeeded(numbered)
            }
            Err(err) => Observation::failed(
                ObservationErrorCode::ExecutionFailed,
                format!("failed to read {}: {err}", resolved.display()),
                false,
            ),
        }
    }
}
