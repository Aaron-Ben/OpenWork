//! Application commands, supervisors, and composition root.

mod application;
mod cancel;
mod chat;
mod error;
mod provider_service;
mod thread_service;
mod turn_service;
mod turn_supervisor;

pub use application::{ApplicationBootstrapError, ApplicationConfig, OpenWorkApplication};
pub use chat::{
    ChatGenerateRequest, ChatGenerateResponse, ChatRuntimeError, TurnLiveEvent, TurnLiveEventKind,
};
pub use error::{ApplicationError, ApplicationErrorCode};
pub use openwork_core::{Agent, AgentConfig, AgentError, AgentEvent, RunResult};
pub use openwork_persistence::{Session, SessionInput, SessionLoadResult, SessionSummary};
pub use openwork_protocol::approval::{ApprovalPolicy, ApprovalResolution, ResolveApproval};
pub use openwork_protocol::provider::{ProviderInput, ProviderProfile};
pub use provider_service::{
    ProviderApplicationService, ProviderIndex, ProviderPreset, ProviderPresetModel,
    ProviderTestResult,
};
pub use thread_service::ThreadApplicationService;
pub use turn_service::TurnApplicationService;
pub use turn_supervisor::{TurnSupervisor, TurnSupervisorError};

pub(crate) use cancel::RequestCancelRegistry;
pub(crate) use chat::ChatRuntime;
