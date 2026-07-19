/// Ephemeral progress emitted while a tool call is still running.
///
/// Progress is best-effort observation data. It is not part of the terminal
/// [`crate::ToolResult`] and callers must not rely on it for correctness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProgress {
    Stdout { chunk: String },
    Stderr { chunk: String },
    Message { message: String },
}
