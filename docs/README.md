# OpenWork 文档索引

Last reviewed: 2026-06-26

## 架构与运行时

- [architecture-overview.md](architecture-overview.md)：当前整体架构、模块边界、核心数据流。
- [crate-restructure-plan.md](crate-restructure-plan.md)：目标 crate 结构、PostgreSQL-first 存储层、依赖方向与破坏性迁移顺序。
- [agent-runtime-and-tool-flow.md](agent-runtime-and-tool-flow.md)：Agent loop、工具调用、runtime 事件、doom-loop 检测。
- [local-postgres.md](local-postgres.md)：本地 Docker PostgreSQL 启动方式与 `DATABASE_URL` 约定。

## 权限、审批与工具安全

- [permissions-and-approvals.md](permissions-and-approvals.md)：权限数据模型、各工具权限、human-in-the-loop 审批流程、当前边界。

## Provider 与流式协议

- [ai-provider-integration-design.md](ai-provider-integration-design.md)：Provider 接入、核心 AI 类型、streaming、model registry 状态。
- [desktop-streaming-flow.md](desktop-streaming-flow.md)：Tauri stream event、前端状态累积、审批 UI、reload 策略。

## 持久化与观测

- [session-persistence-and-tracing.md](session-persistence-and-tracing.md)：PostgreSQL 表结构、messages/llm_events/tool_runs 分工、trace 方向。
- [file-diff-snapshot-revert-design.md](file-diff-snapshot-revert-design.md)：文件 diff、snapshot、review、revert 的设计边界和实现策略。

## 维护原则

- 文档应描述当前实现，不要把未实现能力写成已完成。
- 如果修改 `GenerateStreamEvent`、`AgentEvent`、`ContentBlock` 或 session schema，需要同步更新相关文档。
- 如果新增工具，需要同步更新权限表和工具调用流程文档。
- 如果修改根目录 README，需要同步检查 `README.md` 和 `README.en.md` 是否保持一致。
