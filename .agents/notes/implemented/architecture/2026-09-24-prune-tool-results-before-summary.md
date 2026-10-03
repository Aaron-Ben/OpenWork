# Agent Note: 先修剪旧工具结果，再决定是否摘要

Status: implemented

## 问题

上下文压力的主要来源常常是旧的大工具结果。摘要能解决压力，但它要调模型、要花时间，还会丢细节。工具结果上限只压住了单条结果。参照任务回放中，加上上限之后总量只降了 4%，见 [Agent Note：工具结果上限](2026-09-24-tool-result-bounds.md)。

## 决策

- threshold 与 overflow 触发后，先推进修剪水位线，再重新估算（`crates/openwork-core/src/session/compaction/prune.rs`、`crates/openwork-core/src/context/prune.rs`）。
- 水位线以下、超过 8 192 字符的 Tool Result 只留开头 4 096 与结尾 1 024 字符，中间换成一行标记，写明完整内容的落盘路径。
- 水位线停在最近一次模型响应上，存在 `sessions` 表里，只进不退。
- 修剪只改请求副本。没有落盘的结果在修剪前补写落盘文件。
- 任何一步失败，都退回摘要。修剪不是压缩：它不写 checkpoint，也不占用压缩额度。
- overflow 更保守：只有修剪后低于压缩线，才不摘要直接重提交。
- 手动压缩不先修剪。

阈值与做法来自两个参考项目。DSH 的 tool-result-pruner 默认 8 192 / 4 096 / 1 024，只在压缩触发后、摘要之前运行，不调模型（`packages/compaction/compaction-tool-result-pruner/src/config.ts` 与同目录 README）。maka 把工具结果归档，在模型投影里换成可以回读的占位（`packages/runtime/src/tool-result-archive-transition.ts`）。

事实见 [compaction.md §2](../../../../docs/subsystems/compaction.md)。

## 考虑过的方案

**只靠摘要。** 没有采用，理由见“问题”。

**压力下降后恢复原文。** 没有采用。同一段历史会在修剪与不修剪之间来回切换，每次切换都让提示缓存失效。

**修剪最近一次响应产生的结果。** 没有采用。模型正要基于它们做下一步。

**手动压缩也先修剪。** 没有采用。用户执行 `/compact` 要的就是一份摘要。

## 后果

- 很多次压力可以不调摘要模型就解除，也不丢失可以取回的内容。
- 修剪标记里的路径必须可读。补写落盘失败时，这次不修剪。
- 摘要的输入是修剪后的投影。标记写明了省略的内容和位置，摘要的事实来源仍可追溯。
- 水位线只进不退，所以压力下降后，旧结果也不会恢复原文。
