# OpenWork 文档

一个功能一篇文档。每篇描述**这个功能是什么、边界在哪、怎么验收**，不记录迁移过程。

跨功能且容易混淆的领域术语，以 [architecture.md](architecture.md) 为准。具体的 interface、状态机和验收，仍以下面各篇 owning 文档为准。

## 索引

| 文档 | 内容 |
|---|---|
| [architecture.md](architecture.md) | crate 划分、依赖方向、核心不变量、领域词汇 |
| [session-runtime.md](session-runtime.md) | Session/Turn 状态机、Agent Loop、Tool Call 生命周期、Update 协议、中断语义 |
| [context-window.md](context-window.md) | 三条物化链（System Context / Conversation / Tool Surface）、组装边界、预算估算 |
| [compaction.md](compaction.md) | 四类压缩触发、摘要格式与重试、运行状态、checkpoint、三类恢复 |
| [trace.md](trace.md) | 质量追踪：内容、token 口径、标注；三层标识、Span 语义、完整度派生 |
| [tools.md](tools.md) | 工具四层契约、权限两分、路径安全、七个内置工具 |
| [update-plan.md](update-plan.md) | Turn 级任务清单、Core 控制工具、持久化与 Desktop 投影 |
| [skills.md](skills.md) | Skill 目录契约、`$` 精确路径选择、三层渐进披露、只读边界 |
| [multi-agent.md](multi-agent.md) | 只读子 Agent：身份与拓扑、五个控制工具、mailbox 与信封、并发限额、非交互授权、重启对账 |
| [collaboration.md](collaboration.md) | 本机 BYOA Runtime、身份、通信、AgentRunner、消息协调、Board、Agenda 与存储 |
| [collaboration-desktop.md](collaboration-desktop.md) | macOS Desktop supervisor、Tauri command、SSE 投影与协作界面 |
| [permissions.md](permissions.md) | 文件沙箱、`auto` / `accept-edits` 两模式、四档路径、被拒后一次性越界、危险命令检测、审批卡片、会话状态 |
| [data-model.md](data-model.md) | 全部表的 DDL 与约束理由、写入顺序、启动修正 |
| [desktop.md](desktop.md) | Tauri Bridge、前端状态三层、Reducer、Trace UI |
| [local-postgres.md](local-postgres.md) | 本地数据库启动、迁移、检查与重建 |

进行中的开发计划放在 `plans/`。计划回答按什么顺序做、做到什么程度算完成、何时需要决策。计划**不定义功能**，完成后删除。当前计划：[plans/sandbox-and-tools.md](plans/sandbox-and-tools.md)、[plans/collab-core.md](plans/collab-core.md)、[plans/collab-opencode.md](plans/collab-opencode.md)。

需要保留的外部参考资料，放在 `references/`。它们只描述其他项目，不约束 OpenWork。OpenWork 采纳的决定，必须写进对应的 owning 文档。

## 事实来源

| 问题 | 看哪里 |
|---|---|
| 当前实际是什么 | 源码 + 各 owning crate 的 `migrations/`；尚未实现的目标以文档中的提示为准 |
| 协作 Runtime 和业务语义 | [collaboration.md](collaboration.md) |
| 协作存储与并发约束 | [collaboration.md §13](collaboration.md) |
| 协作 Desktop 投影 | [collaboration-desktop.md](collaboration-desktop.md) |
| 其他功能应该是什么、为什么 | 本目录对应 owning 文档 |
| 怎么跑起来、有哪些命令 | 仓库根 [README.md](../README.md) 的“快速开始” |

Owning 文档描述当前约束和已确认的目标。**代码与目标有差距时，在该篇的“尚未实施”小节中明确列出差距。**没有这类提示的内容，应与实现和自动化证据一致。

## 阅读顺序

第一次接触这个项目：

1. [architecture.md](architecture.md) —— 建立词汇和边界
2. [session-runtime.md](session-runtime.md) —— 一次请求怎么跑完
3. [context-window.md](context-window.md) —— 模型每次看到什么
4. 按需读 [compaction.md](compaction.md) / [tools.md](tools.md) / [update-plan.md](update-plan.md) / [trace.md](trace.md) / [skills.md](skills.md) / [multi-agent.md](multi-agent.md)；协作模式先读 [collaboration.md](collaboration.md)，再按需读 Desktop 文档

## 维护原则

- **一个功能一篇文档。** 新增能力时，先判断它属于哪一篇。只有它有独立的生命周期、失败语义和验收标准时，才新开一篇。
- **每篇自带验收清单。** 没有验收标准的设计描述，就没有约束力。
- **`references/` 不受上面两条约束**，也不描述 OpenWork 的目标状态。调研外部项目的结论放在这里。落到 OpenWork 的决定，必须写回对应的功能文档才生效。
- **不写迁移叙事。** "以前是什么样"属于 git 历史，不属于文档。
- **按 ASD-STE100 写。** 中文文档也一样，规则见仓库根 [CLAUDE.md](../CLAUDE.md) 的“写作”一节。
- 统一使用这些领域词汇：Session、Turn、Model Call、Tool Call、Permission、Message、Update、Trace、Compaction。
- 修改根目录 README 时，同时检查 `README.md` 与 `README.en.md`。
