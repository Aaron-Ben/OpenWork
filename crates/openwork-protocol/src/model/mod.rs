//! 厂商无关的模型合同；这是协议类型的唯一所有者。

mod error;
mod event;
mod message;
mod port;
mod request;
mod response;

pub use error::{
    DeliveryState, ModelError, ModelErrorCode, ModelFailurePhase, ProviderError, RetryHint,
};
pub use event::{GenerateStreamEvent, ModelEvent};
pub use message::{
    Base64Source, ContentBlock, DataBlock, DataSource, Message, ProviderOpaqueBlock, Role,
    TextBlock, ThinkingBlock, ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
};
pub use port::{
    ModelCallOptions, ModelPort, ModelStream, ModelTransportObserver, ModelTransportSignal,
    ModelTransportSignalKind,
};
pub use request::{
    GenerateRequest, ModelCapabilities, ModelRequest, ThinkingConfig, ThinkingMode, ToolDefinition,
};
pub use response::{FinishReason, GenerateResponse, ModelResponse, TokenUsage};
