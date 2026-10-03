# Agent Note: 显式选择 Skill 绑定精确路径

Status: implemented

## 问题

用户想在消息里指定一个 skill。提交时按名称重新查找，会在 skill 被删除、改名或参数被伪造时选错文件。选中的正文还要决定进入哪条上下文链。

## 决策

- 输入框只显示 `$name`，另存 `{ start, end, name, path }` 绑定（`desktop/src/features/chat/skillMentions.ts`）。
- Desktop 提交有序的 `UserInput[]`。Core 在创建 Turn 前用 `resolve_selected_skills` 按 canonical path 与 name 校验（`crates/openwork-core/src/skills/selection.rs`）。失败时返回 `skill_unavailable`。
- 正文是一条 `message_kind = 'skill_instruction'` 的 User-role Text Message，写在用户可见消息之前。
- 手写的 `$name` 是普通文本。Core 不扫描它。

做法来自 Codex：`UserInput::Skill { name, path }`（`codex-rs/protocol/src/user_input.rs`）与输入框的 `MentionBinding`（`codex-rs/tui/src/bottom_pane/mod.rs`）。行为见 [skills.md §4](../../../../docs/subsystems/skills.md)。

## 考虑过的方案

**只提交名称，由 Core 按名称解析。** 没有采用。名称只用于显示，删除与改名后会静默选到另一份文件。

**把选择记成一次 `read` Tool Call。** 没有采用。那次调用没有发生，Trace 会记录不存在的事实。

**扫描文本里的 `$name` 并隐式激活。** 没有采用。`$HOME` 这类 shell 变量会被误认，用户也看不出哪个 token 已绑定。

**用 `contenteditable` 渲染 token。** 没有采用。受控 `textarea` 保留原生光标、IME 与无障碍文本，底色由同尺寸的只读层绘制。

**给 Span 加 `skill_name` / `skill_path` 属性。** 没有采用。显式选择可从 `messages.content` 读出，模型读取可从 Tool Call 输入读出，达不到 [trace.md §12](../../../../docs/subsystems/trace.md) 的新增门槛。

## 后果

- 选择后的删除、停用与改名都在 Turn 被接受前失败，Desktop 保留草稿。
- 正文是历史快照，resume 不重读磁盘。
- 只有一个 skill 根，同名不会出现，所以不定义覆盖顺序。
