//! OpenWork collaboration daemon, persistence, scheduling, and local protocols.

pub mod activity;
pub mod autonomy;
mod card_events;
pub mod claim_reaper;
pub mod coordination;
pub mod daemon;
pub mod domain;
pub mod event;
pub mod home;
pub mod mcp;
pub mod migration;
pub mod model;
pub mod opencode;
pub mod permission;
pub mod proactivity;
mod runtime_events;
pub mod scheduler;
pub mod storage;
pub mod time;
pub mod triage;
mod wake_triage;
