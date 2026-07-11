//! OpenWork 内置 Action Handler，按它们操作的外部执行领域组织。

mod filesystem;
mod output;
mod process;

pub(crate) use filesystem::{Edit, Glob, Grep, List, Read, Write};
pub(crate) use output::truncate_output;
pub(crate) use process::Bash;
