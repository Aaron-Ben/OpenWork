//! Wire types shared by the Desktop, Collaboration Server, Computer, and Agent shim.

mod agent;
mod computer;
mod desktop;
mod error;
mod events;

pub use agent::*;
pub use computer::*;
pub use desktop::*;
pub use error::*;
pub use events::*;

pub const MESSAGE_BODY_MAX_BYTES: usize = 1024 * 1024;

pub fn entity_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

pub fn request_id() -> String {
    entity_id("req")
}
