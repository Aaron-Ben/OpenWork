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
| [permissions.md](permissions.md) | 效果模型、只读判定、`default` / `acceptEdits` 两模式、规则语言、命令解析、审批卡片、权限配置文件 |
| [data-model.md](data-model.md) | 全部表的 DDL 与约束理由、写入顺序、启动修正 |
| [desktop.md](desktop.md) | Tauri Bridge、前端状态三层、Reducer、Trace UI |
| [local-postgres.md](local-postgres.md) | 本地数据库启动、迁移、检查与重建 |

规范类文档在 [`.claude/rules/`](../.claude/rules/)：目前有 [database.md](../.claude/rules/database.md)（时间字段与迁移规范）。

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
4. 按需读 [compaction.md](compaction.md) / [tools.md](tools.md) / [trace.md](trace.md)

## 维护原则

- **一个功能一篇文档。** 新增能力时先判断它属于哪一篇，只有当它拥有独立的生命周期、失败语义和验收标准时才新开一篇。
- **每篇自带验收清单。** 没有验收标准的设计描述等于没有约束力。
- **不写迁移叙事。** "以前是什么样"属于 git 历史，不属于文档。
- 领域词汇统一：Session、Turn、Model Call、Tool Call、Permission、Message、Update、Trace、Compaction。
- 修改根目录 README 时同步检查 `README.md` 与 `README.en.md`。
