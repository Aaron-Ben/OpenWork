# Crew 文档

`docs/subsystems/` 每页描述一个子系统已经实现的行为、边界与验收。决策理由与还没实现的设计写在 [Agent Notes](../.agents/notes/README.md)。文档分层与写作规则见 [AGENTS.md](AGENTS.md)。

## 根目录

| 文档 | 内容 |
|---|---|
| [architecture.md](architecture.md) | 进程与通信、包与依赖方向、数据归属、领域词汇 |
| [local-services.md](local-services.md) | 本地 PostgreSQL 的启动、检查与重建 |
| [testing.md](testing.md) | 测试分层、原则、写法与运行方式 |
| [defensive-patterns.md](defensive-patterns.md) | 本仓库实际出现过的缺陷与对应的写法 |
| [AGENTS.md](AGENTS.md) | 文档标准 |
| [templates/package-readme.md](templates/package-readme.md) | 包 README 的结构与写法 |

## 子系统

| 文档 | 内容 |
|---|---|
| [messaging.md](subsystems/messaging.md) | Server：数据模型、消息与序号、已读位置、运行期状态、接口与 SSE |
| [agent-runtime.md](subsystems/agent-runtime.md) | Computer：启动与停止、Runner 与 Turn、OpenCode、Seatbelt、本机目录、`crew` 命令 |

## 包

每个包的入口、源码地图与对模型上下文的影响。

| 包 | 内容 |
|---|---|
| [@crew/protocol](../packages/protocol/README.md) | 接口契约、branded ID、启动握手、SSE 读取 |
| [@crew/server](../packages/server/README.md) | Collaboration Server |
| [@crew/computer](../packages/computer/README.md) | Agent 宿主与 `crew` 命令 |
| [@crew/desktop](../apps/desktop/README.md) | Electron 主进程与界面 |

## 事实来源

| 问题 | 看哪里 |
|---|---|
| 现在实际是什么 | 源码与 `packages/server/drizzle/` 中的迁移；子系统页与它们一致 |
| 为什么这样设计 | [`.agents/notes/`](../.agents/notes/README.md) |
| 怎样运行、有哪些命令 | 仓库根 [README.md](../README.md) 与 [AGENTS.md](../AGENTS.md) |

子系统页与代码不一致时，以代码为准，并在同一个改动里修正子系统页。

## 阅读顺序

第一次接触这个项目：

1. [architecture.md](architecture.md)：整体怎样组成。
2. [messaging.md](subsystems/messaging.md)：消息怎样保存与通知。
3. [agent-runtime.md](subsystems/agent-runtime.md)：Agent 怎样被唤醒、运行与回复。
