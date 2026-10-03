# Agent Note: Skill 复用 read，不加 skill 工具

Status: implemented

## 问题

模型需要按需加载 skill 的正文、参考文档和脚本。要决定两件事：加一个专用工具，还是复用已有工具；Core 要不要记录本 Session 已加载过哪些 skill。

## 决策

- 目录给出每个 `SKILL.md` 的绝对路径。模型用 `read` 读正文与 `references/`，用 `bash` 运行 `scripts/`。
- 内置工具里没有 `skill` 工具（`crates/openwork-tools/src/builtins/mod.rs`）。
- Core 不记录 Session 已加载的 skill。
- 正文作为 Tool Result 进入 Conversation，可以被压缩回收，并产生普通的 `tool_call` Span。

行为见 [skills.md §3.3](../../../../docs/subsystems/skills.md)。

## 考虑过的方案

**专用的 `skill(name)` 工具。** 没有采用，理由有三条。第一，它与 `read` 做同一件事；正文引用 `references/` 时，模型还要切回 `read`。第二，每次 Model Call 都带工具定义，成本常驻；目录已经告诉模型有哪些 skill。第三，Codex 的文件系统 skill 也没有专用工具：目录给出路径，模型自己打开。它的 `skills.read` 只服务没有文件系统路径的 executor 与 orchestrator package（`codex-rs/ext/skills/src/render_tests.rs` 中的提示原文，工具在 `codex-rs/ext/skills/src/tools/`）。

**记录本 Session 已加载的 skill，避免重复读取。** 没有采用。压缩后，正文可能已不在 Conversation 里，而集合仍记得“已加载”。模型会以为自己看得见正文。

## 后果

- 模型能 `read` 任何已知路径的 skill，包括已停用的 skill。目录不列出不等于访问控制。
- skill 没有新的读文件或执行方式，`read` 与 `bash` 的边界自动适用。
- 模型重复读同一个 skill 时，要再付一次正文的 token。
