//! Local macOS collaboration Server, Computer daemon, and wire protocol.

pub mod computer;
#[cfg(target_os = "macos")]
pub mod launchd;
pub mod protocol;
pub mod server;
