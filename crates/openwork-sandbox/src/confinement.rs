//! 协作 Agent 的本机 Engine 进程能读写哪里（collaboration.md §3.1）。
//!
//! 与工作台的 [`SandboxPolicy`](crate::SandboxPolicy) 不同：这里没有模式、授权与四档路径，只有
//! 一个 Agent 的固定围栏。工作台把整个 `~/.openwork` 列为硬保护，而 Agent 的 home 就在其中，
//! 所以两者不共用策略；共用的是 Seatbelt profile 的生成方式与主机事实。

use std::path::{Path, PathBuf};

use crate::policy::{SandboxEnvironment, canonical_or_lexical};

/// 一个 Agent 的 Engine 进程树的围栏：只能写 `writable_roots` 与临时根；`$HOME` 之内只能读
/// `writable_roots`、临时根与 `readable_paths`；`$HOME` 之外照常可读；网络不受限制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfinement {
    home: PathBuf,
    temp_roots: Vec<PathBuf>,
    writable_roots: Vec<PathBuf>,
    readable_paths: Vec<PathBuf>,
}

impl EngineConfinement {
    /// 以 `environment` 的主目录与临时根为基础。此时只有临时根与可写设备可写，
    /// `$HOME` 之内（临时根除外）什么都读不到。
    pub fn new(environment: &SandboxEnvironment) -> Self {
        Self {
            home: environment.home().to_path_buf(),
            temp_roots: environment.temp_roots().to_vec(),
            writable_roots: Vec::new(),
            readable_paths: Vec::new(),
        }
    }

    /// `root` 整个子树可读可写。路径在此规范化：Seatbelt 按真实路径匹配。
    pub fn with_writable_root(mut self, root: &Path) -> Self {
        self.writable_roots.push(canonical_or_lexical(root));
        self
    }

    /// `$HOME` 之内额外可读的文件或目录子树，例如本 Agent 的 token 与 Engine 可执行文件。
    pub fn with_readable_path(mut self, path: &Path) -> Self {
        self.readable_paths.push(canonical_or_lexical(path));
        self
    }

    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    /// 可写的根：临时根在前，随后是本 Agent 的根。
    pub(crate) fn writable_roots(&self) -> impl Iterator<Item = &Path> {
        self.temp_roots
            .iter()
            .chain(&self.writable_roots)
            .map(PathBuf::as_path)
    }

    /// `$HOME` 之内仍可读的路径：所有可写根，加上显式放行的路径。
    pub(crate) fn home_read_exceptions(&self) -> impl Iterator<Item = &Path> {
        self.writable_roots()
            .chain(self.readable_paths.iter().map(PathBuf::as_path))
    }
}
