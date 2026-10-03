# Agent Note: 文件工具的边界是进程内围栏

Status: implemented

## 问题

bash 由 Seatbelt 约束。`read`、`write`、`edit`、`grep`、`glob`、`list` 在 OpenWork 进程内运行，不经过 `sandbox-exec`。它们也必须遵守同样的四档路径。

如果文件工具与 bash 各持一份路径规则，两边迟早不一致。一边能写的路径，另一边可能不能写；或者反过来。

## 决策

- 文件工具经 `ToolSessionContext::resolve_path`（`crates/openwork-tools/src/checked_path.rs`）取得 `CheckedPath`。它先把路径规范化成真实路径，新文件取最近的已存在父目录。然后用本次调用的 `SandboxPolicy::check` 以 `Actor::FileTool` 判断。
- `CheckedPath` 的字段私有，工具拿不到未经检查的路径。
- 围栏不持有路径规则。Seatbelt profile（`SeatbeltProfile::new`）与 `check` 由 `SandboxPolicy` 与 `tiers.rs` 的同一组函数推导。
- `crates/openwork-sandbox/tests/parity.rs` 的 `acc_10_file_tool_fence_and_seatbelt_agree_on_every_path` 在真实内核上比较两侧的可读、可写结论。`bash_and_file_tools_differ_only_on_the_workspace_under_accept_edits` 把唯一有意的差异写成显式期望。
- 主目录工作区是第二种有意差异，见 [工作区是主目录或其上级时，bash 不能写工作区](2026-09-24-home-workspace-bash-cannot-write.md)。对等测试不覆盖它，`matrix.rs` 的 `a_home_workspace_is_read_only_for_bash` 覆盖它。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**文件工具放进沙箱下的独立 worker 进程。** maka 的文件工具经 `packages/runtime/src/filesystem-worker/` 的 worker 执行。worker 在沙箱后端下启动，包括 `macos-seatbelt`，由内核强制。没有采用：威胁面是模型选定的路径参数，工具代码本身可信。“先规范化再判包含”覆盖得了这个威胁面。独立进程还要维护 worker 的生命周期与进程间协议。如果文件工具以后开始执行不可信的逻辑，再改为 maka 的做法。

**文件工具与 bash 各自持有路径规则。** 2026-08-01 的设计里，围栏只允许读写工作区与 skill 根，bash 另有一套按文本推断的规则。没有保留：两份规则会漂移。DSH 让 Seatbelt profile 与进程内围栏共用 `writableRoots`（`packages/sandbox/sandbox/src/roots.ts`，围栏在 `packages/fs/fs-sandbox/`）。

## 后果

- 路径规则只有一份。改档位清单时，两侧一起变化，对等测试防止漂移。
- 代价：文件工具的边界比 bash 弱。它依赖进程内代码正确，而不是内核。
- 文件工具不依赖沙箱可用。沙箱不可用时，它们照常工作。
- 读取不限于工作区。除凭据目录外处处可读，与 bash 在沙箱里能 `cat` 的范围一致。
- 规范化与写入之间有一个时间窗口，路径中的符号链接可能在这时被替换。原设计文档承认这个窗口：write 与 edit 在创建父目录后再解析检查一次，只能收窄它，不能消除它。它能否被利用，没有验证（未确认）。
