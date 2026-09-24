//! 一次工具调用、以及协作 Engine 进程能读写什么，并让内核强制执行（docs/permissions.md §2–§3、
//! docs/collaboration.md §3.1）。
//!
//! 沙箱模式与四档路径只在这里定义。bash 的 Seatbelt profile 与文件工具的围栏都由
//! [`SandboxPolicy`] 推导，两者不会各说各话。本 crate 不依赖任何其他 OpenWork crate；
//! 审批决定与工具进程都不在这里。

mod backend;
mod confinement;
mod denial;
mod policy;
mod probe;
mod seatbelt;
mod tiers;

pub use backend::{SandboxBackend, SandboxUnavailable, Seatbelt};
pub use confinement::EngineConfinement;
pub use denial::{RunOutcome, classify};
pub use policy::{
    Access, Actor, Denial, GrantError, GrantScope, MAX_GRANTS, PathGrant, PathTier,
    SandboxEnvironment, SandboxMode, SandboxPolicy,
};
pub use probe::{SandboxStatus, probe};
pub use seatbelt::{SANDBOX_EXEC, SeatbeltProfile};
