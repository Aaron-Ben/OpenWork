//! Single-writer conversation actor used by the session runtime.

mod actor;
mod commands;
mod item;
mod snapshot;
mod state;
mod view;

pub use actor::ChatStateHandle;
pub use item::{ConversationItem, ConversationItemOrigin, MessageKind, SyntheticReason};
pub use snapshot::{AssistantDraftSnapshot, ConversationSnapshot};
pub use state::ChatStateError;
pub use view::{ConversationCompactionView, ConversationView};
