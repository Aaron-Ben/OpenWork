# Agent Note: 按 Trace 数据决定工具链缓存与会话级越界授权

Status: proposed

## 问题

有两个问题现在没有答案。

第一，工具链缓存是否默认可写。Go 模块缓存 `~/go/pkg/mod`、npm 与 pnpm 缓存、`~/.cargo/registry` 现在都不可写，首次下载依赖要越界一次。

第二，是否需要会话级越界授权，以及它的范围按什么界定。现在每次越界都要批准，提交类 git 操作每次一张卡片。

两个问题都取决于真实使用中哪些越界反复出现。没有数据时，任何结论都是猜测。

## 提议

- 用 Tool Span 的 `escalationPaths`、`sandboxDenied`、`permissionDecision` 与 `permissionDecisionSource` 做统计（`crates/openwork-core/src/session/tool_trace_attributes.rs`）。
- 按路径聚合：哪些路径反复被申请，用户批准与拒绝各多少次，哪些调用被内核拒绝。
- 按结果分别回答缓存与会话授权两个问题。不预设结论，结论可能是两者都不需要。
- 得出结论后，在实现它的同一个改动里，把本 Note 改写为 implemented，并更新子系统页。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**现在就把工具链缓存加入可写根。** 没有采用：如果沙箱内的进程能写用户的缓存，用户在沙箱外构建时就可能取到篡改过的产物。Go 编译缓存改为重定向到私有临时目录，见 [Go 编译缓存重定向到 OpenWork 私有的临时目录](../../implemented/architecture/2026-09-24-go-build-cache-redirect.md)。

**现在就加回会话授权。** 2026-08-01 的设计有「本会话允许」。maka 把批准的路径累加进会话边界（`packages/core/src/sandbox-boundary.ts` 的 `applySandboxBoundaryExpansion`）。没有采用：理由见 [越界只列具体路径，只批这一次](../../implemented/architecture/2026-09-24-path-only-single-use-escalation.md)。

## 验收条件

- 能从 Trace 存储中按 `escalationPaths[].path` 聚合出申请次数、批准次数与拒绝次数。
- 能区分被内核拒绝的调用，并知道被拒绝的是哪个路径。
- 缓存与会话授权各有一条结论，并给出数据依据。

## 风险

- `sandboxDenied` 只是布尔值，Trace 不记录被拒的路径。被拒的那一行只出现在工具结果文本里，`denial_line`（`crates/openwork-core/src/session/run_loop/authorization.rs`）只把它放到卡片上。要回答“被拒的是哪些路径”，需要新增属性或解析结果文本。
- `sandboxDenied` 是推断。其他原因产生的 `EPERM` 也会计入。
- Trace 是有损的。完整度为 `partial` 或 `none` 的会话会少算次数。
- 数据只来自少数使用者，样本可能偏小。
- 路径里有用户名与项目名。聚合前要把 `$HOME` 与工作区根替换成占位符。
