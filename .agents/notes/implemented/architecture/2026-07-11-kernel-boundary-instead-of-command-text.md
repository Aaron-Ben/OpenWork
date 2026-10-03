# Agent Note: 边界由内核强制，不由命令文本推断

Status: implemented

## 问题

模型每一轮会调用很多次工具。每次都问，用户会疲劳，最后不看内容就点同意。不问，就要有别的机制守住边界。

“看命令文本，证明它只读就不问”有三个结构性问题，改进实现也补不上：

- 文本看不到 `$VAR` 展开、子进程，也看不到 `build.rs` 与测试代码写了什么；
- “只读”本身靠不住。仓库配置能让 `git status` 执行 fsmonitor 与外部 diff 等辅助程序；
- `cargo test`、`npm run build` 这类最常见的操作都证明不了。于是打断集中在最常见的调用上。

## 决策

- bash 以 `/bin/bash -c` 在 Seatbelt 内执行（`crates/openwork-tools/src/builtins/process/bash.rs` 的 `SHELL` 与 `SandboxBackend::wrap`）。沙箱内的命令一律直接执行，内核拒绝越界的文件操作。
- 执行前的判定只看三件事：写目标是否硬保护、模型是否请求越界、命令是否命中危险命令清单（`crates/openwork-core/src/session/approval.rs` 的 `gate`）。它不推断命令会读写什么。
- 被沙箱拒绝是结果事实，不是权限判定。`classify`（`crates/openwork-sandbox/src/denial.rs`）返回 `RunOutcome::Denied`，结果末尾追加拒绝标记。
- 危险命令卡片批准后，命令仍在当前模式的沙箱内执行，批准不放宽沙箱。
- 只有模型显式带 `sandboxPermissions` 时才出越界卡片。系统不替模型猜。
- `tree-sitter-bash` 只用于危险命令检测（`crates/openwork-tools/src/permission/danger.rs`）。检测看不懂的命令不问，直接在沙箱内执行，见 [危险命令清单是封闭的](2026-09-24-closed-danger-command-list.md)。
- 提交 `4d65def` 删除了按文本放行的整套机制：`readonly/` 判定表与标志 arity 表、`sed.rs`、`bash/filesystem.rs` 闸门、`eligibility.rs`、`grant.rs` 的 arity 归约与会话授权、`engine.rs` / `rule.rs` / `effect.rs`、`card.rs`，以及 `tests/permissions_p1–p4.rs`。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**按命令文本证明只读，证明不了就问。** 这是 2026-08-01 到 2026-09-24 的实现。当时的设计不做 OS 沙箱，理由是 Seatbelt 的规则优先级没有文档化的契约，失效又是静默的，必须靠真机测试守住。没有保留：上面三个问题改进实现也补不上。Codex 走过同一条路。提交 `3b45c29062` 不再把 git 命令当作天然安全，提交 `942af8447b` 删除了整份已知安全命令白名单（`codex-rs/shell-command/src/command_safety/is_safe_command.rs`）。maka 的同一结论见 `packages/core/src/permission.ts` 中 “Shell command categorization” 一节的注释。静默失效的风险改由启动自检与真机测试承担。

**沙箱不可用时退回命令文本判定。** 没有采用：那样就要维护两套判定。降级路径还会在沙箱故障时悄悄换成更弱的边界。切换时，删除与新增在同一批里完成。

## 后果

- 读代码、改代码、编译、测试、用 git 查看状态与历史，全部不被打断。只有越界与危险命令出卡片。
- 边界的正确性落在内核与 profile 生成上，不再落在一张判定表上。判定表写错一条，就是一次静默的自动执行；这类错误不再存在。
- 代价：沙箱拦不住边界内的破坏。`auto` 下 `rm -rf src` 会成功，危险命令检测只尽力覆盖这一块。
- `.git` 在沙箱内只读，已提交的历史受内核保护。暴露的只有未提交的改动与未跟踪的文件。
- 代价：bash 只能在有可用沙箱的平台上运行。Linux 现在停用 bash，见 [Linux 沙箱后端](../../proposed/architecture/2026-09-24-linux-sandbox-backend.md)。
- Seatbelt 的行为只能在真实内核上确认。`crates/openwork-sandbox/tests/matrix.rs` 与 `parity.rs` 在每次 `cargo test` 时运行。
