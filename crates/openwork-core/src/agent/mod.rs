//! Control plane for sub-agents.
//!
//! Not to be confused with the `openwork-agent` crate, which answers "what is
//! this Agent" as a static definition. This module answers "who may create,
//! address and cap sub-agents at runtime".
//!
//! A sub-agent is a full Session ([`crate::session::SessionHandle`]) with its
//! own Conversation, Turns and Trace. Nothing here duplicates the Agent Loop;
//! [`AgentControl`] only owns creation, the `task_name` index and the
//! concurrency cap. Design: `docs/multi-agent.md`.

mod control;
mod limiter;
mod registry;

pub use control::{AgentControl, AgentControlError, SubAgentHost, SubAgentSpec};
pub use limiter::{DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS, TurnSlot};
pub use registry::{SpawnReservation, SubAgent};
