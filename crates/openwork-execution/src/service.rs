use std::sync::Arc;

use async_trait::async_trait;
use openwork_protocol::approval::{ApprovalPolicy, ExecutionPolicyDecision};
use openwork_protocol::capability::{
    ActionInvokeError, ActionInvoker, ActionRequest, CapabilityResolverPort, ExecutionPort,
    Observation, ObservationErrorCode,
};

use crate::{policy, schema::validate_input};

pub struct ExecutionService {
    resolver: Arc<dyn CapabilityResolverPort>,
    invoker: Arc<dyn ActionInvoker>,
}

impl ExecutionService {
    pub fn new(resolver: Arc<dyn CapabilityResolverPort>, invoker: Arc<dyn ActionInvoker>) -> Self {
        Self { resolver, invoker }
    }
}

#[async_trait]
impl ExecutionPort for ExecutionService {
    async fn authorize(
        &self,
        request: &ActionRequest,
        approval_policy: ApprovalPolicy,
    ) -> ExecutionPolicyDecision {
        let spec = match self.resolver.resolve(&request.name).await {
            Ok(Some(spec)) => spec,
            Ok(None) => {
                return ExecutionPolicyDecision::Deny {
                    reason: format!("capability not found: {}", request.name),
                };
            }
            Err(error) => {
                return ExecutionPolicyDecision::Deny {
                    reason: error.to_string(),
                };
            }
        };

        if let Err(reason) = validate_input(&spec.input_schema, &request.input) {
            return ExecutionPolicyDecision::Deny { reason };
        }

        policy::evaluate(approval_policy, spec.risk_hint)
    }

    async fn execute(&self, request: ActionRequest) -> Observation {
        let spec = match self.resolver.resolve(&request.name).await {
            Ok(Some(spec)) => spec,
            Ok(None) => {
                return Observation::failed(
                    ObservationErrorCode::CapabilityNotFound,
                    format!("capability not found: {}", request.name),
                    false,
                );
            }
            Err(error) => {
                return Observation::failed(
                    ObservationErrorCode::ExecutionFailed,
                    error.to_string(),
                    true,
                );
            }
        };

        if let Err(message) = validate_input(&spec.input_schema, &request.input) {
            return Observation::failed(ObservationErrorCode::InvalidArguments, message, false);
        }

        match self.invoker.invoke(request).await {
            Ok(observation) => observation,
            Err(ActionInvokeError::HandlerNotFound(name)) => Observation::failed(
                ObservationErrorCode::HandlerNotFound,
                format!("action handler not found: {name}"),
                false,
            ),
            Err(ActionInvokeError::Failed(message)) => {
                Observation::failed(ObservationErrorCode::ExecutionFailed, message, false)
            }
        }
    }
}
