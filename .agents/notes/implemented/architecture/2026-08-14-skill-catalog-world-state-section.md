# Agent Note: Skill 目录作为 world state section

Status: implemented

## 问题

模型要先知道有哪些 skill，才能决定读哪个 `SKILL.md`。目录在会话中途会变：用户新装、删除或停用 skill。压缩之后，目录也必须仍然可见。目录写在 System 前缀里时，每次变化都改写前缀字节，整段对话的提示缓存随之失效。

## 决策

- 目录是 world state 的 `skills/catalog` section：`SkillsCatalogState`（`crates/openwork-core/src/context/world_state/skills_catalog.rs`）。正文由 `SkillCatalogLoader::load_body` 渲染（`crates/openwork-core/src/context/skill_catalog.rs`）。
- run loop 在每次 Model Call 前采样。目录没变就不发；变了就追加带取代声明的全量正文；变成空时发失效声明。
- 压缩换掉目录消息后，`RetainedSections::scan` 发现消息不在投影里，下一次采样重发正文。
- System 前缀只有 `core/agent-system` 一段。

行为见 [skills.md §3.2](../../../../docs/subsystems/skills.md)。

## 考虑过的方案

**目录作为 System Context part。** 2026-08-07 的实现用 key `skills/catalog` 放在 System Context 里，每个 Turn 重新物化，压缩前后都在。没有保留：任何 skill 变化都改写前缀，使提示缓存失效（`crates/openwork-core/src/context/builder.rs` 的测试注释）。

**首轮注入一条普通 Conversation 消息。** 原设计拒绝了它：压缩投影只保留最后一条用户请求、摘要和边界后的消息，目录会丢失。补回目录要给压缩投影加特例。world state 的自愈扫描承担了这件事，并且四个 section 共用。

Codex 也把 skill 目录做成 world state section，以 developer 角色发送（`codex-rs/ext/skills/src/world_state.rs`、`codex-rs/ext/skills/src/fragments.rs`）。

## 后果

- 目录变化只追加一条消息。前缀与已有消息的字节不变。
- 每次采样都扫描一次 skill 根。最多加载 100 个 skill，代价有限。
- 摘要请求排除 world state 消息，过时的目录不会冻结进摘要。
- 进程重启后基线为空，目录带取代声明重发一次。
