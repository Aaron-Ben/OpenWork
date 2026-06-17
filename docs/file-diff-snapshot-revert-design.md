# 文件 Diff / Snapshot / Revert 设计

Last reviewed: 2026-06-17

## 1. 背景

Anvil 已经具备 agent loop、工具调用、审批和会话持久化。文件类工具和 `bash` 都可能改变工作区，因此需要让用户在 app 重启后仍能看到一次请求修改了哪些文件，并在安全条件满足时回滚这些修改。

## 2. Codex 与 opencode 的取舍

Codex 更偏 runtime / protocol：

- 以 thread / turn 为边界。
- diff 作为 turn 事件向上层发送。
- rollback 更像线程状态回滚。
- 优点是事件边界清晰、适合 trace、回放和多端协议。

opencode 更偏 product / session：

- 请求前后记录 snapshot。
- session/message/part 保存 snapshot 和 summary diff。
- UI 可以展示本轮变更并支持 revert。
- 优点是贴近用户心智，适合 review 和产品化 workspace。

Anvil 当前采用混合路线：

- 第一版使用 session/request 级 snapshot，优先保证 UI 可 review、可恢复。
- 数据模型保留 `request_id`，后续可关联 `tool_run_id`、turn diff event 和 trace。

## 3. 第一版目标

- chat request 开始前捕获 git 工作区状态。
- chat request 结束、取消、doom-loop 或错误后再次捕获状态。
- 保存本次请求造成的净文件变化。
- app 重启后仍能列出变更文件并查看 before/after diff。
- 支持安全 revert：只有当前文件内容仍等于 snapshot 的 after 内容时才允许回滚。

## 4. 非目标

- 不实现完整 OS sandbox。
- 不实现 Codex 风格 thread rollback。
- 不把 diff 注入模型上下文。
- 不解析 bash 命令来推断文件影响。
- 不处理跨请求并发写入的自动合并。

## 5. 数据模型

### `worktree_snapshots`

保存一次 request 的工作区变更摘要：

```text
id
session_id
request_id
working_dir
before_status_json
after_status_json
changed_files_json
created_at
completed_at
reverted_at
```

### `worktree_snapshot_files`

保存文件级 before/after 内容：

```text
id
snapshot_id
path
before_content
after_content
```

当前使用内容快照而不是只保存 unified diff，是为了支持安全 revert。

## 6. 捕获策略

当前只在 git repository 内工作：

```text
request start
  -> git status --porcelain -z
  -> 记录 dirty/untracked 文件当前内容

request finish
  -> 再次 git status --porcelain -z
  -> 合并 before/after 涉及的路径
  -> 对 clean-before/dirty-after 文件，从 HEAD 读取 before 内容
  -> 保存 before_content / after_content 不同的文件
```

如果工作目录不是 git repo，或者 git 命令失败，则跳过 snapshot，不影响聊天请求。

## 7. Revert 安全策略

revert 时逐文件校验：

```text
current_content == after_content
  -> 写回 before_content
current_content != after_content
  -> 拒绝 revert
```

如果 `before_content` 为 `null`，表示文件在 snapshot 前不存在，revert 时删除该文件。

这避免覆盖用户在 snapshot 之后手动修改的文件。

## 8. UI 策略

前端参考 cc-haha 的 `CurrentTurnChangeCard` / `DiffViewer`：

- 会话底部展示 request 级变更卡片。
- 卡片展示变更文件列表。
- 展开后按文件查看 before/after diff。
- 提供 `Revert` 按钮。
- revert 后标记为 `Reverted`。

Anvil 第一版使用轻量 unified diff 文本展示，不引入额外 diff viewer 依赖。

## 9. 后续演进

1. 将 snapshot 与 `tool_runs` 关联，支持 tool 级 diff。
2. 存储统一 diff 和增删行统计，避免前端重复计算。
3. 增加大文件和二进制文件策略。
4. 增加冲突详情 UI。
5. 增加 turn diff event，让 trace 能直接消费文件变化。
