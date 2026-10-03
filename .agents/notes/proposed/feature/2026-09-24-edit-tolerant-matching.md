# Agent Note: edit 容错匹配

Status: proposed

## 问题

edit 失败最常见的原因不是意图错了，而是 `oldString` 的缩进、空白或转义与文件不一致。每次失败，模型都要重读文件再试一次。那一轮的上下文代价，远大于容错匹配本身的风险。

现在 edit 只做精确匹配（`crates/openwork-tools/src/builtins/filesystem/edit.rs` 的 `apply_edit`），见 [tools.md §7.3](../../../../docs/subsystems/tools.md)。

## 提议

按顺序尝试四种策略：

| 顺序 | 策略 | 容忍的差异 |
|---|---|---|
| 1 | `exact` | 无 |
| 2 | `line-trimmed` | 每行首尾空白 |
| 3 | `whitespace` | 连续空白折叠 |
| 4 | `escape` | 换行、引号等写成了转义形式 |

- 每种策略都要求唯一匹配。找到多处即失败，不往下一种走。
- 非精确策略要求 `oldString` 去空白后至少 5 个字符，且匹配到的范围不远大于 `oldString`。
- `newString` 原样写入，不重新缩进。
- 摘要注明使用的容忍方式，例如 `Edited src/a.rs:120-128 (+3 -1, matched ignoring indentation)`。
- 移植时保留来源许可声明（MIT 与 Apache-2.0），逐段注明出处。

2026-09-24 对照 ZCode 时，还记下两种容忍：`oldString` 带 read 输出的行号前缀，以及弯引号与直引号不一致。ZCode 有这两种匹配（`apps/zcode-cli/packages/core/src/tool/edit-matchers.ts` 的 `collectLineNumberPrefixCandidates`、`collectQuoteNormalizedCandidates`）。是否加入，实现前再定。

## 考虑过的方案

**照搬 maka 的实现。** maka 的 `packages/runtime/src/edit-replace.ts` 只保留 `line-trimmed`、`whitespace`、`escape` 三种整段匹配。它删去 opencode 的 block-anchor 与 context-aware，因为它们只凭首尾行这类部分信号定位。这个文件逐段写明了来自 opencode、cline 与 gemini-cli 的许可。提议采用同样的取舍。

**改用 `apply_patch` 补丁语言。** 没有采用：可靠的补丁需要另外定义多文件、hunk 定位、偏移容忍、部分失败与回滚。没有这些契约就把两种编辑方式塞进一个工具，结果更难预测。

**保持精确匹配。** 这是现状。代价是缩进不一致造成的失败重试。

## 验收条件

- tools.md 验收一节原第 16 条：`oldString` 不存在、在同一策略下出现多次、或与 `newString` 相同时，edit 失败。
- 原第 29 条：缩进、空白或转义不一致的 `oldString` 在唯一匹配时成功，摘要注明使用的容忍方式；任一策略下出现多处匹配即失败。
- 原第 30 条：去空白后少于 5 个字符的 `oldString` 不做非精确匹配。
- 每段移植代码都带来源许可声明。

## 风险

- 模糊匹配落在错误位置。唯一匹配、最短长度与范围检查是防线，maka 还限制了可做模糊匹配的源文件大小。
- 匹配成功后，模型可能以为文件里的写法就是它给的写法。摘要中的容忍方式提示它真实写法不同。
- 本仓库是 Apache-2.0，根目录没有第三方声明文件。移植时要决定在哪里登记 MIT 与 Apache-2.0 的来源声明。
