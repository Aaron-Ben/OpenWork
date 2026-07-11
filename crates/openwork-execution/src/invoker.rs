use async_trait::async_trait;
use openwork_protocol::capability::{ActionInvokeError, ActionInvoker, ActionRequest, Observation};

use crate::ExecutionContext;
use crate::actions::{Bash, Edit, Glob, Grep, List, Read, Write};
use crate::handler::ActionHandler;

pub struct BuiltinActionInvoker {
    context: ExecutionContext,
    handlers: Vec<Box<dyn ActionHandler>>,
}

impl BuiltinActionInvoker {
    pub fn new(context: ExecutionContext) -> Self {
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
impl ActionInvoker for BuiltinActionInvoker {
    async fn invoke(&self, request: ActionRequest) -> Result<Observation, ActionInvokeError> {
        let handler = self
            .handlers
            .iter()
            .find(|handler| handler.name() == request.name)
            .ok_or_else(|| ActionInvokeError::HandlerNotFound(request.name.clone()))?;
        Ok(handler.invoke(request.input, &self.context).await)
    }
}
