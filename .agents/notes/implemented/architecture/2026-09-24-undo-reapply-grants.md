# Agent Note: 撤销与重新应用文件改动时的授权

Status: implemented

## 问题

`write` / `edit` 的改动可以在界面上撤销与重新应用（[tools.md §8](../../../../docs/subsystems/tools.md)）。这是用户的操作，不是模型的 Tool Call。它仍要经过文件工具围栏，所以需要一个 `SandboxPolicy`。

只用会话模式不够。模型可以经越界批准修改敏感文件，例如 `.env`。越界授权只作用于那一次调用，不留状态。按会话模式，这个文件不可写。于是这次改动做得出来，却撤销不了：用户收不回一次自己批准过的改动。

原始记录写的是“用户选 A”，没有保存其他选项的内容。

## 决策

授权为：会话模式，加上这批改动涉及的每个文件的精确写授权。

- `SandboxRuntime::file_change_policy`（`crates/openwork-core/src/session_tools.rs`）为每个 `FileChangeArtifact` 生成一条 `PathGrant`：`access = Write`、`scope = Exact`。路径先接到工作区根下，再规范化。
- `OpenWorkCore::file_change_context`（`crates/openwork-core/src/core.rs`）用会话落库的 `sandbox_mode` 作为模式。`undo_file_changes` 与 `reapply_file_changes` 都经过它。
- 授权不影响硬保护。`.git/hooks`、`~/.openwork` 与 skill 根照样不可写。
- 授权只覆盖这批改动记录里的文件，不覆盖目录。

设计见 [permissions.md §4](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

<!-- agent-note: 原始记录没有备选方案 -->

## 后果

- 经越界批准改过的敏感文件也能撤销与重新应用。
- 撤销不出卡片。用户点“撤销”这个动作本身就是授权。
- 授权的范围等于改动记录里的路径。模型不能借撤销写其他文件。
- 哈希检查照常生效。文件后来被外部改动时，撤销返回冲突，不覆盖。
- 授权不留任何会话状态，与越界授权的生存期一致。
