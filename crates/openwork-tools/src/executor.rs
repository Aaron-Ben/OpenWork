use async_trait::async_trait;

use crate::builtins::{Bash, Edit, Glob, Grep, List, Read, Write};
use crate::handler::ToolHandler;
use crate::{ToolContext, ToolErrorCode, ToolInvocation, ToolResult};

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn invoke(&self, invocation: ToolInvocation) -> ToolResult;
}

pub struct BuiltinToolExecutor {
    context: ToolContext,
    handlers: Vec<Box<dyn ToolHandler>>,
}

impl BuiltinToolExecutor {
    pub fn new(context: ToolContext) -> Self {
        Self {
            context,
            handlers: vec![
                Box::<Read>::default(),
                Box::<Write>::default(),
                Box::<Edit>::default(),
                Box::<Grep>::default(),
                Box::<Glob>::default(),
                Box::<List>::default(),
                Box::<Bash>::default(),
            ],
        }
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.handlers.iter().map(|handler| handler.name()).collect()
    }
}

#[async_trait]
impl ToolExecutor for BuiltinToolExecutor {
    async fn invoke(&self, invocation: ToolInvocation) -> ToolResult {
        let Some(handler) = self
            .handlers
            .iter()
            .find(|handler| handler.name() == invocation.name)
        else {
            return ToolResult::failed(
                ToolErrorCode::HandlerNotFound,
                format!("tool handler not found: {}", invocation.name),
                false,
            );
        };
        handler.invoke(invocation.input, &self.context).await
    }
}
