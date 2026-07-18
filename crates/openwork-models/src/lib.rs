//! Model contracts, provider profiles, protocol adapters, and transport.

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
