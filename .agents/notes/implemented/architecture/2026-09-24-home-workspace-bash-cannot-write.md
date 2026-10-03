# Agent Note: 工作区是主目录或其上级时，bash 不能写工作区

Status: implemented

## 问题

`auto` 模式下，bash 可以写整个工作区。工作区通常是项目目录，所以影响有限。但用户可以把 `$HOME` 或 `/Users` 选为工作区。这时工作区里有许多位置，其中的内容会在沙箱外执行：

- `~/Library/LaunchAgents` 在下次登录时启动；
- `PATH` 里的 `~/.local/bin` 能冒充常用命令；
- 许多工具的配置文件能指定启动命令。

于是沙箱内的一次写入，就能变成沙箱外的持久执行。硬保护档要防的正是这一类写入。

## 决策

- `SandboxPolicy::workspace_contains_home`（`crates/openwork-sandbox/src/policy.rs`）判断规范化后的 `$HOME` 是否位于工作区之下。成立时，bash 的可写根不含工作区。
- `SandboxPolicy::bash_writes_workspace` 只在 `auto` 且工作区不包含主目录时为真。因此两个模式下，bash 写这种工作区都要走越界。
- 文件工具不受影响。围栏仍按会话模式允许它们写工作区。
- `runtime/sandbox-policy` 的 bash 一行按 `bash_writes_workspace` 渲染。模型在 `auto` 下也会读到“写工作区需要 sandboxPermissions”。
- 测试：`policy::a_workspace_containing_home_is_read_only_for_bash_in_every_mode`（单元测试）；`matrix::a_home_workspace_is_read_only_for_bash`（真实内核，含 `~/Library/LaunchAgents`）。

设计见 [permissions.md §2、§15 #49](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**把自启动路径逐个加入硬保护。** 例如把 `~/Library/LaunchAgents`、`~/.local/bin` 加进硬保护清单，工作区仍对 bash 可写。没有采用：这类位置列举不全。漏掉一个，就留下一条沙箱外执行的路径。

## 后果

- 边界不依赖一份不完整的清单。
- 以主目录为工作区时，`auto` 下 bash 的每次写入都要批准，打断变多。把工作区设为具体的项目目录，就没有这个代价。
- 在这种工作区里，文件工具写 `~/Library/LaunchAgents` 不需要越界。设计接受这一点，原始记录没有写理由。
- 判断用规范化路径。指向主目录的符号链接工作区同样命中。
- 这是对等测试里两侧有意不同的第二种情况。第一种是 `accept-edits` 下的工作区（[permissions.md §4](../../../../docs/subsystems/permissions.md)）。
