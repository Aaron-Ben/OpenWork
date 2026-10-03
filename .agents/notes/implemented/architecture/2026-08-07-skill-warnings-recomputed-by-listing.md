# Agent Note: Skill warning 由列表重新计算

Status: implemented

## 问题

发现与目录截断会产生 warning。用户需要在 Desktop 列表里看到每一条的具体原因。Turn 路径渲染目录时也会产生同样的 warning。要决定 warning 怎样从 Turn 路径到达用户。

## 决策

- `SkillCatalogLoader::load_body` 返回正文与 warning。`WorldStateCapture::capture` 对每条 warning 调用一次 `tracing::warn!`，然后丢弃（`crates/openwork-core/src/context/world_state/capture.rs`）。
- `WorldState` 与 `SkillsCatalogState` 不携带 warning。
- `list_skills` 调用同一个渲染函数，重新算出 warning（`crates/openwork-core/src/context/skill_catalog.rs`）。
- 两条路径使用 Core 持有的同一份 `SkillRoots` 与停用集合。目录上限是固定的 8000 字符，与模型窗口无关。所以截断结果只取决于扫描结果。

行为见 [skills.md §3.2](../../../../docs/subsystems/skills.md)。

## 考虑过的方案

这两个方案在目录还是 System Context part 时提出。目录移到 world state 后，同样的理由适用于 `WorldState`。

**把 warning 放进 `ResolvedSystemContext`。** 没有采用。它是给 `ModelRequestBuilder` 消费的物化结果，不是诊断通道。加一个字段后，每个消费者都要判断是否处理它。

**让 `SystemContextBuilder::build` 返回 `{ context, warnings }`。** 没有采用。三个构造点都要改签名，只为丢掉一个它们都不用的值。

## 后果

- 列表必须使用 Core 持有的 `SkillRoots`，不能自己读取 `HOME`。否则两次扫描的输入不同，warning 也不同。
- warning 只出现在列表与日志中，模型看不到。
- 如果截断规则改为依赖模型窗口，这个结论不再成立，要重新决定。
