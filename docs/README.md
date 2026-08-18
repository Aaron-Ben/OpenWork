# OpenWork 文档

一个功能一篇文档。每篇描述**这个功能是什么、边界在哪、怎么验收**，不记录迁移过程。

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
| [collaboration.md](collaboration.md) | 协作模式：常驻对等 Agent、独立 daemon、OpenCode 引擎、MCP 动作面、权限与审批、房间与看板、triage 与发言竞争 |
| [collaboration-desktop.md](collaboration-desktop.md) | 协作模式桌面端：两个 Shell 与 mode 切换、Rail 与三栏、事件通道、消息分页、待审批角标、复用边界 |
| [collaboration-data-model.md](collaboration-data-model.md) | 协作模式的 13 张 `collab_*` 表：DDL、约束理由、写入顺序、不建表的东西、保留期 |
| [permissions.md](permissions.md) | 效果模型、只读判定、`default` / `acceptEdits` 两模式、内置规则、命令解析、审批卡片、会话状态 |
| [data-model.md](data-model.md) | 全部表的 DDL 与约束理由、写入顺序、启动修正 |
| [desktop.md](desktop.md) | Tauri Bridge、前端状态三层、Reducer、Trace UI |
| [local-postgres.md](local-postgres.md) | 本地数据库启动、迁移、检查与重建 |

规范类文档在 [`.claude/rules/`](../.claude/rules/)：目前有 [database.md](../.claude/rules/database.md)（时间字段与迁移规范）。

外部参考资料在 [`references/`](references/)：**它们描述别的项目怎么做，不约束 OpenWork。**

| 文档 | 内容 |
|---|---|
| [references/codex-multi-agent.md](references/codex-multi-agent.md) | Codex 的多代理实现：AgentPath 身份、AgentControl 控制平面、mailbox 通信、四层资源限额、角色即配置层、V1/V2 差异 |
| [references/cumora-byoa.md](references/cumora-byoa.md) | Cumora 的 BYOA 实现：daemon 与服务器切分、本地引擎适配、shim、triage 非对称失败、seen/HELD 发言竞争、主动性三层与常量表 |

## 事实来源

| 问题 | 看哪里 |
|---|---|
| 当前实际是什么 | 源码 + `crates/openwork-core/migrations/` |
| 应该是什么、为什么 | 本目录 |
| 怎么跑起来、有哪些命令 | 仓库根 [AGENTS.md](../AGENTS.md) |

文档描述目标状态。**代码与目标有差距时，各篇的"尚未实施"小节会明确列出**——按文档写新代码，不要照抄尚未收敛的现状。

## 阅读顺序

第一次接触这个项目：

1. [architecture.md](architecture.md) —— 建立词汇和边界
2. [session-runtime.md](session-runtime.md) —— 一次请求怎么跑完
3. [context-window.md](context-window.md) —— 模型每次看到什么
4. 按需读 [compaction.md](compaction.md) / [tools.md](tools.md) / [update-plan.md](update-plan.md) / [trace.md](trace.md) / [skills.md](skills.md) / [multi-agent.md](multi-agent.md) / [collaboration.md](collaboration.md) / [collaboration-desktop.md](collaboration-desktop.md) / [collaboration-data-model.md](collaboration-data-model.md)

## 维护原则

- **一个功能一篇文档。** 新增能力时先判断它属于哪一篇，只有当它拥有独立的生命周期、失败语义和验收标准时才新开一篇。
- **每篇自带验收清单。** 没有验收标准的设计描述等于没有约束力。
- **`references/` 不受上面两条约束**，也不描述 OpenWork 的目标状态。调研外部项目的结论放这里，落到 OpenWork 的决定必须写回对应的功能文档才生效。
- **不写迁移叙事。** "以前是什么样"属于 git 历史，不属于文档。
- 领域词汇统一：Session、Turn、Model Call、Tool Call、Permission、Message、Update、Trace、Compaction。
- 修改根目录 README 时同步检查 `README.md` 与 `README.en.md`。
