//! What one tool call may read and write, and making the kernel enforce it
//! (docs/permissions.md §2–§3).
//!
//! The only definition of the sandbox modes and the four path tiers. Bash's
//! Seatbelt profile and the file tools' fence are both derived from a
//! [`SandboxPolicy`], so the two cannot disagree. No other OpenWork crate is
//! a dependency; approval decisions and tool processes live elsewhere.

mod backend;
mod denial;
mod policy;
mod probe;
mod seatbelt;
mod tiers;

pub use backend::{SandboxBackend, SandboxUnavailable, Seatbelt};
pub use denial::{RunOutcome, classify};
pub use policy::{
    Access, Actor, Denial, GrantError, GrantScope, MAX_GRANTS, PathGrant, PathTier,
    SandboxEnvironment, SandboxMode, SandboxPolicy,
};
pub use probe::{SandboxStatus, probe};
pub use seatbelt::{SANDBOX_EXEC, SeatbeltProfile};
