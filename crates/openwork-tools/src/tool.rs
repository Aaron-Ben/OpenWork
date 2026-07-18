use async_trait::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    ToolCallContext, ToolDefinition, ToolExecutionError, ToolId, ToolResult, ToolRisk,
    ToolSessionContext,
};

pub trait ToolOutput: Send + 'static {
    fn into_tool_result(self) -> ToolResult;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextToolOutput(String);

impl TextToolOutput {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }
}

impl ToolOutput for TextToolOutput {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::succeeded(self.0)
    }
}

impl ToolOutput for ToolResult {
    fn into_tool_result(self) -> ToolResult {
        self
    }
}

#[async_trait]
pub trait Tool: Send + Sync + 'static {
    type Input: DeserializeOwned + JsonSchema + Send + 'static;
    type Output: ToolOutput;

    fn id(&self) -> ToolId;
    fn description(&self) -> &'static str;
    fn risk(&self) -> ToolRisk;

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: Self::Input,
    ) -> Result<Self::Output, ToolExecutionError>;
}

#[async_trait]
pub(crate) trait DynTool: Send + Sync {
    fn id(&self) -> ToolId;
    fn definition(&self) -> Result<ToolDefinition, String>;
    fn validate(&self, input: &Value) -> Result<(), String>;

    async fn call(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: Value,
    ) -> ToolResult;
}

pub(crate) struct ToolAdapter<T: Tool> {
    inner: T,
}

impl<T: Tool> ToolAdapter<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<T: Tool> DynTool for ToolAdapter<T> {
    fn id(&self) -> ToolId {
        self.inner.id()
    }

    fn definition(&self) -> Result<ToolDefinition, String> {
        let schema = schemars::schema_for!(T::Input);
        let mut input_schema = serde_json::to_value(schema)
            .map_err(|error| format!("failed to serialize input schema: {error}"))?;
        if let Some(object) = input_schema.as_object_mut() {
            object.remove("$schema");
            object.remove("title");
        }
        Ok(ToolDefinition {
            id: self.inner.id(),
            description: self.inner.description().to_string(),
            input_schema,
            risk_hint: self.inner.risk(),
        })
    }

    fn validate(&self, input: &Value) -> Result<(), String> {
        serde_json::from_value::<T::Input>(input.clone())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn call(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: Value,
    ) -> ToolResult {
        let input = match serde_json::from_value::<T::Input>(input) {
            Ok(input) => input,
            Err(error) => {
                return ToolResult::failed(
                    crate::ToolErrorCode::InvalidArguments,
                    format!("invalid tool input: {error}"),
                    false,
                );
            }
        };
        match self.inner.execute(session, call, input).await {
            Ok(output) => output.into_tool_result(),
            Err(error) => ToolResult::from_execution_error(error),
        }
    }
}
