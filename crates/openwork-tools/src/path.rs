//! 不访问文件系统的路径运算。

use std::path::{Component, Path, PathBuf};

/// 只按字面消去 `.` 与 `..`，不解析符号链接。
///
/// 结果不能证明真实文件在哪里——那要靠规范化（tools.md §6.2）；这里只把模型给的
/// 相对写法变成一个可以拿去规范化的绝对路径。
pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// 给模型看的路径：在工作区内时相对工作区，否则是绝对路径（agent-context.md §1）。
pub(crate) fn display_path(workspace: &Path, path: &Path) -> String {
    path.strip_prefix(workspace)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .display()
        .to_string()
}
