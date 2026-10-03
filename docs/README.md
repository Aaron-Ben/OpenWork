# OpenWork 文档

`docs/subsystems/` 每页描述一个子系统已经实现的行为、边界与验收。决策理由与还没实现的设计写在 [Agent Notes](../.agents/notes/README.md)。文档分层、写作规则与字数上限见 [AGENTS.md](AGENTS.md)。

跨功能的领域术语以 [architecture.md](architecture.md) 为准。

## 根目录

| 文档 | 内容 |
|---|---|
| [architecture.md](architecture.md) | crate 划分、依赖方向、核心不变量、领域词汇 |
| [data-model.md](data-model.md) | 全部表的 DDL 与约束理由、写入顺序、启动修正 |
| [desktop.md](desktop.md) | Tauri Bridge、前端状态三层、Reducer、Trace UI |
| [local-postgres.md](local-postgres.md) | 本地数据库启动、迁移、检查与重建 |
| [testing.md](testing.md) | 测试分层、原则、写法与运行方式 |
| [AGENTS.md](AGENTS.md) | 文档标准 |
| [templates/crate-readme.md](templates/crate-readme.md) | crate README 的结构与写法 |

## 子系统

| 文档 | 内容 |
|---|---|
| [session-runtime.md](subsystems/session-runtime.md) | Session 命令、Turn 状态、Agent Loop、Tool Call 生命周期、SessionUpdate 与 Snapshot、进程中断 |
| [context-window.md](subsystems/context-window.md) | 一次请求的组成：System Context、World State、Conversation、Tool Surface；组装、预算与上下文检查 |
| [compaction.md](subsystems/compaction.md) | 四类触发、修剪旧工具结果、摘要格式、运行状态与提醒、checkpoint、恢复与回放 |
| [tools.md](subsystems/tools.md) | 工具四层契约、注册表与工具集、路径解析、内置工具、结果与落盘、上限 |
| [permissions.md](subsystems/permissions.md) | 文件沙箱、`auto` / `accept-edits` 两模式、四档路径、越界、危险命令检测、审批卡片 |
| [update-plan.md](subsystems/update-plan.md) | Turn 级任务清单、Core 控制工具、持久化与 Desktop 投影 |
| [skills.md](subsystems/skills.md) | Skill 目录契约、发现、显式选择、只读边界 |
| [multi-agent.md](subsystems/multi-agent.md) | 只读子 Agent：身份与拓扑、控制工具、通信、并发与非交互、对账 |
| [trace.md](subsystems/trace.md) | 质量追踪：标识、Span 语义、正文、完整度、查询与保留、Token 口径、属性 |
| [collaboration.md](subsystems/collaboration.md) | 协作 Runtime：身份、通信、AgentRunner、消息与唤醒、发布规则、Board、Agenda、存储 |
| [collaboration-desktop.md](subsystems/collaboration-desktop.md) | 协作界面：Tauri 监督进程与命令、事件与刷新、房间、Agent、看板、运行记录 |

## 事实来源

| 问题 | 看哪里 |
|---|---|
| 现在实际是什么 | 源码与各 crate 的 `migrations/`；子系统页与它们一致 |
| 为什么这样设计 | [`.agents/notes/implemented/`](../.agents/notes/README.md) |
| 打算怎样改 | `.agents/notes/proposed/` |
| 怎样运行、有哪些命令 | 仓库根 [README.md](../README.md) 的“快速开始” |

子系统页与代码不一致时，以代码为准，并在同一个改动里修正子系统页。

## 阅读顺序

第一次接触这个项目：

1. [architecture.md](architecture.md)：建立词汇和边界。
2. [session-runtime.md](subsystems/session-runtime.md)：一次请求怎样跑完。
3. [context-window.md](subsystems/context-window.md)：模型每次看到什么。
4. 按需读其他子系统页。协作模式先读 [collaboration.md](subsystems/collaboration.md)，再读 [collaboration-desktop.md](subsystems/collaboration-desktop.md)。
