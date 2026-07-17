# OpenWork 文档索引

Last reviewed: 2026-07-16

## 目标设计

本轮项目结构、数据模型、数据库和 Trace 的目标设计只以以下文档集为准：

- [redesign/README.md](redesign/README.md)：范围、权威性和总览；
- [redesign/01-project-structure.md](redesign/01-project-structure.md)：完整 Core Runtime、能力 crate 和单向依赖；
- [redesign/02-event-update-model.md](redesign/02-event-update-model.md)：Session/Prompt/Model Call/Tool Call/Permission 与数据面；
- [redesign/03-database-schema.md](redesign/03-database-schema.md)：简化后的 8 张目标 PostgreSQL 表；
- [redesign/04-trace-design.md](redesign/04-trace-design.md)：Core 内单表 Prompt Trace、内嵌 Event 和埋点规则。

这些文档是设计规格，不表示代码、Migration 或 Desktop 已经实施。

## 当前实现参考

以下文档用于理解仓库当前行为；当文档与源码不一致时，以源码为准：

- [architecture-overview.md](architecture-overview.md)：当前整体架构与模块关系；
- [agent-runtime-and-tool-flow.md](agent-runtime-and-tool-flow.md)：当前 Agent loop、工具和 doom-loop；
- [desktop-streaming-flow.md](desktop-streaming-flow.md)：当前 Tauri Live Event 与前端累积；
- [durable-turn-lifecycle.md](durable-turn-lifecycle.md)：当前 Recorded/Live Event 与 Turn 生命周期；
- [session-persistence-and-tracing.md](session-persistence-and-tracing.md)：当前 Journal、Session 重放与 Trace 基线；
- [permissions-and-approvals.md](permissions-and-approvals.md)：当前权限和审批合同；
- [model-provider-v1-design.md](model-provider-v1-design.md)：当前 Provider/Model 适配边界；
- [local-postgres.md](local-postgres.md)：当前本地 PostgreSQL 使用方式。

## 历史设计与过程资料

以下文档保留作为背景和实现演进证据，不再约束本轮目标结构：

- [../plans/openwork-core-architecture-blueprint.md](../plans/openwork-core-architecture-blueprint.md)；
- [../plans/event-journal-persistence-refactor.md](../plans/event-journal-persistence-refactor.md)；
- [../plans/desktop-tauri-application-boundary-refactor.md](../plans/desktop-tauri-application-boundary-refactor.md)；
- [../plans/capability-tool-observation-design.md](../plans/capability-tool-observation-design.md)；
- [trace-bata-design.md](trace-bata-design.md)：已由新 Trace 设计替代。

## 判断顺序

1. 源码与现有 Migration 回答“现在实际是什么”；
2. `docs/redesign/` 回答“重构后的目标是什么”；
3. 其他 `docs/` 与 `plans/` 回答“此前为什么这样设计或实现”；
4. 旧文档不得覆盖新目标，也不能把目标设计描述成已实现。

## 维护原则

- 修改当前实现后，同步更新对应的“当前实现参考”；
- 修改目标边界时，只在 `docs/redesign/` 更新，避免再产生平行蓝图；
- Core 是完整 Session Runtime；目标领域词汇统一使用 Prompt、Model Call、Tool Call 与 Permission；
- Session State、Event、Update、Chat History、Signals 与 Trace 必须始终分别说明用途；
- 数据库设计必须区分模型配置、运行快照、统计、UI 回放、模型上下文和 Trace；
- 如果修改根目录 README，继续同步检查 `README.md` 和 `README.en.md`；
- 本轮设计不引入 Memory、MCP、Skill、Plan、Compaction、Artifact、新工具或新 Worktree 能力。
