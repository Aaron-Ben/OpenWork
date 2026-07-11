use async_trait::async_trait;
use globset::Glob as GlobSpec;
use ignore::WalkBuilder;
use openwork_permissions::AccessKind;
use openwork_protocol::capability::{Observation, ObservationErrorCode};
use regex::Regex;
use serde_json::Value;
use std::path::Path;

use crate::ExecutionContext;
use crate::actions::truncate_output;
use crate::handler::ActionHandler;

use super::resolve;

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

/// 正则搜索文件内容(ripgrep 风格,尊重 .gitignore)。用 `spawn_blocking` 避免阻塞 tokio。
#[derive(Default)]
pub struct Grep;

#[async_trait]
impl ActionHandler for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }

    async fn invoke(&self, input: Value, ctx: &ExecutionContext) -> Observation {
        let Some(pattern) = input.get("pattern").and_then(Value::as_str) else {
            return invalid_arguments("missing or invalid 'pattern' argument");
        };
        let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let glob_filter = input.get("glob").and_then(Value::as_str);
        let output_mode = input
            .get("outputMode")
            .and_then(Value::as_str)
            .unwrap_or("content");
        let max_results = input
            .get("maxResults")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS);

        let regex = match Regex::new(pattern) {
            Ok(r) => r,
            Err(err) => return invalid_arguments(format!("invalid regex: {err}")),
        };
        let matcher = match glob_filter {
            Some(g) => match GlobSpec::new(g) {
                Ok(gb) => Some(gb.compile_matcher()),
                Err(err) => return invalid_arguments(format!("invalid glob: {err}")),
            },
            None => None,
        };

        let root = resolve(&ctx.working_dir, path);
        if let Err(message) = ctx.check_path(&root, AccessKind::Read) {
            return Observation::denied(message);
        }
        let mode = output_mode.to_string();

        let result = tokio::task::spawn_blocking(move || {
            run_grep(&root, &regex, matcher.as_ref(), &mode, max_results)
        })
        .await;

        match result {
            Ok(output) => {
                let truncated = truncate_output(output, MAX_OUTPUT_BYTES);
                Observation::succeeded(truncated)
            }
            Err(err) => Observation::failed(
                ObservationErrorCode::ExecutionFailed,
                format!("grep task failed: {err}"),
                false,
            ),
        }
    }
}

fn invalid_arguments(message: impl Into<String>) -> Observation {
    Observation::failed(ObservationErrorCode::InvalidArguments, message, false)
}

fn run_grep(
    root: &Path,
    regex: &Regex,
    matcher: Option<&globset::GlobMatcher>,
    mode: &str,
    max_results: usize,
) -> String {
    if !root.exists() {
        return format!("path not found: {}", root.display());
    }
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .build();

    let mut out = String::new();
    let mut hits = 0usize;

    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if let Some(m) = matcher
            && !m.is_match(path)
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = path.strip_prefix(root).unwrap_or(path);

        match mode {
            "files_with_matches" => {
                if content.lines().any(|line| regex.is_match(line)) {
                    out.push_str(&format!("{}\n", rel.display()));
                    hits += 1;
                }
            }
            "count" => {
                let c = content.lines().filter(|line| regex.is_match(line)).count();
                if c > 0 {
                    out.push_str(&format!("{}:{}\n", rel.display(), c));
                    hits += 1;
                }
            }
            _ => {
                for (i, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        out.push_str(&format!("{}:{}:{}\n", rel.display(), i + 1, line));
                        hits += 1;
                        if hits >= max_results {
                            break;
                        }
                    }
                }
            }
        }
        if hits >= max_results {
            break;
        }
    }

    if out.is_empty() {
        format!("no matches for /{}/", regex.as_str())
    } else {
        out
    }
}
