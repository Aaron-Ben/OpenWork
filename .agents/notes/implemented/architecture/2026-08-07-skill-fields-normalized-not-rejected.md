# Agent Note: Skill 字段归一化，不拒绝

Status: implemented

## 问题

目录是按行的格式。某个字段带换行，就能在目录里伪造一条不存在的 skill。另一方面，一个手写的 `SKILL.md` 出错，不应让整个应用不能使用。

## 决策

- `name` 与 `description` 先做空白归一化（`split_whitespace().join(" ")`），再校验（`crates/openwork-core/src/skills/parser.rs`）。目录行里不会出现换行。
- 路径含控制字符时，跳过该 skill，不做归一化。归一化后的路径不能再交给 `read`。
- 单个 skill 失败只产生一条 warning，不影响其他 skill，也不让 Turn 失败（`crates/openwork-core/src/skills/discovery.rs`）。

行为见 [skills.md §1.3、§1.4](../../../../docs/subsystems/skills.md)。

## 考虑过的方案

**三层注入防护。** 早期设计有三层：拒绝含 C0 字符的 description，拒绝含标签记号的正文，渲染后做结构断言。它还要处理 `</skill >`、`<skill/>` 和带属性的标签等变体。没有采用：归一化后同样不能伪造条目，而且没有失败路径，也没有要向用户解释的错误类别。

**与 `AGENTS.md` 一样确定性失败。** 没有采用。`AGENTS.md` 是项目唯一的权威指令，读取出错时 Turn 应停止。skill 是一组互相独立的可选包。

## 后果

- 归一化只保证目录格式，不是安全审查。一行正常的描述仍能诱导模型，见 [Agent Note：Skill 信任边界](2026-08-07-skill-trust-boundary.md)。
- 用户在列表里看到每条失败的路径与原因。
- 描述里有意写的换行会变成空格。
