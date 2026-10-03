use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use globset::GlobMatcher;
use schemars::JsonSchema;
use serde::Deserialize;

use super::scan::{
    Interrupt, SCAN_TIMEOUT, ScanBudget, compile_glob, display_path, relative_to_root,
};
use crate::checked_path::PathIntent;
use crate::spill::{SpillFile, SpillWriter};
use crate::{
    AsyncFileSystem, TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk,
    ToolSessionContext,
};
use openwork_sandbox::Access;

/// Paths returned (tools.md §7 glob).
const MAX_RESULTS: usize = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GlobInput {
    /// Glob pattern relative to `path`, for example `**/*.rs` or `src/**/*.ts`.
    pub pattern: String,
    /// Directory to search; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
}

#[derive(Debug, Default)]
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    type Input = GlobInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("glob")
    }

    fn description(&self) -> &'static str {
        "Find files by name pattern (e.g. \"**/*.rs\"). Respects .gitignore. Returns at most 100 paths, most recently modified first, and the exact number of matches; when results are cut, the full list is saved to a file you can read."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: GlobInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let matcher = compile_glob(&input.pattern)?;
        let root = session
            .resolve_path(
                &input.path,
                Access::Read,
                PathIntent::MustExist,
                &call.sandbox_policy,
            )
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
            spill_path: session
                .spill
                .as_ref()
                .map(|spill| spill.file_for(&call.call_id)),
            budget: ScanBudget::new(call.cancel.clone(), SCAN_TIMEOUT),
        };
        let scan = tokio::task::spawn_blocking(move || scan(request))
            .await
            .map_err(|error| ToolExecutionError::execution(format!("glob task failed: {error}")))?;
        if scan.interrupt == Some(Interrupt::Cancelled) {
            return Err(ToolExecutionError::cancelled("glob cancelled"));
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
    matcher: GlobMatcher,
    spill_path: Option<PathBuf>,
    budget: ScanBudget,
}

struct Scan {
    /// Newest first.
    newest: Vec<String>,
    total: u64,
    interrupt: Option<Interrupt>,
    spill: Option<SpillFile>,
}

/// Keeps the [`MAX_RESULTS`] most recently modified matches in a min-heap, so
/// memory does not depend on the number of matches. The full list goes to
/// the spill writer in traversal order.
fn scan(request: ScanRequest) -> Scan {
    let mut heap = BinaryHeap::<Reverse<(SystemTime, String)>>::with_capacity(MAX_RESULTS + 1);
    let mut spill = SpillWriter::new(request.spill_path.clone());
    let mut total = 0u64;
    let mut interrupt = None;
    for entry in request.filesystem.walk_files(&request.root) {
        if let Some(stop) = request.budget.check() {
            interrupt = Some(stop);
            break;
        }
        let Ok(entry) = entry else { continue };
        if !request
            .matcher
            .is_match(relative_to_root(&request.root, &entry.path))
        {
            continue;
        }
        total += 1;
        let display = display_path(&request.workspace, &entry.path);
        spill.push(format!("{display}\n").as_bytes());
        heap.push(Reverse((
            entry.modified.unwrap_or(SystemTime::UNIX_EPOCH),
            display,
        )));
        if heap.len() > MAX_RESULTS {
            heap.pop();
        }
    }
    let newest = heap
        .into_sorted_vec()
        .into_iter()
        .map(|Reverse((_, display))| display)
        .collect::<Vec<_>>();
    let truncated = total > newest.len() as u64;
    Scan {
        newest,
        total,
        interrupt,
        spill: spill.finish(truncated),
    }
}

impl Scan {
    fn render(&self, input: &GlobInput) -> String {
        let timed_out = self.interrupt == Some(Interrupt::TimedOut);
        if self.total == 0 {
            return if timed_out {
                format!(
                    "No files match {} in {} before the search timed out after {}s — narrow the path or pattern",
                    input.pattern,
                    input.path,
                    SCAN_TIMEOUT.as_secs()
                )
            } else {
                format!("No files match {} in {}", input.pattern, input.path)
            };
        }
        let mut text = self.newest.join("\n");
        let saved = match &self.spill {
            Some(spill) if spill.capped => format!(
                "; the first 64 MB of the full list is saved to {}",
                spill.path.display()
            ),
            Some(spill) => format!("; full list saved to {}", spill.path.display()),
            None => String::new(),
        };
        if timed_out {
            text.push_str(&format!(
                "\n\n[at least {} files; search timed out after {}s — narrow the path or pattern{saved}]",
                self.total,
                SCAN_TIMEOUT.as_secs()
            ));
        } else if self.total > self.newest.len() as u64 {
            text.push_str(&format!(
                "\n\n[showing the {} most recently modified of {} files{saved}]",
                self.newest.len(),
                self.total
            ));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use super::super::test_support::TestDirectory;
    use super::*;
    use crate::{SpillDirectory, ToolErrorCode, ToolOutput};

    fn session(workspace: &TestDirectory) -> ToolSessionContext {
        crate::test_support::unconfined_session(workspace.path())
    }

    async fn glob(session: &ToolSessionContext, pattern: &str, path: &str) -> String {
        GlobTool
            .execute(
                session,
                crate::test_support::call_context("glob", CancellationToken::new()),
                GlobInput {
                    pattern: pattern.to_string(),
                    path: path.to_string(),
                },
            )
            .await
            .expect("glob result")
            .into_tool_result()
            .text_content()
    }

    fn set_modified(path: &std::path::Path, seconds: u64) {
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open fixture");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .expect("set mtime");
    }

    /// tools.md §10 #24: at most 100 paths, newest first, exact total, full
    /// list on disk.
    #[tokio::test]
    async fn acc_24_returns_the_newest_hundred_with_an_exact_total() {
        let workspace = TestDirectory::new("glob-newest");
        for index in 0..150u64 {
            let path = workspace.path().join(format!("f{index:03}.rs"));
            std::fs::write(&path, "").expect("write fixture");
            set_modified(&path, 1_000_000 + index);
        }
        std::fs::write(workspace.path().join("skip.txt"), "").expect("write fixture");
        let spill_root = workspace.path().join(".spill");
        let session = session(&workspace).with_spill_directory(SpillDirectory::new(&spill_root));

        let text = glob(&session, "*.rs", ".").await;

        let (listing, footer) = text.split_once("\n\n").expect("footer");
        let listed = listing.lines().collect::<Vec<_>>();
        assert_eq!(listed.len(), MAX_RESULTS);
        assert_eq!(listed.first(), Some(&"f149.rs"));
        assert_eq!(listed.last(), Some(&"f050.rs"));
        let saved = spill_root.join("glob.txt");
        assert_eq!(
            footer,
            format!(
                "[showing the 100 most recently modified of 150 files; full list saved to {}]",
                saved.display()
            )
        );
        assert_eq!(
            std::fs::read_to_string(saved)
                .expect("spill file")
                .lines()
                .count(),
            150
        );
    }

    #[tokio::test]
    async fn matches_relative_to_the_search_root_and_shows_workspace_paths() {
        let workspace = TestDirectory::new("glob-relative");
        std::fs::create_dir_all(workspace.path().join("desktop/src")).expect("create dirs");
        std::fs::write(workspace.path().join("desktop/src/app.ts"), "").expect("write fixture");

        let text = glob(&session(&workspace), "src/**/*.ts", "desktop").await;

        assert_eq!(text, "desktop/src/app.ts");
    }

    #[tokio::test]
    async fn reports_no_matches() {
        let workspace = TestDirectory::new("glob-none");
        assert_eq!(
            glob(&session(&workspace), "*.rs", ".").await,
            "No files match *.rs in ."
        );
    }

    #[tokio::test]
    async fn observes_cancellation_while_traversing() {
        let workspace = TestDirectory::new("glob-cancel");
        std::fs::write(workspace.path().join("a.rs"), "fixture").expect("write fixture");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = GlobTool
            .execute(
                &session(&workspace),
                crate::test_support::call_context("glob-cancel", cancel),
                GlobInput {
                    pattern: "*.rs".to_string(),
                    path: ".".to_string(),
                },
            )
            .await
            .expect_err("cancelled traversal");

        assert_eq!(error.code, ToolErrorCode::Cancelled);
    }

    #[test]
    fn timeout_reports_at_least_the_counted_files() {
        let scan = Scan {
            newest: vec!["a.rs".to_string()],
            total: 1,
            interrupt: Some(Interrupt::TimedOut),
            spill: None,
        };
        assert_eq!(
            scan.render(&GlobInput {
                pattern: "*.rs".to_string(),
                path: ".".to_string()
            }),
            "a.rs\n\n[at least 1 files; search timed out after 30s — narrow the path or pattern]"
        );
    }
}
