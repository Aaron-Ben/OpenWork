//! Application commands, supervisors, and composition root.

mod cancel;
mod chat;
mod registry;
mod turn_supervisor;

pub use cancel::RequestCancelRegistry;
pub use chat::{
    ChatGenerateRequest, ChatGenerateResponse, ChatRuntime, ChatRuntimeError,
    ChatStreamEventPayload, map_agent_event,
};
pub use openwork_core::{Agent, AgentConfig, AgentError, AgentEvent, RunResult};
pub use openwork_protocol::approval::{ApprovalPolicy, ApprovalResolution, ResolveApproval};
pub use registry::{FallbackRule, ModelRegistry, RegistryConfig, RegistryError};
pub use turn_supervisor::{TurnSupervisor, TurnSupervisorError};
