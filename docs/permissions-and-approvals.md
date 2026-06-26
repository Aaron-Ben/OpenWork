# 权限模型与 Human-in-the-loop 审批

Last reviewed: 2026-06-17

## 1. 目标

权限模型用于回答“工具可以访问什么资源”。审批模型用于回答“某次工具调用是否需要人确认”。两者相关，但不是同一件事：

- 权限模型：执行时的资源边界。
- 审批模型：执行前的人类授权流程。

当前项目已经有路径权限数据模型和审批桥，但还没有操作系统级 sandbox。

## 2. 权限数据模型

代码位置：

```text
crates/openwork-tools/src/permissions.rs
crates/openwork-tools/src/tool.rs
crates/openwork-runtime/src/router.rs
```

核心类型：

```rust
pub enum FileSystemMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

pub enum NetworkMode {
    Restricted,
    Enabled,
}

pub struct PermissionProfile {
    pub filesystem: FileSystemPermissions,
    pub network: NetworkMode,
}
```

默认 profile：

```rust
PermissionProfile::workspace_write(working_dir)
```

默认语义：

- 允许读取 `working_dir`
- 允许写入 `working_dir`
- 允许写入系统临时目录
- 禁止写 `.git`、`.agents`、`.codex`
- 网络模式为 `Restricted`

## 3. 路径检查方式

工具通过：

```rust
ctx.check_path(path, AccessKind::Read)
ctx.check_path(path, AccessKind::Write)
```

进行检查。当前检查使用 lexical normalize，不依赖 `canonicalize`。这避免了目标文件不存在时无法检查的问题，但也意味着它不是完整的 symlink 安全边界。

当前应把它视为“应用层权限检查”，不是 sandbox。

## 4. 各工具权限

| 工具 | 当前检查 |
| --- | --- |
| `read` | 目标文件必须允许 `Read` |
| `write` | 目标文件必须允许 `Write` |
| `edit` | 目标文件必须允许 `Write` |
| `list` | 目标目录必须允许 `Read` |
| `grep` | 搜索根目录必须允许 `Read` |
| `glob` | 搜索根目录必须允许 `Read` |
| `bash` | 检查 `working_dir` 可读；命令内部文件访问不被精细约束 |

`bash` 如果在 `NetworkMode::Restricted` 下执行，会注入：

```text
OPENWORK_NETWORK_RESTRICTED=1
```

这只是环境信号，不是网络隔离。

## 5. 审批模型

代码位置：

```text
crates/openwork-tools/src/approval.rs
crates/openwork-runtime/src/router.rs
apps/desktop/src/components/chat/ApprovalDialog.tsx
apps/desktop/src-tauri/src/lib.rs
```

核心类型：

```rust
pub enum ApprovalPolicy {
    Untrusted,
    OnFailure,
    OnRequest,
    Granular,
    Never,
}

pub enum ApprovalsReviewer {
    User,
    AutoReview,
}
```

当前真实语义：

| 策略 | 当前行为 |
| --- | --- |
| `Untrusted` | 每次工具调用都请求审批 |
| `Never` | 不请求审批，直接执行 |
| `OnFailure` | 无 sandbox，保守降级为请求审批 |
| `OnRequest` | 无 sandbox，保守降级为请求审批 |
| `Granular` | 无 sandbox，保守降级为请求审批 |

`ApprovalsReviewer::AutoReview` 当前仅占位。runtime 会保守拒绝，不会静默放行。

## 6. 审批流程

```text
模型返回 tool_call
  -> runtime 判断 ApprovalPolicy
  -> 发出 AgentEvent::ApprovalRequest
  -> Tauri 映射为 approval_request
  -> 前端 ApprovalDialog 展示工具名和参数
  -> 用户允许/拒绝
  -> resolve_approval
  -> ApprovalBridge::resolve
  -> runtime 继续执行或返回拒绝错误
```

这就是当前的 human-in-the-loop。

## 7. 重要边界

用户同意 `bash` 代表“允许这次 bash 工具调用”，不代表命令已经被 sandbox 限制在 workspace 内。

对于结构化工具，例如：

```text
write(path="/repo/a.txt")
```

路径是显式字段，权限模型能检查。

对于 bash：

```text
bash(command="cat /some/path && echo hi > /other/path")
```

命令是自由文本，当前没有完整 shell 解析和系统隔离，因此不能达到同等强度。

## 8. 建议后续改进

1. 新增 `tool_runs`，记录工具调用、审批、执行结果、耗时。
2. 给 `bash` 增加独立策略，例如 `allow_bash`、危险命令二次确认、输出大小限制。
3. 将 `OnRequest` / `OnFailure` 接到真实 sandbox executor。
4. 对写文件工具展示 diff 审批，而不是只展示 path。
5. 把审批事件持久化到 trace，便于复盘。
