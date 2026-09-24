use std::io::{self, BufRead, BufReader};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use sha2::{Digest, Sha256};

use crate::observation::ContentHash;
use crate::spill::{FOOTER_RESERVE_BYTES, MAX_RESULT_BYTES};
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};
use openwork_sandbox::Access;

use crate::checked_path::PathIntent;

/// Lines returned when the model does not ask for fewer (tools.md §9 read).
const MAX_LINES: usize = 2000;
/// Longer lines are cut and marked; minified files would otherwise fill the
/// byte budget with a single line.
const MAX_LINE_CHARS: usize = 2000;
/// Content budget; the rest of [`MAX_RESULT_BYTES`] is left for the footer.
const MAX_CONTENT_BYTES: usize = MAX_RESULT_BYTES - FOOTER_RESERVE_BYTES;
const CANCEL_CHECK_LINES: usize = 4096;
/// Bytes of one line kept in memory: enough for [`MAX_LINE_CHARS`] of any
/// UTF-8. The rest of an overlong line is hashed and dropped as it streams,
/// so a single-line minified file costs no more memory than a short one.
const MAX_LINE_BYTES: usize = MAX_LINE_CHARS * 4;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadInput {
    /// Absolute path, or a path relative to the working directory.
    pub path: String,
    /// 1-based line number to start from. Defaults to 1.
    #[serde(default = "default_offset")]
    pub offset: usize,
    /// Maximum number of lines to return. Defaults to 2000, which is also the maximum.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Default)]
pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    type Input = ReadInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("read")
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 text file. Returns at most 2000 lines or 32 KB, whichever comes first, each line prefixed with its 1-based line number and a tab. Lines longer than 2000 characters are cut. When the file continues, the output ends with the offset to continue from."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: ReadInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        if input.limit == 0 || input.limit > MAX_LINES {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "limit must be between 1 and {MAX_LINES}; omit it to read up to {MAX_LINES} lines"
            )));
        }
        // Models trained on 0-based APIs send 0 for "the start"; treat it as
        // line 1 rather than spend a round trip on an error.
        let offset = input.offset.max(1);
        let resolved = session
            .resolve_path(
                &input.path,
                Access::Read,
                PathIntent::MustExist,
                &call.sandbox_policy,
            )
            .await?;
        let display = resolved.as_path().display().to_string();
        let filesystem = session.filesystem.clone();
        let path = resolved.as_path().to_path_buf();
        let cancel = call.cancel.clone();
        let limit = input.limit;
        let page = tokio::task::spawn_blocking(move || {
            let reader = filesystem.open_reader(&path).map_err(PageError::Io)?;
            read_page(BufReader::new(reader), offset, limit, &cancel)
        })
        .await
        .map_err(|error| ToolExecutionError::execution(format!("read task failed: {error}")))?;
        match page {
            Ok(page) => {
                let hash = page.hash;
                let text = page.render(offset)?;
                session.observations.record(resolved.as_path(), hash);
                Ok(TextToolOutput::new(text))
            }
            Err(PageError::Cancelled) => Err(ToolExecutionError::cancelled("read cancelled")),
            Err(PageError::Binary) => Err(ToolExecutionError::execution(format!(
                "{display} looks like a binary file (it contains NUL bytes); read only returns text. Use bash with a tool such as `file`, `xxd` or `strings` to inspect it."
            ))),
            Err(PageError::InvalidUtf8 { line }) => Err(ToolExecutionError::execution(format!(
                "line {line} of {display} is not valid UTF-8; the file may use another encoding. Use bash with `iconv` or `file` to inspect it."
            ))),
            Err(PageError::Io(error)) => Err(read_error(&display, &error)),
        }
    }
}

fn read_error(display: &str, error: &io::Error) -> ToolExecutionError {
    if error.kind() == io::ErrorKind::IsADirectory {
        return ToolExecutionError::execution(format!(
            "{display} is a directory; use list to see its entries"
        ));
    }
    ToolExecutionError::execution(format!("failed to read {display}: {error}"))
}

fn default_offset() -> usize {
    1
}

fn default_limit() -> usize {
    MAX_LINES
}

#[derive(Debug)]
enum PageError {
    Cancelled,
    Binary,
    InvalidUtf8 { line: usize },
    Io(io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// Everything from the offset to the end of the file was returned.
    End,
    /// The requested number of lines was returned and more follow.
    Lines,
    /// The byte budget ran out before the requested lines did.
    Bytes,
}

#[derive(Debug)]
struct Page {
    text: String,
    /// Last line included in `text`; 0 when nothing was included.
    last: usize,
    total_lines: usize,
    stop: Stop,
    /// Hash of the whole file, whatever part was returned: reading any page
    /// counts as having seen the file (tools.md §9 先读后改).
    hash: ContentHash,
}

impl Page {
    fn render(self, offset: usize) -> Result<String, ToolExecutionError> {
        if self.total_lines == 0 {
            return Ok("[empty file]".to_string());
        }
        if offset > self.total_lines {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "offset {offset} is past the end of the file ({} lines); use an offset between 1 and {}",
                self.total_lines, self.total_lines
            )));
        }
        let mut text = self.text;
        match self.stop {
            Stop::End => {}
            Stop::Lines => text.push_str(&format!(
                "\n[showing lines {offset}-{} of {}. Continue with offset={}]",
                self.last,
                self.total_lines,
                self.last + 1
            )),
            Stop::Bytes => text.push_str(&format!(
                "\n[showing lines {offset}-{} of {}; stopped at 32 KB. Continue with offset={}]",
                self.last,
                self.total_lines,
                self.last + 1
            )),
        }
        Ok(text)
    }
}

/// Collects lines `offset..offset + limit` within the byte budget, then keeps
/// counting to the end so the footer can report the total. Memory stays
/// bounded by the budget whatever the file size.
fn read_page(
    mut reader: impl BufRead,
    offset: usize,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Page, PageError> {
    let mut text = String::new();
    let mut line = Vec::new();
    let mut number = 0usize;
    let mut last = 0usize;
    let mut stop = Stop::End;
    let mut collecting = true;
    let mut hasher = Sha256::new();
    loop {
        let Some(overlong) = next_line(&mut reader, &mut line, &mut hasher)? else {
            break;
        };
        number += 1;
        if number.is_multiple_of(CANCEL_CHECK_LINES) && cancel.is_cancelled() {
            return Err(PageError::Cancelled);
        }
        if !collecting {
            continue;
        }
        if number < offset {
            continue;
        }
        if number - offset == limit {
            stop = Stop::Lines;
            collecting = false;
            continue;
        }
        let formatted = format_line(number, &line, overlong)?;
        let separator = usize::from(!text.is_empty());
        if text.len() + separator + formatted.len() > MAX_CONTENT_BYTES {
            stop = Stop::Bytes;
            collecting = false;
            continue;
        }
        if separator == 1 {
            text.push('\n');
        }
        text.push_str(&formatted);
        last = number;
    }
    Ok(Page {
        text,
        last,
        total_lines: number,
        stop,
        hash: hasher.finalize().into(),
    })
}

/// Reads the next line into `kept` without its terminator, keeping at most
/// [`MAX_LINE_BYTES`]. Every byte, kept or not, goes into `hasher`. Returns
/// `None` at end of file, otherwise whether the line was longer than kept.
fn next_line(
    reader: &mut impl BufRead,
    kept: &mut Vec<u8>,
    hasher: &mut Sha256,
) -> Result<Option<bool>, PageError> {
    kept.clear();
    let mut seen = false;
    let mut overlong = false;
    loop {
        let buffer = reader.fill_buf().map_err(PageError::Io)?;
        if buffer.is_empty() {
            return Ok(seen.then_some(overlong));
        }
        seen = true;
        let (content, consumed, done) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(end) => (&buffer[..end], end + 1, true),
            None => (buffer, buffer.len(), false),
        };
        hasher.update(&buffer[..consumed]);
        let room = MAX_LINE_BYTES.saturating_sub(kept.len());
        kept.extend_from_slice(&content[..content.len().min(room)]);
        overlong |= content.len() > room;
        reader.consume(consumed);
        if done {
            if !overlong && kept.last() == Some(&b'\r') {
                kept.pop();
            }
            return Ok(Some(overlong));
        }
    }
}

fn format_line(number: usize, raw: &[u8], overlong: bool) -> Result<String, PageError> {
    if raw.contains(&0) {
        return Err(PageError::Binary);
    }
    let content = match std::str::from_utf8(raw) {
        Ok(content) => content,
        // Keeping a bounded prefix can split the last character; only that
        // is tolerated, invalid bytes elsewhere are still reported.
        Err(error) if overlong && error.error_len().is_none() => {
            std::str::from_utf8(&raw[..error.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => return Err(PageError::InvalidUtf8 { line: number }),
    };
    match content.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => Ok(format!(
            "{number}\t{}... (line truncated to {MAX_LINE_CHARS} chars)",
            &content[..cut]
        )),
        None if overlong => Ok(format!(
            "{number}\t{content}... (line truncated to {MAX_LINE_CHARS} chars)"
        )),
        None => Ok(format!("{number}\t{content}")),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use tokio_util::sync::CancellationToken;

    use super::super::test_support::TestDirectory;
    use super::*;
    use crate::{ToolErrorCode, ToolOutput};

    fn session(workspace: &std::path::Path) -> ToolSessionContext {
        crate::test_support::unconfined_session(workspace)
    }

    fn input(path: &str) -> ReadInput {
        ReadInput {
            path: path.to_string(),
            offset: default_offset(),
            limit: default_limit(),
        }
    }

    async fn read(
        workspace: &std::path::Path,
        input: ReadInput,
    ) -> Result<String, ToolExecutionError> {
        ReadTool
            .execute(
                &session(workspace),
                crate::test_support::call_context("read", CancellationToken::new()),
                input,
            )
            .await
            .map(|output| output.into_tool_result().text_content())
    }

    /// 读取除凭据目录外处处允许（permissions.md §2.3）；经符号链接进入凭据目录照样被拒。
    #[tokio::test]
    async fn rejects_read_through_symlink_into_a_credential_directory() {
        let sandbox = TestDirectory::new("read-symlink");
        let workspace = sandbox.path().join("workspace");
        let home = sandbox.path().join("home");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(home.join(".ssh")).expect("create credential directory");
        std::fs::write(home.join(".ssh/id_rsa"), "secret").expect("write key");
        std::fs::create_dir_all(sandbox.path().join("elsewhere")).expect("create directory");
        std::fs::write(sandbox.path().join("elsewhere/notes.txt"), "public").expect("write notes");
        symlink(home.join(".ssh"), workspace.join("keys")).expect("create symlink");
        symlink(sandbox.path().join("elsewhere"), workspace.join("other")).expect("create symlink");
        let session = session(&workspace);
        let read_at = |path: &str| {
            ReadTool.execute(
                &session,
                crate::test_support::fenced_call("read", &workspace, &home),
                input(path),
            )
        };

        let error = read_at("keys/id_rsa")
            .await
            .expect_err("credential read must be denied");
        assert_eq!(error.code, ToolErrorCode::PermissionDenied);
        assert!(error.sandbox_denied);
        assert!(
            error.message.starts_with("[sandbox: read access to "),
            "{}",
            error.message
        );

        let text = read_at("other/notes.txt")
            .await
            .expect("reading outside the workspace is allowed")
            .into_tool_result()
            .text_content();
        assert!(text.contains("public"), "{text}");
    }

    #[tokio::test]
    async fn allows_read_through_symlink_that_stays_inside_workspace() {
        let sandbox = TestDirectory::new("read-internal-symlink");
        let workspace = sandbox.path().join("workspace");
        std::fs::create_dir_all(workspace.join("real")).expect("create workspace");
        std::fs::write(workspace.join("real/file.txt"), "inside").expect("write file");
        symlink(workspace.join("real"), workspace.join("alias")).expect("create symlink");

        let text = read(&workspace, input("alias/file.txt"))
            .await
            .expect("internal symlink is allowed");

        assert_eq!(text, "1\tinside");
    }

    #[tokio::test]
    async fn reads_a_one_based_page_with_original_line_numbers() {
        let workspace = TestDirectory::new("read-page");
        std::fs::write(workspace.path().join("page.txt"), "one\ntwo\nthree\nfour\n")
            .expect("write fixture");

        let text = read(
            workspace.path(),
            ReadInput {
                path: "page.txt".to_string(),
                offset: 2,
                limit: 2,
            },
        )
        .await
        .expect("read page");

        assert_eq!(
            text,
            "2\ttwo\n3\tthree\n[showing lines 2-3 of 4. Continue with offset=4]"
        );
    }

    #[tokio::test]
    async fn a_page_that_reaches_the_end_has_no_footer() {
        let workspace = TestDirectory::new("read-end");
        std::fs::write(workspace.path().join("end.txt"), "one\r\ntwo").expect("write fixture");

        let text = read(
            workspace.path(),
            ReadInput {
                path: "end.txt".to_string(),
                offset: 0,
                limit: 2,
            },
        )
        .await
        .expect("read to end");

        assert_eq!(text, "1\tone\n2\ttwo");
    }

    /// tools.md §12 #20: the default read of a 5000-line file stops at line
    /// 2000 or at the last complete line within 32 KB, never in the middle.
    #[tokio::test]
    async fn acc_20_default_read_stops_at_the_first_limit_and_says_how_to_continue() {
        let workspace = TestDirectory::new("read-limits");
        let short = (1..=5000).map(|n| format!("l{n}\n")).collect::<String>();
        std::fs::write(workspace.path().join("short.txt"), &short).expect("write fixture");
        let text = read(workspace.path(), input("short.txt"))
            .await
            .expect("read");
        assert!(text.starts_with("1\tl1\n"));
        assert!(text.contains("\n2000\tl2000\n"));
        assert!(!text.contains("2001\t"));
        assert!(text.ends_with("[showing lines 1-2000 of 5000. Continue with offset=2001]"));

        let wide = (1..=5000)
            .map(|n| format!("{n:05} {}\n", "x".repeat(60)))
            .collect::<String>();
        std::fs::write(workspace.path().join("wide.txt"), &wide).expect("write fixture");
        let text = read(workspace.path(), input("wide.txt"))
            .await
            .expect("read");
        assert!(text.len() <= MAX_RESULT_BYTES);
        let (body, footer) = text.rsplit_once('\n').expect("footer");
        let last_line = body.lines().last().expect("last line");
        let (number, content) = last_line.split_once('\t').expect("numbered line");
        let number = number.parse::<usize>().expect("line number");
        assert_eq!(content, format!("{number:05} {}", "x".repeat(60)));
        assert_eq!(
            footer,
            format!(
                "[showing lines 1-{number} of 5000; stopped at 32 KB. Continue with offset={}]",
                number + 1
            )
        );
    }

    /// tools.md §12 #21.
    #[tokio::test]
    async fn acc_21_long_lines_are_cut_to_2000_chars_and_marked() {
        let workspace = TestDirectory::new("read-long-line");
        std::fs::write(workspace.path().join("long.txt"), "中".repeat(10_000))
            .expect("write fixture");

        let text = read(workspace.path(), input("long.txt"))
            .await
            .expect("read");

        assert_eq!(
            text,
            format!("1\t{}... (line truncated to 2000 chars)", "中".repeat(2000))
        );
    }

    /// A multi-megabyte single line is read in bounded memory, and the
    /// observation still hashes the whole file.
    #[test]
    fn overlong_lines_are_cut_while_streaming() {
        let long = format!("{}\nsecond\r\n", "中".repeat(1_000_000));
        let page = read_page(
            std::io::BufReader::with_capacity(64, long.as_bytes()),
            1,
            MAX_LINES,
            &CancellationToken::new(),
        )
        .expect("page");

        assert_eq!(page.total_lines, 2);
        assert_eq!(
            page.text,
            format!(
                "1\t{}... (line truncated to 2000 chars)\n2\tsecond",
                "中".repeat(2000)
            )
        );
        assert_eq!(page.hash, crate::observation::content_hash(long.as_bytes()));
    }

    #[tokio::test]
    async fn reports_empty_binary_directory_and_past_the_end_actionably() {
        let workspace = TestDirectory::new("read-edge-cases");
        std::fs::write(workspace.path().join("empty.txt"), "").expect("write fixture");
        std::fs::write(workspace.path().join("blob.bin"), b"PK\x00\x01").expect("write fixture");
        std::fs::write(workspace.path().join("latin1.txt"), b"ok\ncaf\xe9\n").expect("write");
        std::fs::write(workspace.path().join("two.txt"), "a\nb\n").expect("write fixture");
        std::fs::create_dir(workspace.path().join("dir")).expect("create dir");

        assert_eq!(
            read(workspace.path(), input("empty.txt"))
                .await
                .expect("empty"),
            "[empty file]"
        );
        let binary = read(workspace.path(), input("blob.bin"))
            .await
            .expect_err("binary");
        assert!(binary.message.contains("binary file"));
        let latin1 = read(workspace.path(), input("latin1.txt"))
            .await
            .expect_err("utf8");
        assert!(latin1.message.contains("line 2"));
        assert!(latin1.message.contains("not valid UTF-8"));
        let directory = read(workspace.path(), input("dir"))
            .await
            .expect_err("directory");
        assert!(directory.message.contains("use list"));
        let past_end = read(
            workspace.path(),
            ReadInput {
                path: "two.txt".to_string(),
                offset: 5,
                limit: 10,
            },
        )
        .await
        .expect_err("past the end");
        assert_eq!(past_end.code, ToolErrorCode::InvalidArguments);
        assert!(past_end.message.contains("between 1 and 2"));
    }
}
