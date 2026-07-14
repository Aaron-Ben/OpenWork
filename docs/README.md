# OpenWork 文档索引

Last reviewed: 2026-07-11

## 目标架构与路线

- [../plans/openwork-core-architecture-blueprint.md](../plans/openwork-core-architecture-blueprint.md)：唯一的目标架构与建设顺序，定义 `openwork-core`、Protocol、Capabilities、Execution、Workspace、Persistence 等模块边界。
- [../plans/event-journal-persistence-refactor.md](../plans/event-journal-persistence-refactor.md)：S2 Persistence 专题设计，冻结 `recorded_events`、Expected Version、事件边界、显式迁移和三旧 crate 的退出顺序。
- [../plans/desktop-tauri-application-boundary-refactor.md](../plans/desktop-tauri-application-boundary-refactor.md)：S7 Desktop 收口专题设计，定义 Tauri Host、Application API、Command/Query/Subscription 和启动/退出边界。

## 当前实现参考

- [architecture-overview.md](architecture-overview.md)：当前整体架构、模块边界和核心数据流，不定义未来路线。
- [agent-runtime-and-tool-flow.md](agent-runtime-and-tool-flow.md)：Agent loop、工具调用、runtime 事件、doom-loop 检测。
- [local-postgres.md](local-postgres.md)：当前代码所需的本地 PostgreSQL 开发方式，不代表目标 Persistence 方案。

## 权限、审批与工具安全

- [permissions-and-approvals.md](permissions-and-approvals.md)：权限数据模型、各工具权限、human-in-the-loop 审批流程、当前边界。
- [../plans/capability-tool-observation-design.md](../plans/capability-tool-observation-design.md)：Capability 合同、Catalog、Action Handler、Execution 与 Observation 的当前专题设计。

## Provider 与流式协议

- [model-provider-v1-design.md](model-provider-v1-design.md)：Model Port、厂商 Adapter、错误分类、流式重试闸门、Provider Repository 与 PostgreSQL 表结构。
- [desktop-streaming-flow.md](desktop-streaming-flow.md)：Tauri stream event、前端状态累积、审批 UI、reload 策略。

## 持久化与观测

- [session-persistence-and-tracing.md](session-persistence-and-tracing.md)：当前迁移状态、Journal-backed Session/Turn/Message、遗留表删除状态和未完成边界。

## 维护原则

- 未来架构、模块所有权和建设顺序只以 `openwork-core-architecture-blueprint.md` 为准。
- 其余文档只描述当前代码或某个专题，不再各自维护第二套路线路径。
- 文档应描述当前实现，不要把未实现能力写成已完成。
- 如果修改 `ModelEvent`、`AgentEvent`、`ContentBlock` 或 PostgreSQL schema，需要同步更新相关文档。
- 如果新增内置 Action，需要同步更新 Capability Catalog、Execution Handler、名称对齐测试、权限表和工具调用流程文档。
- 如果修改根目录 README，需要同步检查 `README.md` 和 `README.en.md` 是否保持一致。
