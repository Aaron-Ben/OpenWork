//! 权限相关的文本分析。按命令文本判定放行的代码已全部删除（permissions.md §1.4）；
//! 这里只剩危险命令检测，它只决定要不要多问一次，不决定能不能执行。

mod danger;

pub use danger::{DangerKey, DangerMatch, detect as detect_danger};
