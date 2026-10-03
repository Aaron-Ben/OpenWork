# Agent Note: 结果的三条通道

Status: implemented

## 问题

一次 Tool Call 的输出有三类读者：模型、正在看着的界面、之后重载或撤销时的界面。它们需要的内容不同。

模型只需要有界的文本，多一个字节都要随每次 Model Call 重发。界面在执行中要看实时输出。重载与撤销需要当时准确的变更内容，包括文件的完整前后版本。把这三样塞进同一个字段，要么撑大模型上下文，要么丢掉界面需要的数据。

## 决策

- **文本**：`ToolResult.content`。模型适配器只发送它（`crates/openwork-models/src/adapters/openai_chat/request.rs`）。
- **Artifact**：`ToolResult.artifacts`，随 Tool 消息写入 `messages.content`。它不发给模型，也不计入 Token 预算（`crates/openwork-core/src/context/budget.rs`）。`file_change` Artifact 保存 diff hunk、前后哈希与前后内容（`crates/openwork-tools/src/file_change.rs`）。
- **Progress**：`ToolProgress` 经 `try_send` 发出，Core 转成 `tool_call_progress` Update。它不写入 Conversation，不进 Snapshot。

规则见 [tools.md §8](../../../../docs/subsystems/tools.md)。

## 考虑过的方案

**桌面端重新读磁盘计算 diff。** 没有采用：Session 重载后，磁盘已是后来的状态，界面只能猜当时改了什么。Artifact 在写入时记录准确的变更，重载后照样显示。

**Progress 写入 Conversation 或 Snapshot。** 没有采用：进度是临时观察数据，一次调用只有一个最终结果。不落库，增加进度能力就不需要数据库迁移。代价是断线重同步只恢复最终结果。

**用 Git 快照实现撤销。** 原始记录只写明这条链路“不是 Git 快照或任意文件系统事务”，没有写比较过程。

## 后果

- `file_change` Artifact 携带文件的完整前后内容。界面能撤销与重新应用，不依赖磁盘或 Git。
- Artifact 不计入 Token 预算，否则一次大文件编辑会为模型看不到的内容触发压缩。
- 压缩运行状态的 `edited_paths` 只从 Artifact 得出（[compaction.md](../../../../docs/subsystems/compaction.md) §3）。
- 撤销只覆盖 write 与 edit 改过的、不超过 1 MiB 的 UTF-8 文本文件。bash 的改动不能撤销。
- 撤销不是数据库与文件系统的单一原子事务。文件改写成功而 `undone` 写回失败时，两者短暂不一致。
- 进度通道满时丢弃进度。界面可能漏掉部分实时输出，最终结果不受影响。
