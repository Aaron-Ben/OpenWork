//! Single-writer conversation actor used by the session runtime.

mod actor;
mod commands;
mod snapshot;
mod state;

pub use actor::ChatStateHandle;
pub use snapshot::{AssistantDraftSnapshot, ConversationSnapshot};
pub use state::ChatStateError;
