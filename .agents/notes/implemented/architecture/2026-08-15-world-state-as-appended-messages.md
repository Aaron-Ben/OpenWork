# Agent Note: World State 以追加消息进入 Conversation

Status: implemented

## 问题

项目布局、`AGENTS.md` 和 Skill 目录原先是 System 前缀的三个 part。Agent 自己就能改变它们：新建一个顶层目录，或编辑 `AGENTS.md`。前缀中任何一个字节变化，都会让整段对话的提示缓存失效，长对话每轮都按全价重新计算。

这些内容又必须是最新的。Agent 在 Turn 中途改了 `AGENTS.md`，下一次 Model Call 就应当看到。

## 决策

- System 前缀只剩 `core/agent-system`，在 Actor 生命周期内逐字节不变（`crates/openwork-core/src/context/builder.rs`）。
- 四个 section（项目上下文、`AGENTS.md`、Skill 目录、沙箱策略）在每次 Model Call 之前采样。变化的 section 以 `Role::User`、`MessageKind::WorldState` 追加到对话末尾（`crates/openwork-core/src/context/world_state/`）。
- 每个 section 全量重渲染，并自带取代或失效声明。一个 section 一条消息。
- 顺序是：采样 → 比较 → 逐条写库 → 追加 → 推进基线。写库失败时基线不推进。
- 是否重发，由内存基线与“消息还在不在投影里”共同决定。扫描先按 `MessageKind` 过滤。
- `AGENTS.md` 外面包一层 `<project_instructions>`。Codex 出于同样的理由把它包在 `<INSTRUCTIONS>` 里（`codex-rs/core/src/context/user_instructions.rs`）。
- 摘要输入去掉 World State 消息。

事实见 [context-window.md §2、§3](../../../../docs/subsystems/context-window.md)。

## 考虑过的方案

**保留在 System 前缀。** 没有采用，理由见“问题”。

**只在 Turn 开始时采样。** 没有采用。Agent 在 Turn 中途改动工作区后，要等下一个 Turn 才能看到自己动作的后果。

**几个 section 合成一条消息。** 没有采用。压缩后要逐个判断 section 的消息还在不在，合成一条就只能全部一起重发。

**增量渲染。** 没有采用。项目上下文只列顶层条目（最多 64 条），一次全量重渲染约 400 token，可以接受。

**只按“基线为空”判断进程重启。** 没有采用。没有 `AGENTS.md` 也没有 Skill 的仓库，每次新建 Session 都会为从未提供过的内容发两条失效声明。

**类型擦除的 section 注册表。** Codex 的 world state 用 `IndexMap<&str, Box<dyn ErasedWorldStateSection>>` 保存 section，扩展可以经 `add_extension_section` 加入（`codex-rs/core/src/context/world_state/mod.rs`）。OpenWork 没有扩展 API，所以用四个具名字段。

两个 section 另有专门的 Agent Note：[Skill 目录](2026-08-14-skill-catalog-world-state-section.md)、[沙箱策略](2026-09-24-sandbox-policy-world-state-section.md)。

## 后果

- 前缀缓存不再因工作区变化失效。
- 每次变化都追加一条消息，直到压缩之前都不会消失。频繁变化的工作区会让对话变长。
- 用户输入了同样的标记，也不会被当成 section 的消息。
- 进程重启后，仍在对话里的 section 带取代声明重发一次。
- 上下文检查不采样，看不到下一次调用才会追加的 section。
