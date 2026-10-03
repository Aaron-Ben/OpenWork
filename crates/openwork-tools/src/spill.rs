//! Bounded tool results and their spill files (tools.md §8 "大结果落盘").
//!
//! Every model-visible result fits in [`MAX_RESULT_BYTES`]. When a tool has to
//! leave something out, the complete text goes to a per-session spill file and
//! the bounded text names that file, so the model can `read` or `grep` it
//! instead of running the tool again.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::{ToolCallId, ToolResult, ToolResultContent};

/// Upper bound for the text of any tool result, footer included.
///
/// Equals the request projection's per-result cap (8000 tokens at 4 bytes per
/// token), so a bounded result is never cut again in the middle.
pub const MAX_RESULT_BYTES: usize = 32_000;

/// Bytes reserved at the end of a bounded result for its footer.
pub(crate) const FOOTER_RESERVE_BYTES: usize = 512;

/// A spill file stops growing here, so unbounded output (`yes`, a runaway
/// search) cannot fill the disk.
pub const MAX_SPILL_BYTES: u64 = 64 * 1024 * 1024;

/// Kept in memory before the complete text moves to disk.
const SPILL_BUFFER_BYTES: usize = 256 * 1024;

/// The directory that holds one session's spill files.
///
/// It lives under `~/.openwork/spill/<session-id>/`, which is hard-protected:
/// the model can read it but nothing it runs can write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpillDirectory {
    path: PathBuf,
}

impl SpillDirectory {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The directory for one session under the spill root. The session key is
    /// made filename-safe so it cannot point outside `root`.
    pub fn for_session(root: &Path, session_key: &str) -> Self {
        Self::new(root.join(safe_file_name(session_key)))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The spill file for one tool call. Call ids come from the provider, so
    /// anything outside `[A-Za-z0-9_-]` is replaced to keep the name inside
    /// this directory.
    pub(crate) fn file_for(&self, call_id: &ToolCallId) -> PathBuf {
        self.file_for_call(call_id.as_str())
    }

    /// [`Self::file_for`] by raw id, for Core, which refers to spill files
    /// from persisted Tool Results.
    pub fn file_for_call(&self, call_id: &str) -> PathBuf {
        self.path.join(format!("{}.txt", safe_file_name(call_id)))
    }

    /// Writes `content` to the call's spill file.
    ///
    /// Returns `None` when the write fails: the caller must then omit the
    /// path rather than point at a file that cannot be read.
    pub(crate) async fn save(&self, call_id: &ToolCallId, content: &[u8]) -> Option<PathBuf> {
        let path = self.file_for(call_id);
        if tokio::fs::create_dir_all(&self.path).await.is_err() {
            return None;
        }
        match tokio::fs::write(&path, content).await {
            Ok(()) => Some(path),
            Err(_) => {
                let _ = tokio::fs::remove_file(&path).await;
                None
            }
        }
    }
}

/// Keeps `[A-Za-z0-9_-]` and replaces everything else, so an id from a
/// provider or a client can never name a path outside its directory.
fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Collects the complete text behind a bounded listing (grep, glob).
///
/// Small listings stay in memory; a listing that outgrows the buffer moves to
/// its spill file and streams from then on, so memory is bounded whatever the
/// total. Blocking: use it inside `spawn_blocking`.
pub(crate) struct SpillWriter {
    state: SpillState,
    written: u64,
}

enum SpillState {
    Buffering {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    Writing {
        path: PathBuf,
        file: BufWriter<File>,
    },
    Unavailable,
}

/// Where a finished spill ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpillFile {
    pub(crate) path: PathBuf,
    /// The file stopped at [`MAX_SPILL_BYTES`] and misses the rest.
    pub(crate) capped: bool,
}

impl SpillWriter {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            state: match path {
                Some(path) => SpillState::Buffering {
                    path,
                    bytes: Vec::new(),
                },
                None => SpillState::Unavailable,
            },
            written: 0,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let room = MAX_SPILL_BYTES.saturating_sub(self.written);
        let take = bytes.len().min(usize::try_from(room).unwrap_or(usize::MAX));
        self.written = self.written.saturating_add(bytes.len() as u64);
        let bytes = &bytes[..take];
        if bytes.is_empty() {
            return;
        }
        let state = std::mem::replace(&mut self.state, SpillState::Unavailable);
        self.state = match state {
            SpillState::Buffering {
                path,
                bytes: mut bytes_so_far,
            } => {
                bytes_so_far.extend_from_slice(bytes);
                if bytes_so_far.len() <= SPILL_BUFFER_BYTES {
                    SpillState::Buffering {
                        path,
                        bytes: bytes_so_far,
                    }
                } else {
                    open_spill(path, &bytes_so_far)
                }
            }
            SpillState::Writing { path, mut file } => match file.write_all(bytes) {
                Ok(()) => SpillState::Writing { path, file },
                Err(_) => abandon(&path),
            },
            SpillState::Unavailable => SpillState::Unavailable,
        };
    }

    /// Keeps the file when `keep` is true (something was left out of the
    /// bounded text) and discards it otherwise. `None` means there is no
    /// readable file to name.
    pub(crate) fn finish(self, keep: bool) -> Option<SpillFile> {
        let capped = self.written > MAX_SPILL_BYTES;
        let path = match self.state {
            SpillState::Unavailable => return None,
            SpillState::Buffering { .. } if !keep => return None,
            SpillState::Writing { path, .. } if !keep => {
                let _ = std::fs::remove_file(&path);
                return None;
            }
            SpillState::Buffering { path, bytes } => match open_spill(path, &bytes) {
                SpillState::Writing { path, file } => flush(path, file)?,
                _ => return None,
            },
            SpillState::Writing { path, file } => flush(path, file)?,
        };
        Some(SpillFile { path, capped })
    }
}

fn open_spill(path: PathBuf, bytes: &[u8]) -> SpillState {
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return SpillState::Unavailable;
    }
    let Ok(file) = File::create(&path) else {
        return SpillState::Unavailable;
    };
    let mut file = BufWriter::new(file);
    match file.write_all(bytes) {
        Ok(()) => SpillState::Writing { path, file },
        Err(_) => abandon(&path),
    }
}

fn flush(path: PathBuf, mut file: BufWriter<File>) -> Option<PathBuf> {
    match file.flush() {
        Ok(()) => Some(path),
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

fn abandon(path: &Path) -> SpillState {
    let _ = std::fs::remove_file(path);
    SpillState::Unavailable
}

/// The sentence that tells the model where the complete content is.
pub(crate) fn saved_at(path: &Path) -> String {
    format!(
        "Full output saved at {} — use read with offset/limit, or grep, to look at it.",
        path.display()
    )
}

/// Last-resort bound applied to every tool result after the tool returns.
///
/// Built-in tools already bound their own output; this catches the rest (for
/// example a long error message) so no result exceeds [`MAX_RESULT_BYTES`].
/// The text is cut only at the end, on a line boundary where possible.
pub(crate) async fn bound_result(
    mut result: ToolResult,
    spill: Option<&SpillDirectory>,
    call_id: &ToolCallId,
) -> ToolResult {
    let text = result.text_content();
    if text.len() <= MAX_RESULT_BYTES {
        return result;
    }
    let saved = match spill {
        Some(spill) => spill.save(call_id, text.as_bytes()).await,
        None => None,
    };
    let kept = &text[..head_boundary(&text, MAX_RESULT_BYTES - FOOTER_RESERVE_BYTES)];
    let omitted = text.len() - kept.len();
    let footer = match saved {
        Some(path) => format!("... ({omitted} bytes omitted. {})", saved_at(&path)),
        None => format!("... ({omitted} bytes omitted.)"),
    };
    result.content = vec![ToolResultContent::Text {
        text: format!("{kept}\n{footer}"),
    }];
    result
}

/// The largest prefix of `text` within `max_bytes`, ending after a newline
/// when one exists in the second half of the budget, otherwise on a char
/// boundary.
pub(crate) fn head_boundary(text: &str, max_bytes: usize) -> usize {
    if text.len() <= max_bytes {
        return text.len();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    match text[..end].rfind('\n') {
        Some(newline) if newline >= end / 2 => newline + 1,
        _ => end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolResultStatus;

    #[test]
    fn file_names_cannot_escape_the_spill_directory() {
        let spill = SpillDirectory::new("/spill/session");
        assert_eq!(
            spill.file_for(&ToolCallId::new("../../etc/passwd")),
            PathBuf::from("/spill/session/______etc_passwd.txt")
        );
        assert_eq!(
            spill.file_for(&ToolCallId::new("call_Ab-9")),
            PathBuf::from("/spill/session/call_Ab-9.txt")
        );
        assert_eq!(
            SpillDirectory::for_session(Path::new("/spill"), "../other").path(),
            Path::new("/spill/___other")
        );
    }

    #[test]
    fn head_boundary_prefers_a_line_end() {
        let text = "aaaa\nbbbb\ncccc";
        assert_eq!(head_boundary(text, 12), 10);
        assert_eq!(head_boundary(text, 100), text.len());
    }

    #[test]
    fn head_boundary_respects_utf8() {
        let text = "中".repeat(10);
        let end = head_boundary(&text, 7);
        assert!(text.is_char_boundary(end));
        assert_eq!(end, 6);
    }

    #[tokio::test]
    async fn oversized_results_are_cut_at_the_end_and_saved() {
        let directory = std::env::temp_dir().join(format!("openwork-spill-{}", std::process::id()));
        let spill = SpillDirectory::new(&directory);
        let text = (0..10_000)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let result = bound_result(
            ToolResult::succeeded(text.clone()),
            Some(&spill),
            &ToolCallId::new("oversized"),
        )
        .await;

        let bounded = result.text_content();
        assert!(bounded.len() <= MAX_RESULT_BYTES);
        assert!(bounded.starts_with("line 0\n"));
        assert!(bounded.contains("bytes omitted. Full output saved at"));
        assert_eq!(result.status, ToolResultStatus::Succeeded);
        let saved = std::fs::read_to_string(directory.join("oversized.txt")).expect("spill file");
        assert_eq!(saved, text);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test]
    async fn failed_spill_omits_the_path() {
        let blocker =
            std::env::temp_dir().join(format!("openwork-spill-file-{}", std::process::id()));
        std::fs::write(&blocker, "not a directory").expect("blocker");
        let spill = SpillDirectory::new(blocker.join("session"));
        let result = bound_result(
            ToolResult::succeeded("x\n".repeat(MAX_RESULT_BYTES)),
            Some(&spill),
            &ToolCallId::new("unwritable"),
        )
        .await;

        let bounded = result.text_content();
        assert!(bounded.len() <= MAX_RESULT_BYTES);
        assert!(bounded.contains("bytes omitted.)"));
        assert!(!bounded.contains("saved at"));
        let _ = std::fs::remove_file(blocker);
    }

    #[tokio::test]
    async fn small_results_are_untouched() {
        let result = bound_result(
            ToolResult::succeeded("short"),
            None,
            &ToolCallId::new("short"),
        )
        .await;
        assert_eq!(result.text_content(), "short");
    }
}
