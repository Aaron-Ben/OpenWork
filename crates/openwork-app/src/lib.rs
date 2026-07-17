//! Application composition root and host-facing runtime services.

mod application;
mod error;
mod provider_service;
mod runtime_service;

pub use application::{ApplicationBootstrapError, ApplicationConfig, OpenWorkApplication};
pub use error::{ApplicationError, ApplicationErrorCode};
pub use openwork_core::{
    LoadedSession as RuntimeLoadedSession, ModelInput as RuntimeModelInput,
    SessionInput as RuntimeSessionInput, SessionRecord as RuntimeSessionRecord,
    SessionSnapshot as RuntimeSessionSnapshot, SessionUpdate as RuntimeSessionUpdate,
    SessionUpdateEnvelope as RuntimeSessionUpdateEnvelope, TraceSpanRecord as RuntimeTraceSpan,
    TraceTurnSummary as RuntimeTraceSummary, TurnAccepted as RuntimeTurnAccepted,
};
pub use openwork_models::provider::{ProviderInput, ProviderProfile};
pub use provider_service::{
    ProviderApplicationService, ProviderIndex, ProviderPreset, ProviderPresetModel,
    ProviderTestResult,
};
pub use runtime_service::RuntimeApplicationService;
