use crate::policy::AccessKind;
use async_trait::async_trait;
use globset::{Glob as GlobPattern, GlobSet};
use ignore::WalkBuilder;
use openwork_protocol::capability::{Observation, ObservationErrorCode};
use serde_json::Value;
use std::path::Path;

use crate::ExecutionContext;
use crate::actions::truncate_output;
use crate::handler::ActionHandler;

use super::resolve;

const MAX_RESULTS: usize = 2000;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

/// 按文件名模式查找文件(尊重 .gitignore)。用 `spawn_blocking` 避免阻塞 tokio。
#[derive(Default)]
pub struct Glob;

#[async_trait]
impl ActionHandler for Glob {
    fn name(&self) -> &'static str {
        "glob"
    }

    async fn invoke(&self, input: Value, ctx: &ExecutionContext) -> Observation {
        let Some(pattern) = input.get("pattern").and_then(Value::as_str) else {
            return invalid_arguments("missing or invalid 'pattern' argument");
        };
        let path = input.get("path").and_then(Value::as_str).unwrap_or(".");

        let glob = match GlobPattern::new(pattern) {
            Ok(g) => g,
            Err(err) => return invalid_arguments(format!("invalid glob: {err}")),
        };
        let set = match GlobSet::builder().add(glob).build() {
            Ok(s) => s,
            Err(err) => return invalid_arguments(format!("invalid glob: {err}")),
        };

        let root = resolve(&ctx.working_dir, path);
        if let Err(message) = ctx.check_path(&root, AccessKind::Read) {
            return Observation::denied(message);
        }
        let result = tokio::task::spawn_blocking(move || run_glob(&root, &set)).await;

        match result {
            Ok(output) => {
                let truncated = truncate_output(output, MAX_OUTPUT_BYTES);
                Observation::succeeded(truncated)
            }
            Err(err) => Observation::failed(
                ObservationErrorCode::ExecutionFailed,
                format!("glob task failed: {err}"),
                false,
            ),
        }
    }
}

fn invalid_arguments(message: impl Into<String>) -> Observation {
    Observation::failed(ObservationErrorCode::InvalidArguments, message, false)
}

fn run_glob(root: &Path, set: &GlobSet) -> String {
    if !root.exists() {
        return format!("path not found: {}", root.display());
    }
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .build();

    let mut out = String::new();
    let mut count = 0usize;
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if set.is_match(path) {
            let rel = path.strip_prefix(root).unwrap_or(path);
            out.push_str(&format!("{}\n", rel.display()));
            count += 1;
            if count >= MAX_RESULTS {
                break;
            }
        }
    }

    if out.is_empty() {
        "no files matched\n".to_string()
    } else {
        out
    }
}
