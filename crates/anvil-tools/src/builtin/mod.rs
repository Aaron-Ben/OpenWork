mod bash;
mod list;
mod read;
mod write;

use std::path::{Path, PathBuf};

pub use bash::Bash;
pub use list::List;
pub use read::Read;
pub use write::Write;

/// 解析工具的 path 参数:绝对路径原样返回,相对路径基于工作目录解析。
pub(crate) fn resolve(working_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}
