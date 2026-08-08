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
mod tool;

pub use control::{AgentControl, AgentControlError, SubAgentHost, SubAgentSpec, SubAgentStatus};
pub use limiter::{DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS, TurnSlot};
pub use registry::{SpawnReservation, SubAgent};
pub(crate) use tool::{
    AGENT_TOOLS, AgentTool, FollowupTaskArgs, InterruptAgentArgs, NoArgs, SpawnAgentArgs,
    WaitAgentArgs, agent_prompt_rules, agent_tool_definitions, parse_args, validate_wait_timeout,
};
