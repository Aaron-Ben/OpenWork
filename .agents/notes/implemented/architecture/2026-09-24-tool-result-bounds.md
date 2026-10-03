# Agent Note: 工具结果上限 32 KB，单个落盘文件上限 64 MiB

Status: implemented

## 问题

工具结果会一直留在 Conversation 里，并随每次 Model Call 重发。改造前，工具先返回完整结果，再由请求投影截断。投影的单条上限是 8000 token。`truncate_text`（`crates/openwork-core/src/context/projection.rs`）保留头尾，去掉中间。源码的关键部分往往在中间。

T0 最初沿用 DSH 的 read 字节上限 50 KB。DSH 在 `packages/fs/tool-fs/src/read-render.ts` 中定义 `READ_MAX_BYTES = 50 * 1024`。按这个值，32–50 KB 之间的读取仍会被投影从中间截断。

落盘解决“截断即丢失”。但 `yes` 一类的无限输出在超时前能写出数 GB。

## 决策

- `MAX_RESULT_BYTES = 32_000`（`crates/openwork-tools/src/spill.rs`），包含结尾的续读或落盘提示。它等于 `DEFAULT_MAX_TOOL_RESULT_TOKENS = 8_000`（`crates/openwork-core/src/context/limits.rs`）乘以 `ESTIMATED_BYTES_PER_TOKEN = 4`（`crates/openwork-core/src/context/budget.rs`）。
- read 的内容预算是 `MAX_RESULT_BYTES - FOOTER_RESERVE_BYTES`（512 字节），停在最后一个完整行（`crates/openwork-tools/src/builtins/filesystem/read.rs`）。
- 其他工具的结果超过 32 000 字节，或触及工具自己的上限时，完整内容写入 `~/.openwork/spill/<session-id>/`。read 不落盘。
- `MAX_SPILL_BYTES = 64 * 1024 * 1024`。落盘文件写满即停，结果里注明只保存了前一部分。

设计见 [tools.md §7 read、§10 大结果落盘](../../../../docs/subsystems/tools.md)。

## 考虑过的方案

**沿用 DSH 的 50 KB。** 没有采用：超过 32 000 字节的结果仍会被投影从中间截断，违背“截断只发生在末尾”。

## 后果

- 有界的结果不会再被投影截断。投影的单条上限只是兜底。
- 参照任务回放按 4 字节 / token 估算。改造前的基线原始 84,650 token，投影后 42,927。加入结果上限与落盘之后是 41,251，只降了 4%。原因是投影本来就截掉了大结果。T0 的收益主要是正确性：不从中间截断，总数准确，完整结果可取回。
- 这个数字说明，压住累积上下文的不是单条上限。因此，旧结果修剪与先读后改提前到沙箱之前完成（[compaction.md §2](../../../../docs/subsystems/compaction.md)、[tools.md §7 先读后改](../../../../docs/subsystems/tools.md)）。
- 32 000 与投影上限是手工对齐的，代码里没有测试把两者绑在一起。改 `DEFAULT_MAX_TOOL_RESULT_TOKENS` 或估算比例时，要同时改 `MAX_RESULT_BYTES`。
- 每个 Session 的落盘总量没有上限，只有单个文件有上限。启动时清理 7 天前的目录。
