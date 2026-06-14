use anvil_core::ai::ToolDefinition;

use crate::tool::{Tool, ToolContext, ToolError, ToolOutput};

/// 内置工具注册表:按 name 查找、导出工具声明、统一执行入口。
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// 导出所有工具声明,喂给 `GenerateRequest::tools`。
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|tool| tool.definition()).collect()
    }

    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|tool| tool.name()).collect()
    }

    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name() == name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        Ok(tool.execute(input, ctx).await)
    }

    /// 注册 read/write/list/bash 四个内置工具。
    pub fn with_builtin() -> Self {
        let mut registry = Self::new();
        registry.register(Box::<crate::builtin::Read>::default());
        registry.register(Box::<crate::builtin::Write>::default());
        registry.register(Box::<crate::builtin::List>::default());
        registry.register(Box::<crate::builtin::Bash>::default());
        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
