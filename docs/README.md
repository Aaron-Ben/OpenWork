# OpenWork 文档

`docs/subsystems/` 每页描述一个子系统已经实现的行为、边界与验收。决策理由与还没实现的设计写在 [Agent Notes](../.agents/notes/README.md)。文档分层、写作规则与字数上限见 [AGENTS.md](AGENTS.md)。

跨功能的领域术语以 [architecture.md](architecture.md) 为准。

## 根目录

| 文档 | 内容 |
|---|---|
| [architecture.md](architecture.md) | crate 划分、依赖方向、核心不变量、领域词汇 |
| [local-postgres.md](local-postgres.md) | 本地数据库启动、迁移、检查与重建 |
| [testing.md](testing.md) | 测试分层、原则、写法与运行方式 |
| [AGENTS.md](AGENTS.md) | 文档标准 |
| [templates/crate-readme.md](templates/crate-readme.md) | crate README 的结构与写法 |

## 子系统

| 文档 | 内容 |
|---|---|
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
2. [collaboration.md](subsystems/collaboration.md)：协作 Runtime 怎样工作。
3. [collaboration-desktop.md](subsystems/collaboration-desktop.md)：界面怎样驱动 Runtime。
