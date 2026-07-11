//! 厂商无关的 Capability、Action 与 Observation 合同。

mod port;
mod types;

pub use port::{ActionInvoker, CapabilityResolverPort, ExecutionPort};
pub use types::{
    ActionInvokeError, ActionRequest, CapabilityResolveError, CapabilityRiskHint, CapabilitySpec,
    Observation, ObservationContent, ObservationError, ObservationErrorCode, ObservationStatus,
};
