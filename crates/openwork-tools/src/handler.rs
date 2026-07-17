use async_trait::async_trait;
use serde_json::Value;

use crate::{ToolContext, ToolResult};

#[async_trait]
pub(crate) trait ToolHandler: Send + Sync {
    fn name(&self) -> &'static str;

    async fn invoke(&self, input: Value, context: &ToolContext) -> ToolResult;
}

pub(crate) use ToolHandler as ActionHandler;
