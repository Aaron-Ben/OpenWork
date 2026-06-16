use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::Path;

use crate::tool::{Tool, ToolContext, ToolOutput};
use crate::{AccessKind, builtin::resolve};

/// 精确文本编辑工具:用 `newString` 替换文件中唯一出现的 `oldString`。
/// `oldString == ""` 表示新建文件(已存在则拒绝)。`replaceAll: true` 替换全部。
#[derive(Default)]
pub struct Edit;

#[async_trait]
impl Tool for Edit {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing a unique occurrence of `oldString` with `newString`. \
         Use `oldString: \"\"` to create a new file (refuses if it already exists). \
         Set `replaceAll: true` to replace every occurrence. \
         Without `replaceAll`, `oldString` must match exactly and be unique in the file."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filePath": { "type": "string", "description": "Absolute or working-dir-relative path." },
                "oldString": { "type": "string", "description": "Exact text to find. Empty string means create a new file." },
                "newString": { "type": "string", "description": "Replacement text (or full content for a new file)." },
                "replaceAll": { "type": "boolean", "description": "Replace every occurrence. Defaults to false." }
            },
            "required": ["filePath", "oldString", "newString"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path) = input.get("filePath").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'filePath' argument");
        };
        let Some(old) = input.get("oldString").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'oldString' argument");
        };
        let Some(new) = input.get("newString").and_then(Value::as_str) else {
            return ToolOutput::error("missing or invalid 'newString' argument");
        };
        let replace_all = input
            .get("replaceAll")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let resolved = resolve(&ctx.working_dir, path);
        if let Err(message) = ctx.check_path(&resolved, AccessKind::Write) {
            return ToolOutput::error(message);
        }
        match apply_edit(&resolved, old, new, replace_all).await {
            Ok(message) => ToolOutput::text(message),
            Err(message) => ToolOutput::error(message),
        }
    }
}

/// 纯编辑逻辑(抽出来便于单测):对 `path` 应用一次编辑,返回人类可读结果或错误文案。
async fn apply_edit(
    path: &Path,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, String> {
    if old == new {
        return Err("oldString and newString are identical (no-op)".to_string());
    }

    // 新建文件分支。
    if old.is_empty() {
        if path.exists() {
            return Err(format!(
                "file already exists: {}; to modify it, provide a non-empty oldString",
                path.display()
            ));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("failed to create parent dirs: {e}"))?;
        }
        tokio::fs::write(path, new)
            .await
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        return Ok(format!("created {} ({} bytes)", path.display(), new.len()));
    }

    // 编辑现有文件。
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    let count = content.matches(old).count();
    if count == 0 {
        return Err(format!("oldString not found in {}", path.display()));
    }

    let updated = if replace_all {
        content.replace(old, new)
    } else if count == 1 {
        content.replacen(old, new, 1)
    } else {
        return Err(format!(
            "oldString is not unique: found {} occurrences in {}; include more surrounding context or set replaceAll: true",
            count,
            path.display()
        ));
    };

    tokio::fs::write(path, &updated)
        .await
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;

    if replace_all {
        Ok(format!(
            "replaced {} occurrence(s) in {}",
            count,
            path.display()
        ))
    } else {
        Ok(format!("edited {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 进程内唯一的临时文件路径(避免引入 uuid dev-dep)。
    fn temp_file() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("anvil-edit-test-{id}.txt"));
        p
    }

    #[tokio::test]
    async fn create_new_file_with_empty_old() {
        let path = temp_file();
        let _ = std::fs::remove_file(&path);
        let msg = apply_edit(&path, "", "hello world", false).await.unwrap();
        assert!(msg.contains("created"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn create_refuses_existing_file() {
        let path = temp_file();
        std::fs::write(&path, "x").unwrap();
        let err = apply_edit(&path, "", "y", false).await.unwrap_err();
        assert!(err.contains("already exists"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replace_unique_occurrence() {
        let path = temp_file();
        std::fs::write(&path, "foo bar foo").unwrap();
        apply_edit(&path, "bar", "baz", false).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo baz foo");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn non_unique_without_replace_all_errors() {
        let path = temp_file();
        std::fs::write(&path, "foo foo foo").unwrap();
        let err = apply_edit(&path, "foo", "x", false).await.unwrap_err();
        assert!(err.contains("not unique"));
        // 失败时不改写文件。
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo foo foo");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replace_all_replaces_every_occurrence() {
        let path = temp_file();
        std::fs::write(&path, "foo foo foo").unwrap();
        let msg = apply_edit(&path, "foo", "x", true).await.unwrap();
        assert!(msg.contains("3 occurrence"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x x x");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn not_found_errors() {
        let path = temp_file();
        std::fs::write(&path, "hello").unwrap();
        let err = apply_edit(&path, "missing", "x", false).await.unwrap_err();
        assert!(err.contains("not found"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn identical_old_new_is_noop() {
        let path = temp_file();
        std::fs::write(&path, "hello").unwrap();
        let err = apply_edit(&path, "hello", "hello", false)
            .await
            .unwrap_err();
        assert!(err.contains("identical"));
        let _ = std::fs::remove_file(&path);
    }
}
