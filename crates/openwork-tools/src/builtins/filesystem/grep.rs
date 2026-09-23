use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use globset::GlobMatcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use schemars::JsonSchema;
use serde::Deserialize;

use super::scan::{
    Interrupt, SCAN_TIMEOUT, ScanBudget, compile_glob, display_path, relative_to_root,
};
use crate::context::PathIntent;
use crate::policy::AccessKind;
use crate::spill::{SpillFile, SpillWriter};
use crate::{
    AnalysisUnit, AsyncFileSystem, Effect, InvocationAnalysis, TextToolOutput, Tool,
    ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

/// Matching lines returned in `content` mode (tools.md §9 grep).
const MAX_LINES: usize = 250;
/// One large file cannot take the whole result.
const MAX_LINES_PER_FILE: usize = 50;
const MAX_LINE_BYTES: usize = 2000;
/// Entries returned in `files_with_matches` and `count` modes.
const MAX_FILES: usize = 250;
/// Memory the searcher may use to hold one line. A longer line fails that
/// file's search, which is then skipped like an unreadable file.
const MAX_LINE_BUFFER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum GrepOutputMode {
    #[default]
    Content,
    FilesWithMatches,
    Count,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrepInput {
    /// Regular expression to search for (Rust regex syntax).
    pub pattern: String,
    /// Directory or file to search; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
    /// Optional glob, relative to `path`, that files must match, for example `*.rs`.
    #[serde(default)]
    pub glob: Option<String>,
    /// Output shape; defaults to `content`.
    #[serde(default)]
    pub output_mode: GrepOutputMode,
}

#[derive(Debug, Default)]
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    type Input = GrepInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("grep")
    }

    fn description(&self) -> &'static str {
        "Search file contents with a regular expression (ripgrep; respects .gitignore, skips binary files). `content` mode groups matching lines by file: at most 250 lines, 50 per file. `files_with_matches` lists matching files; `count` gives matches per file (at most 250 files each). The exact totals are always reported; when results are cut, the full list is saved to a file you can read."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        let display = format!("grep {} in {}", input.pattern, input.path);
        InvocationAnalysis::new(
            display.clone(),
            vec![AnalysisUnit::new(
                display,
                vec![Effect::read(session.normalize_effect_path(&input.path))],
            )],
        )
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: GrepInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let matcher = RegexMatcherBuilder::new()
            .line_terminator(Some(b'\n'))
            .build(&input.pattern)
            .map_err(|error| {
                ToolExecutionError::invalid_arguments(format!(
                    "invalid regex: {error}. Escape metacharacters such as ( [ {{ . * + ? with a backslash to match them literally."
                ))
            })?;
        let glob = input.glob.as_deref().map(compile_glob).transpose()?;
        let root = session
            .resolve_tool_path(&input.path, AccessKind::Read, PathIntent::MustExist, &call)
            .await?;
        let workspace = session
            .filesystem
            .canonicalize(&session.working_directory)
            .await
            .unwrap_or_else(|_| session.working_directory.clone());
        let request = ScanRequest {
            filesystem: session.filesystem.clone(),
            root: root.as_path().to_path_buf(),
            workspace,
            matcher,
            glob,
            mode: input.output_mode,
            spill_path: session
                .spill
                .as_ref()
                .map(|spill| spill.file_for(&call.call_id)),
            budget: ScanBudget::new(call.cancel.clone(), SCAN_TIMEOUT),
        };
        let scan = tokio::task::spawn_blocking(move || scan(request))
            .await
            .map_err(|error| ToolExecutionError::execution(format!("grep task failed: {error}")))?;
        if scan.interrupt == Some(Interrupt::Cancelled) {
            return Err(ToolExecutionError::cancelled("grep cancelled"));
        }
        Ok(TextToolOutput::new(scan.render(&input)))
    }
}

fn default_path() -> String {
    ".".to_string()
}

struct ScanRequest {
    filesystem: Arc<dyn AsyncFileSystem>,
    root: PathBuf,
    workspace: PathBuf,
    matcher: RegexMatcher,
    glob: Option<GlobMatcher>,
    mode: GrepOutputMode,
    spill_path: Option<PathBuf>,
    budget: ScanBudget,
}

/// Matches kept for one file; `total` counts every match, kept or not.
struct FileMatches {
    display: String,
    lines: Vec<(u64, String)>,
    total: u64,
}

struct Scan {
    mode: GrepOutputMode,
    files: Vec<FileMatches>,
    kept_lines: usize,
    total_lines: u64,
    total_files: u64,
    interrupt: Option<Interrupt>,
    spill: Option<SpillFile>,
}

fn scan(request: ScanRequest) -> Scan {
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .line_number(true)
        .heap_limit(Some(MAX_LINE_BUFFER_BYTES))
        .build();
    let mut spill = SpillWriter::new(request.spill_path.clone());
    let mut scan = Scan {
        mode: request.mode,
        files: Vec::new(),
        kept_lines: 0,
        total_lines: 0,
        total_files: 0,
        interrupt: None,
        spill: None,
    };
    for entry in request.filesystem.walk_files(&request.root) {
        if let Some(interrupt) = request.budget.check() {
            scan.interrupt = Some(interrupt);
            break;
        }
        // Unreadable directories and files are skipped, as ripgrep does.
        let Ok(entry) = entry else { continue };
        if request
            .glob
            .as_ref()
            .is_some_and(|glob| !glob.is_match(relative_to_root(&request.root, &entry.path)))
        {
            continue;
        }
        let display = display_path(&request.workspace, &entry.path);
        if let Some(file) = search_file(
            &request,
            &mut searcher,
            &entry.path,
            display,
            &scan,
            &mut spill,
        ) {
            scan.record(file, &mut spill);
        }
        // The sink stops a file early when the budget runs out; checking here
        // too catches that in the last file, where the loop would otherwise
        // end as if the scan were complete.
        if let Some(interrupt) = request.budget.check() {
            scan.interrupt = Some(interrupt);
            break;
        }
    }
    let truncated = scan.is_truncated();
    scan.spill = spill.finish(truncated);
    scan
}

/// Searches one file. Returns `None` when it has no match or cannot be read.
fn search_file(
    request: &ScanRequest,
    searcher: &mut Searcher,
    path: &Path,
    display: String,
    scan: &Scan,
    spill: &mut SpillWriter,
) -> Option<FileMatches> {
    let reader = request.filesystem.open_reader(path).ok()?;
    let room = MAX_LINES
        .saturating_sub(scan.kept_lines)
        .min(MAX_LINES_PER_FILE);
    let mut file = FileMatches {
        display,
        lines: Vec::new(),
        total: 0,
    };
    let result = searcher.search_reader(
        &request.matcher,
        reader,
        Lossy(|number, line| {
            file.total += 1;
            match request.mode {
                GrepOutputMode::Content => {
                    let line = bounded_line(line);
                    spill.push(format!("{}:{number}:{line}\n", file.display).as_bytes());
                    if file.lines.len() < room {
                        file.lines.push((number, line));
                    }
                }
                // Only whether the file matches matters here.
                GrepOutputMode::FilesWithMatches => return Ok(false),
                GrepOutputMode::Count => {}
            }
            Ok(request.budget.check().is_none())
        }),
    );
    if result.is_err() || file.total == 0 {
        return None;
    }
    Some(file)
}

fn bounded_line(line: &str) -> String {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.len() <= MAX_LINE_BYTES {
        return line.to_string();
    }
    let mut end = MAX_LINE_BYTES;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}... (line truncated to {MAX_LINE_BYTES} bytes)",
        &line[..end]
    )
}

impl Scan {
    fn record(&mut self, file: FileMatches, spill: &mut SpillWriter) {
        self.total_files += 1;
        self.total_lines += file.total;
        match self.mode {
            GrepOutputMode::Content => {
                self.kept_lines += file.lines.len();
                if !file.lines.is_empty() {
                    self.files.push(file);
                }
            }
            GrepOutputMode::FilesWithMatches => {
                spill.push(format!("{}\n", file.display).as_bytes());
                if self.files.len() < MAX_FILES {
                    self.files.push(file);
                }
            }
            GrepOutputMode::Count => {
                spill.push(format!("{}:{}\n", file.display, file.total).as_bytes());
                if self.files.len() < MAX_FILES {
                    self.files.push(file);
                }
            }
        }
    }

    fn is_truncated(&self) -> bool {
        match self.mode {
            GrepOutputMode::Content => self.total_lines > self.kept_lines as u64,
            GrepOutputMode::FilesWithMatches | GrepOutputMode::Count => {
                self.total_files > self.files.len() as u64
            }
        }
    }

    fn render(&self, input: &GrepInput) -> String {
        let timed_out = self.interrupt == Some(Interrupt::TimedOut);
        if self.total_files == 0 {
            let scope = match &input.glob {
                Some(glob) => format!("{} (files matching {glob})", input.path),
                None => input.path.clone(),
            };
            return if timed_out {
                format!(
                    "No matches for /{}/ in {scope} before the search timed out after {}s — narrow the path or glob",
                    input.pattern,
                    SCAN_TIMEOUT.as_secs()
                )
            } else {
                format!("No matches for /{}/ in {scope}", input.pattern)
            };
        }
        let mut text = match self.mode {
            GrepOutputMode::Content => self.render_content(),
            GrepOutputMode::FilesWithMatches => self
                .files
                .iter()
                .map(|file| file.display.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            GrepOutputMode::Count => self
                .files
                .iter()
                .map(|file| format!("{}:{}", file.display, file.total))
                .collect::<Vec<_>>()
                .join("\n"),
        };
        if let Some(footer) = self.footer(timed_out) {
            text.push_str("\n\n");
            text.push_str(&footer);
        }
        text
    }

    fn render_content(&self) -> String {
        self.files
            .iter()
            .map(|file| {
                let mut group = file.display.clone();
                for (number, line) in &file.lines {
                    group.push_str(&format!("\n{number}:{line}"));
                }
                let hidden = file.total - file.lines.len() as u64;
                if hidden > 0 {
                    group.push_str(&format!("\n(+{hidden} more matching lines in this file)"));
                }
                group
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn footer(&self, timed_out: bool) -> Option<String> {
        let saved = match &self.spill {
            Some(spill) if spill.capped => format!(
                "; the first 64 MB of the full list is saved to {}",
                spill.path.display()
            ),
            Some(spill) => format!("; full list saved to {}", spill.path.display()),
            None => String::new(),
        };
        let totals = match self.mode {
            GrepOutputMode::Content => format!(
                "{} matching lines in {} files",
                self.total_lines, self.total_files
            ),
            GrepOutputMode::FilesWithMatches | GrepOutputMode::Count => {
                format!("{} files", self.total_files)
            }
        };
        let shown = match self.mode {
            GrepOutputMode::Content => self.kept_lines,
            GrepOutputMode::FilesWithMatches | GrepOutputMode::Count => self.files.len(),
        };
        if timed_out {
            return Some(format!(
                "[at least {totals}; search timed out after {}s — narrow the path or glob{saved}]",
                SCAN_TIMEOUT.as_secs()
            ));
        }
        self.is_truncated()
            .then(|| format!("[showing {shown} of {totals}{saved}]"))
    }
}

#[cfg(test)]
mod tests {
    use tokio_util::sync::CancellationToken;

    use super::super::test_support::TestDirectory;
    use super::*;
    use crate::{PermissionProfile, SpillDirectory, ToolCallId, ToolErrorCode, ToolOutput};

    fn session(workspace: &TestDirectory) -> ToolSessionContext {
        ToolSessionContext::local(
            workspace.path().to_path_buf(),
            PermissionProfile::from_builtin_rules(workspace.path().to_path_buf()),
        )
    }

    fn input(pattern: &str, mode: GrepOutputMode) -> GrepInput {
        GrepInput {
            pattern: pattern.to_string(),
            path: ".".to_string(),
            glob: None,
            output_mode: mode,
        }
    }

    async fn grep(session: &ToolSessionContext, input: GrepInput) -> String {
        GrepTool
            .execute(
                session,
                ToolCallContext::new(ToolCallId::new("grep"), CancellationToken::new()),
                input,
            )
            .await
            .expect("grep result")
            .into_tool_result()
            .text_content()
    }

    #[tokio::test]
    async fn rejects_an_invalid_regex_with_a_hint() {
        let workspace = TestDirectory::new("grep-invalid");
        let error = GrepTool
            .execute(
                &session(&workspace),
                ToolCallContext::new(ToolCallId::new("grep"), CancellationToken::new()),
                input("fn(", GrepOutputMode::Content),
            )
            .await
            .expect_err("invalid regex");
        assert_eq!(error.code, ToolErrorCode::InvalidArguments);
        assert!(error.message.contains("backslash"));
    }

    #[tokio::test]
    async fn groups_matches_by_workspace_relative_file() {
        let workspace = TestDirectory::new("grep-groups");
        std::fs::create_dir(workspace.path().join("src")).expect("create src");
        std::fs::write(
            workspace.path().join("src/main.rs"),
            "needle\nhay\nneedle\n",
        )
        .expect("write fixture");
        std::fs::write(workspace.path().join("notes.txt"), "no match\n").expect("write fixture");

        let text = grep(
            &session(&workspace),
            GrepInput {
                path: "src".to_string(),
                ..input("needle", GrepOutputMode::Content)
            },
        )
        .await;

        assert_eq!(text, "src/main.rs\n1:needle\n3:needle");
    }

    #[tokio::test]
    async fn applies_glob_to_paths_relative_to_the_search_root() {
        let workspace = TestDirectory::new("grep-relative-glob");
        std::fs::create_dir(workspace.path().join("src")).expect("create src");
        std::fs::write(workspace.path().join("src/main.rs"), "needle\n").expect("write fixture");
        std::fs::write(workspace.path().join("main.txt"), "needle\n").expect("write fixture");

        let text = grep(
            &session(&workspace),
            GrepInput {
                glob: Some("src/*.rs".to_string()),
                ..input("needle", GrepOutputMode::Content)
            },
        )
        .await;

        assert_eq!(text, "src/main.rs\n1:needle");
    }

    #[tokio::test]
    async fn skips_binary_files() {
        let workspace = TestDirectory::new("grep-binary");
        std::fs::write(workspace.path().join("blob.bin"), b"needle\x00\x01").expect("write");
        let text = grep(
            &session(&workspace),
            input("needle", GrepOutputMode::Content),
        )
        .await;
        assert!(text.starts_with("No matches"));
    }

    /// tools.md §12 #22: at most 250 lines, 50 per file and 2000 bytes per
    /// line, with exact totals and the full list on disk.
    #[tokio::test]
    async fn acc_22_bounds_lines_and_reports_exact_totals() {
        let workspace = TestDirectory::new("grep-bounds");
        for file in 0..10 {
            let content = (0..100)
                .map(|line| format!("needle {file} {line}\n"))
                .collect::<String>();
            std::fs::write(workspace.path().join(format!("f{file}.txt")), content)
                .expect("write fixture");
        }
        std::fs::write(
            workspace.path().join("wide.txt"),
            format!("needle {}\n", "w".repeat(5000)),
        )
        .expect("write fixture");
        let spill_root = workspace.path().join(".spill");
        let session = session(&workspace).with_spill_directory(SpillDirectory::new(&spill_root));

        let text = grep(&session, input("needle", GrepOutputMode::Content)).await;

        let shown = text
            .lines()
            .filter(|line| line.contains(":needle "))
            .collect::<Vec<_>>();
        assert_eq!(shown.len(), MAX_LINES);
        assert!(text.contains("(+50 more matching lines in this file)"));
        assert!(shown.iter().all(|line| line.len() <= MAX_LINE_BYTES + 60));
        let saved = spill_root.join("grep.txt");
        assert!(text.ends_with(&format!(
            "[showing 250 of 1001 matching lines in 11 files; full list saved to {}]",
            saved.display()
        )));
        let full = std::fs::read_to_string(saved).expect("spill file");
        assert_eq!(full.lines().count(), 1001);
    }

    #[tokio::test]
    async fn file_modes_report_exact_totals() {
        let workspace = TestDirectory::new("grep-file-modes");
        std::fs::write(workspace.path().join("a.txt"), "x\nx\n").expect("write fixture");
        std::fs::write(workspace.path().join("b.txt"), "x\n").expect("write fixture");

        let files = grep(
            &session(&workspace),
            input("x", GrepOutputMode::FilesWithMatches),
        )
        .await;
        let mut listed = files.lines().collect::<Vec<_>>();
        listed.sort();
        assert_eq!(listed, ["a.txt", "b.txt"]);

        let counts = grep(&session(&workspace), input("x", GrepOutputMode::Count)).await;
        let mut listed = counts.lines().collect::<Vec<_>>();
        listed.sort();
        assert_eq!(listed, ["a.txt:2", "b.txt:1"]);
    }

    #[tokio::test]
    async fn untruncated_results_leave_no_spill_file() {
        let workspace = TestDirectory::new("grep-no-spill");
        std::fs::write(workspace.path().join("a.txt"), "needle\n").expect("write fixture");
        let spill_root = workspace.path().join(".spill");
        let session = session(&workspace).with_spill_directory(SpillDirectory::new(&spill_root));

        grep(&session, input("needle", GrepOutputMode::Content)).await;

        assert!(!spill_root.join("grep.txt").exists());
    }

    /// tools.md §12 #23: a timed-out scan reports what it found as a minimum.
    #[test]
    fn acc_23_timeout_reports_at_least_the_counted_matches() {
        let scan = Scan {
            mode: GrepOutputMode::Content,
            files: vec![FileMatches {
                display: "a.txt".to_string(),
                lines: vec![(1, "needle".to_string())],
                total: 1,
            }],
            kept_lines: 1,
            total_lines: 1,
            total_files: 1,
            interrupt: Some(Interrupt::TimedOut),
            spill: None,
        };
        assert_eq!(
            scan.render(&input("needle", GrepOutputMode::Content)),
            "a.txt\n1:needle\n\n[at least 1 matching lines in 1 files; search timed out after 30s — narrow the path or glob]"
        );
    }

    #[tokio::test]
    async fn observes_cancellation() {
        let workspace = TestDirectory::new("grep-cancel");
        std::fs::write(workspace.path().join("a.txt"), "needle\n").expect("write fixture");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = GrepTool
            .execute(
                &session(&workspace),
                ToolCallContext::new(ToolCallId::new("grep"), cancel),
                input("needle", GrepOutputMode::Content),
            )
            .await
            .expect_err("cancelled");
        assert_eq!(error.code, ToolErrorCode::Cancelled);
    }
}
