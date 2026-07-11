use async_trait::async_trait;

use super::{
    ActionInvokeError, ActionRequest, CapabilityResolveError, CapabilitySpec, Observation,
};

/// 发现并解析可提供给模型的能力声明。
#[async_trait]
pub trait CapabilityResolverPort: Send + Sync {
    async fn list(&self) -> Result<Vec<CapabilitySpec>, CapabilityResolveError>;

    async fn resolve(&self, name: &str) -> Result<Option<CapabilitySpec>, CapabilityResolveError>;
}

/// 已通过 Execution 前置检查后的底层 Action 调用入口。
#[async_trait]
pub trait ActionInvoker: Send + Sync {
    async fn invoke(&self, request: ActionRequest) -> Result<Observation, ActionInvokeError>;
}

/// Core/Agent 唯一可见的安全执行边界。
#[async_trait]
pub trait ExecutionPort: Send + Sync {
    async fn execute(&self, request: ActionRequest) -> Observation;
}
