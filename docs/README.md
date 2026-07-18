# OpenWork 文档索引

Last reviewed: 2026-07-18

## 当前权威文档

重构后的项目结构、运行时、数据库、Trace 和 Desktop 只以以下文档为准：

- [redesign/README.md](redesign/README.md)：范围、实施状态、明确非目标和已知暂缓项；
- [redesign/01-project-structure.md](redesign/01-project-structure.md)：五个 Rust crate、模块职责和依赖方向；
- [redesign/02-event-update-model.md](redesign/02-event-update-model.md)：Session、Turn、Model Call、Tool Call、Permission 和 Live Update；
- [redesign/03-database-schema.md](redesign/03-database-schema.md)：SQLx 干净基线与六张业务表；
- [redesign/04-trace-design.md](redesign/04-trace-design.md)：Turn 下的 Model Call/Tool Call Trace 与降级边界；
- [redesign/05-refactor-roadmap.md](redesign/05-refactor-roadmap.md)：迁移结果、验收 Gate 和未关闭项；
- [redesign/06-frontend-architecture.md](redesign/06-frontend-architecture.md)：Tauri/React 边界、Runtime Store、Trace 页面与暂缓的 Host Contract 生成。

本地数据库启动、迁移、检查和重建见 [local-postgres.md](local-postgres.md)。

## 当前实施边界

- `openwork-core` 是唯一 Session Runtime 和组合入口；
- 一次用户输入对应一个 Turn，工具结果由同一 Agent Loop 送入下一次 Model Call；
- PostgreSQL 使用 SQLx migration，不保留 Event Journal、旧表回填或 `legacy_*` 路径；
- 未完成 Turn 在启动时标记为 `interrupted`，不自动恢复或重放工具；
- Trace 是 best-effort 诊断数据，不参与业务推进；
- Desktop 通过 Tauri Command/Event 使用 Core，不复制后端状态机。

已知暂缓项只有两组：Rust → TypeScript Host Contract/Drift Check，以及 Trace 关闭入口/完整降级验收。它们保留在 `docs/redesign/` 的完成标准中，但当前不实施。

## 维护原则

- 源码和 SQLx migration 回答“当前实际是什么”；
- `docs/redesign/` 同时记录目标、已实施结果和明确暂缓项，不再建立平行蓝图；
- 领域词汇统一使用 Session、Turn、Model Call、Tool Call、Permission、Message、Update 和 Trace；
- 修改根目录 README 时同步检查 `README.md` 与 `README.en.md`；
- 本轮不引入跨进程 Turn 恢复、Event Journal、Memory、MCP、Skill、Plan、Compaction、Artifact、Git/Diff 或 Worktree 能力。
