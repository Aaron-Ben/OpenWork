mod edit;
mod glob;
mod grep;
mod list;
mod read;
mod write;

use std::path::{Path, PathBuf};

pub(crate) use edit::EditTool;
pub(crate) use glob::GlobTool;
pub(crate) use grep::GrepTool;
pub(crate) use list::ListTool;
pub(crate) use read::ReadTool;
pub(crate) use write::WriteTool;

/// 解析 Action 的 path 参数：绝对路径原样返回，相对路径基于工作目录解析。
pub(super) fn resolve(working_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}
