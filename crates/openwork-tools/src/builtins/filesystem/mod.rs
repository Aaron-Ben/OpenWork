mod edit;
mod glob;
mod grep;
mod list;
mod read;
mod write;

use std::path::{Path, PathBuf};

pub(crate) use edit::Edit;
pub(crate) use glob::Glob;
pub(crate) use grep::Grep;
pub(crate) use list::List;
pub(crate) use read::Read;
pub(crate) use write::Write;

/// 解析 Action 的 path 参数：绝对路径原样返回，相对路径基于工作目录解析。
pub(super) fn resolve(working_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}
