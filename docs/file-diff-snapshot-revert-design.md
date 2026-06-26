# 文件 Diff / Revert 决策

Last reviewed: 2026-06-26

## 当前决策

OpenWork 暂不持久化工作区快照，不创建 `worktree_snapshots` 或
`worktree_snapshot_files` 表。

原因：

- Git 已经提供成熟的变更查看与回滚能力。
- 数据库保存 before/after 文件内容会快速膨胀。
- 文件内容可能包含源码、密钥或隐私数据，持久化会增加安全和清理成本。
- 第一版更需要稳定的聊天、工具调用、trace 与审批链路。

## 推荐交互

- 查看变更：`git diff`
- 回滚文件：`git restore <path>`
- 交互式回滚部分 patch：`git restore -p`
- 临时保存变更：`git stash`

## 后续可选能力

如果产品明确需要“回滚到某条用户消息之前”，再设计轻量 file history/checkpoint。
该能力应独立评估，不应默认把所有 request 的 before/after 内容写入主数据库。
