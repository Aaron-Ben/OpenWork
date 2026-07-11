use async_trait::async_trait;
use openwork_protocol::capability::Observation;
use serde_json::Value;

use crate::ExecutionContext;

#[async_trait]
pub(crate) trait ActionHandler: Send + Sync {
    fn name(&self) -> &'static str;

    async fn invoke(&self, input: Value, context: &ExecutionContext) -> Observation;
}
