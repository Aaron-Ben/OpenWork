# Agent Note: bash 后台任务

Status: proposed

## 问题

bash 只支持前台、单次调用，超时最长 120 秒（[tools.md §7.9](../../../../docs/subsystems/tools.md)）。模型不能启动 dev server，也不能在长任务运行时继续做别的事。

## 提议

- `bash` 增加 `run_in_background`，立即返回任务 ID。
- `job_output` 读取任务输出，可以只读上次之后的新增部分。
- `job_kill` 终止任务。
- 后台进程在沙箱内运行。越界请求、危险命令检测、沙箱不可用时停用 bash，都与前台共用同一套规则（[permissions.md](../../../../docs/subsystems/permissions.md) §6、§9、§10）。
- 不含 PTY 与交互式输入（`write_stdin`）。

实现前要先定这些细节：

- 每个任务保留多少输出，落盘怎样处理；
- 同时运行的任务数上限；
- Session 结束或进程退出时，是否终止任务；
- `job_output` 是否只返回增量。

参照：DSH 的 bash 有 `run_in_background`，前台命令超时后可以转为后台任务（`packages/shell/tool-bash/src/index.ts` 的 `promoteOnTimeout`）。任务工具 `job_output`、`job_list`、`job_kill` 在 `packages/jobs/tool-jobs/src/index.ts`。Codex 的工具规划中有 `write_stdin`（`codex-rs/core/src/tools/spec_plan.rs`）。

## 考虑过的方案

**只加 `run_in_background`，不加查询与终止工具。** 没有采用：会留下无法管理的进程，权限也无法闭环。

**在沙箱之前做后台任务。** 没有采用：后台进程必须在沙箱里运行，它的越界与停用规则要与前台一致。先有沙箱，才能一致。沙箱现在已经实现（[permissions.md](../../../../docs/subsystems/permissions.md)）。

## 验收条件

- 后台任务在本次调用的 Seatbelt profile 下运行；沙箱不可用时，`run_in_background` 与前台一样返回 `sandbox_unavailable`。
- `job_output` 返回有界的输出，并报告任务状态与退出码；输出超出上限时落盘。
- `job_kill` 终止整个进程组。
- 取消 Turn 不影响已转入后台的任务；按上文定下的规则，Session 结束时处理剩余任务。
- 测试覆盖：启动、读取增量、终止、任务数上限。

## 风险

- 后台进程比 Turn 活得久。终止失败会留下孤儿进程。
- 后台任务的危险命令卡片与越界卡片在启动时出现。之后任务做什么，用户不再被询问。
- dev server 常常需要监听端口。沙箱不管控网络（[permissions.md §5](../../../../docs/subsystems/permissions.md)），这一点不变。
