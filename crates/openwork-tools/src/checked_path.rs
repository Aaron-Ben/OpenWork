//! 文件工具的路径围栏（tools.md §8、permissions.md §2.4）。
//!
//! 先把路径规范化成真实路径（解析符号链接；新文件取最近的已存在父目录），再用本次调用的
//! [`SandboxPolicy`] 以 `Actor::FileTool` 判断。四档路径由 `openwork-sandbox` 定义，与 bash 的
//! Seatbelt profile 出自同一组函数，这里不持有任何路径规则。
//!
//! 围栏是策略边界，不是内核边界：威胁面是模型给的路径参数，工具代码本身可信。

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use openwork_sandbox::{Access, Actor, Denial, SandboxPolicy};

use crate::notice;
use crate::path::display_path;
use crate::{ToolExecutionError, ToolSessionContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathIntent {
    MustExist,
    MayCreate,
}

/// 通过了围栏的真实路径。字段私有：工具拿不到未经检查的 `PathBuf`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckedPath {
    actual: PathBuf,
}

impl CheckedPath {
    pub(crate) fn as_path(&self) -> &Path {
        &self.actual
    }
}

impl ToolSessionContext {
    /// 解析并检查一个路径参数。被当前策略拒绝时，错误文本就是给模型的拒绝标记。
    pub(crate) async fn resolve_path(
        &self,
        input: &str,
        access: Access,
        intent: PathIntent,
        policy: &SandboxPolicy,
    ) -> Result<CheckedPath, ToolExecutionError> {
        let lexical = self.absolute(input);
        let actual = match intent {
            PathIntent::MustExist => {
                self.filesystem
                    .canonicalize(&lexical)
                    .await
                    .map_err(|error| {
                        ToolExecutionError::execution(format!(
                            "failed to resolve {}: {error}",
                            lexical.display()
                        ))
                    })?
            }
            PathIntent::MayCreate => self.resolve_creatable_path(&lexical).await?,
        };
        self.fence(&actual, access, policy)?;
        Ok(CheckedPath { actual })
    }

    /// 越界请求里的路径按与围栏相同的方式规范化，使校验与执行看到的是同一个真实路径。
    /// 相对路径原样返回，由校验报出"必须是绝对路径"。
    pub(crate) async fn canonical_grant_path(&self, input: &str) -> PathBuf {
        let requested = Path::new(input);
        if !requested.is_absolute() {
            return requested.to_path_buf();
        }
        self.canonical_or_lexical(input).await
    }

    /// 与围栏相同的规范化，但不报错：解析失败时退回字面规范化的结果。用于执行前的
    /// 预判（写目标是否硬保护），真正的检查仍在执行时由 [`Self::resolve_path`] 完成。
    pub(crate) async fn canonical_or_lexical(&self, input: &str) -> PathBuf {
        let lexical = self.absolute(input);
        match self.filesystem.canonicalize(&lexical).await {
            Ok(canonical) => canonical,
            Err(_) => self
                .resolve_creatable_path(&lexical)
                .await
                .unwrap_or(lexical),
        }
    }

    fn fence(
        &self,
        actual: &Path,
        access: Access,
        policy: &SandboxPolicy,
    ) -> Result<(), ToolExecutionError> {
        let display = display_path(&policy.workspace_root, actual);
        match policy.check(actual, access, Actor::FileTool) {
            Ok(()) => Ok(()),
            Err(Denial::HardProtected) => {
                Err(ToolExecutionError::denied(notice::protected(&display)))
            }
            Err(Denial::Sensitive | Denial::Credential | Denial::OutsideWritableRoots) => {
                Err(ToolExecutionError::sandbox_denied(notice::file_denied(
                    &display,
                    access,
                    policy.mode,
                    self.escalation_available(),
                )))
            }
        }
    }

    /// 尚不存在的路径：规范化最近的已存在祖先，再接回其余部分。
    pub(crate) async fn resolve_creatable_path(
        &self,
        lexical: &Path,
    ) -> Result<PathBuf, ToolExecutionError> {
        let mut anchor = lexical.to_path_buf();
        let mut suffix = Vec::<OsString>::new();
        loop {
            match self.filesystem.canonicalize(&anchor).await {
                Ok(canonical) => {
                    suffix.reverse();
                    return Ok(suffix
                        .into_iter()
                        .fold(canonical, |path, component| path.join(component)));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.reject_dangling_symlink(&anchor).await?;
                    let component = anchor
                        .file_name()
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| no_existing_parent(lexical))?;
                    suffix.push(component);
                    if !anchor.pop() {
                        return Err(no_existing_parent(lexical));
                    }
                }
                Err(error) => {
                    return Err(ToolExecutionError::execution(format!(
                        "failed to resolve {}: {error}",
                        anchor.display()
                    )));
                }
            }
        }
    }

    /// 指向不存在目标的符号链接：跟着它写会落在检查不到的地方，直接拒绝。
    async fn reject_dangling_symlink(&self, anchor: &Path) -> Result<(), ToolExecutionError> {
        match self.filesystem.is_symlink(anchor).await {
            Ok(true) => Err(ToolExecutionError::denied(format!(
                "access denied through dangling symlink: {}",
                anchor.display()
            ))),
            Ok(false) => Err(ToolExecutionError::execution(format!(
                "failed to resolve existing path component: {}",
                anchor.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ToolExecutionError::execution(format!(
                "failed to inspect {}: {error}",
                anchor.display()
            ))),
        }
    }
}

fn no_existing_parent(lexical: &Path) -> ToolExecutionError {
    ToolExecutionError::execution(format!(
        "failed to find an existing parent for {}",
        lexical.display()
    ))
}
