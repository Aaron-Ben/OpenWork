//! 模型合同、Provider 配置、模型目录、Responses 线协议与传输。

pub mod catalog;
pub mod model;
pub mod provider;

mod adapters;
mod factory;
mod gateway;
mod transport;

pub(crate) use adapters::error;
pub(crate) use transport::HttpProviderConfig;
pub(crate) use transport::sse;

pub use factory::ProviderFactory;
pub use gateway::RetryPolicy;
pub(crate) use gateway::RetryingModelPort;
pub use transport::HttpTransport;
