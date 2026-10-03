use async_trait::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::escalation::ESCALATION_FIELDS;
use crate::{
    CallInspection, ToolCallContext, ToolDefinition, ToolExecutionError, ToolId, ToolResult,
    ToolRisk, ToolSessionContext,
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

    /// 执行前报告给 Core 的事实（permissions.md §1）：命令原文、写目标、越界请求。
    /// 默认是既不写文件也不启动进程的工具。
    fn inspect(&self, _input: &Self::Input) -> CallInspection {
        CallInspection::read_only()
    }

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
    /// `escalation_available` 为假时从 schema 里删掉越界参数（permissions.md §9.2）。
    fn definition(&self, escalation_available: bool) -> Result<ToolDefinition, String>;
    fn validate(&self, input: &Value) -> Result<(), String>;
    fn inspect(&self, input: &Value) -> Result<CallInspection, String>;

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

    fn definition(&self, escalation_available: bool) -> Result<ToolDefinition, String> {
        let schema = schemars::schema_for!(T::Input);
        let mut input_schema = serde_json::to_value(schema)
            .map_err(|error| format!("failed to serialize input schema: {error}"))?;
        if let Some(object) = input_schema.as_object_mut() {
            object.remove("$schema");
            object.remove("title");
            if !escalation_available {
                remove_escalation_fields(object);
            }
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

    fn inspect(&self, input: &Value) -> Result<CallInspection, String> {
        let input =
            serde_json::from_value::<T::Input>(input.clone()).map_err(|error| error.to_string())?;
        Ok(self.inner.inspect(&input))
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

/// 删掉越界参数，以及只被它们引用的类型定义。
fn remove_escalation_fields(schema: &mut serde_json::Map<String, Value>) {
    if let Some(Value::Object(properties)) = schema.get_mut("properties") {
        for field in ESCALATION_FIELDS {
            properties.remove(field);
        }
    }
    let Some(Value::Object(definitions)) = schema.get("$defs").cloned() else {
        return;
    };
    let referenced = |schema: &serde_json::Map<String, Value>, name: &str| {
        let reference = format!("#/$defs/{name}");
        let mut text = Value::Object(schema.clone());
        if let Some(Value::Object(defs)) = text.get_mut("$defs") {
            defs.remove(name);
        }
        text.to_string().contains(&reference)
    };
    let mut remaining = definitions;
    // 定义之间可能互相引用：反复删掉没人引用的，直到不再变化。
    loop {
        let unused = remaining
            .keys()
            .filter(|name| !referenced(schema, name))
            .cloned()
            .collect::<Vec<_>>();
        if unused.is_empty() {
            break;
        }
        for name in unused {
            remaining.remove(&name);
            if let Some(Value::Object(defs)) = schema.get_mut("$defs") {
                defs.remove(&name);
            }
        }
    }
    if remaining.is_empty() {
        schema.remove("$defs");
    }
}
