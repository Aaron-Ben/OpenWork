//! 会话持久化 crate:SQLite 存储会话与会话消息(parts 存为 JSON 列)。

mod store;

pub use store::{
    NewMessage, Session, SessionError, SessionInput, SessionLoadResult, SessionMessage,
    SessionStore, SessionSummary,
};
