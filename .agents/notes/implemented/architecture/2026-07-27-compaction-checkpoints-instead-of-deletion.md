# Agent Note: 压缩写 checkpoint，不删除消息

Status: implemented

## 问题

长对话会超出上下文窗口。最直接的做法是删除旧消息，或把它们改写成摘要。但用户还需要三件事：回到某个历史位置继续；在摘要缺少细节时读取原文；在重启之后得到同一份对话。删除或改写会让这三件事都失去事实依据。

## 决策

- 一次压缩新增一行 `conversation_compactions`。`messages` 里的原文不变（`crates/openwork-core/src/storage/postgres/compaction.rs`）。
- 模型看到的是投影：replay、摘要、提醒，加上安装边界之后的原始消息。Session 载入时从最新 checkpoint 重建投影。
- checkpoint 有两个边界。事实边界记录摘要覆盖到哪里，安装边界记录从哪里开始拼接原始消息。
- rewind 不改旧行，而是新增一条 `kind = 'rewind'` 的 checkpoint，安装边界取执行时的最大序号，从而隐藏旧尾部。
- 原文回读直接按序号查询 `messages`，以事实边界为上界，不需要新表。
- System Context 与工具面不进 checkpoint，每次从来源重新物化。

事实见 [compaction.md §5、§6、§8](../../../../docs/subsystems/compaction.md)。

## 考虑过的方案

**删除或截断旧消息。** 没有采用。原设计文档把“摘要失败后直接截断旧消息”列为不允许的降级。删除之后，rewind 与原文回读都无从实现。

**只在内存里 rewind。** 没有采用。重启后，最新 checkpoint 与被放弃的尾部会再次出现。

**rewind 时重放 Tool Call 或撤销文件。** 没有采用。rewind 只换投影。文件回滚走 FileChange Undo，由用户显式触发。

**为回读另建一张原文表。** 没有采用。原始消息从未删除，事实边界已经给出严格上界。

## 后果

- 数据库只增不减，存储随对话增长。
- 两个边界增加了理解成本：只有 rewind 让它们不同。
- 重启后对话部分一致，但完整请求不是历史请求的字节复现：`AGENTS.md` 等来源变了，请求也随之变化。
- 模型可以用 `conversation_history` 读回原文，不必猜测摘要省略的细节。
- rewind 不撤销文件副作用，用户可能看到对话回到过去而文件仍是新的。
