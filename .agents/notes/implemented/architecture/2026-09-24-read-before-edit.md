# Agent Note: 先读后改

Status: implemented

## 问题

模型会凭记忆或猜测修改没看过的文件。它也会覆盖用户或 bash 在它上次读取之后做的修改。

edit 内部的内容复核只防一种情况：读取文件与提交写入之间的并发修改。它不知道模型上次看到的是哪个版本。

## 决策

- `FileObservations`（`crates/openwork-tools/src/observation.rs`）记录真实路径到内容 SHA-256 的映射。成功的 read、write、edit 更新它。
- write 与 edit 修改已存在的文件前，`check_current` 要求表中有记录，且哈希等于磁盘内容。创建新文件不检查。
- read 读任何一段都记录整个文件的哈希（`crates/openwork-tools/src/builtins/filesystem/read.rs`）。
- Core 按 Session 持有观察表（`crates/openwork-core/src/core.rs` 的 `session_tool_state`），同一 Session 的 Turn 共用。
- 观察表只在内存中。

规则见 [tools.md §7.4](../../../../docs/subsystems/tools.md)。做法来自 DSH 的 `packages/fs/fs-observation-policy/src/index.ts`，它同样只在内存中保存观察状态。

## 考虑过的方案

**只靠 edit 内部的内容复核。** 没有采用：它只覆盖读取与提交之间的窗口。bash 或用户在模型读取之后、下一次 edit 之前做的修改，它看不见。

**观察表落库。** 没有采用：与 DSH 一样只放在内存中。代价是进程重启后，第一次编辑前要多读一次。

## 后果

- 模型没看过就改、覆盖他人修改，这两种错误都变成可操作的错误文本：`Read <path> before editing it.` 或 `... changed since you last read it ...`。
- 连续编辑同一文件不需要重读，因为 write 与 edit 成功后更新哈希。
- 读一个大文件的一小段也算“看过”。模型可能只看过开头就编辑结尾。
- 撤销与重新应用不经过观察表，它们用 Artifact 中的哈希检查冲突。
- 参照任务回放显示，单条结果上限只让模型可见的 token 下降 4%。先读后改与旧结果修剪因此提前到沙箱改造之前完成，见 [工具结果上限](2026-09-24-tool-result-bounds.md) 与 [先修剪旧工具结果](2026-09-24-prune-tool-results-before-summary.md)。
