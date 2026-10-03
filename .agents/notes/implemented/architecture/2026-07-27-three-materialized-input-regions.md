# Agent Note: 模型输入的三个区域分别物化

Status: implemented

## 问题

一次 Model Call 的输入有三类来源：Agent 定义、对话历史、可用工具。它们的权威来源、变化频率和生命周期都不同。Agent 定义几乎不变，对话每次追加都变，工具面随 Agent 与沙箱状态变。

如果把三者混在一份可变结构里，任何一方的变化都会牵动另外两方。最典型的是压缩：压缩只该替换对话。混在一起时，摘要会顺带吸收 system prompt 或工具定义，成为它们的一份过时副本。

## 决策

- System Context、Conversation、Tool Surface 各自物化，只在 `ContextEngine::prepare`（`crates/openwork-core/src/context/engine.rs`）汇合。
- 组装只做确定性工作：校验、测量、排序、生成 `ModelRequest`。它不读文件，不改 Chat State，不调用 Provider。
- 工具定义与分派来自同一个 `FinalizedToolset`，避免“广告了但调不动”或“能调但没广告”。
- Chat State 只保存已经物化、需要随对话重放的条目，不读取 Skill、计划或 `AGENTS.md` 的来源。
- 新能力先问能不能落在已有工具上。Skill 不新增工具：目录里给出路径，模型用 `read` 打开正文。

事实见 [context-window.md §1、§6、§9](../../../../docs/subsystems/context-window.md)。

## 考虑过的方案

**把所有上下文都放进 Chat State。** 没有采用，理由就是上面的压缩问题。

**Skill 正文放进 System Context。** 没有采用。压缩只替换 Conversation，放在 System Context 里的正文永远回收不掉。

**Skill 目录放进 System Context。** 最初采用，后来与 `AGENTS.md`、项目布局一起移进 World State。理由见 [Agent Note：Skill 目录作为 world state section](2026-08-14-skill-catalog-world-state-section.md) 与 [Agent Note：World State 以追加消息进入 Conversation](2026-08-15-world-state-as-appended-messages.md)。Skill 复用 `read` 的理由见 [Agent Note：Skill 用 read 打开](2026-08-07-skills-use-read-instead-of-a-skill-tool.md)。

**预建通用的上下文来源注册表。** 没有采用。只有出现第二个需要独立更新与恢复的动态来源时，才抽出通用接口。World State 出现后，代码仍用四个具名字段，没有注册表。

## 后果

- 压缩可以只换对话，System Context 与工具面每次从来源重新物化。
- 新能力必须先决定落在哪个区域，不能直接塞进 Chat State 或组装函数。
- 组装函数保持纯粹，上下文检查与真实请求共用它，所以两者不会各自组装出不同的结果。
- 三个区域各自测量，预算估算能指出压力来自哪一部分。
