# 进度：协作 OpenCode 接入修正

配合 [collab-opencode.md](collab-opencode.md) 使用。每完成一步就更新；计划删除时一并删除。

分支：`permission`。

## 1. 工作包状态

| 工作包 | 状态 | 说明 |
|---|---|---|
| C1 协作契约进入 OpenCode | 完成，已提交 `f68aede` | `EngineRuntimeConfig.instructions_file`；OpenCode 派生配置 `instructions` 引用受管 `AGENTS.md`，classify 用 `classify/` 配置目录 |
| C2 需要用户处理的失败暂停 | 完成，已提交 `f68aede` | `runner.rs` `engine_backoff_after`：`Unauthenticated` 暂停 15 分钟，限流按 retry-after 或 60 秒 |
| C3 OpenCode 进 Seatbelt | 完成（未提交） | `openwork-sandbox::EngineConfinement` + `Seatbelt::confine`；HomeManager 为每个 Agent 构造围栏；OpenCode 经 `sandbox-exec` 启动，`XDG_DATA_HOME` 改为 `agents/<id>/engines/<engine>/data`，登录信息经 `OPENCODE_AUTH_CONTENT` 传入；沙箱不可用时 inventory 为 error |
| C4–C7 | 未开始 | 排期待定 |

## 2. 已定决策

| # | 结论 |
|---|---|
| D1 | OpenCode 用 `openwork-sandbox`（Seatbelt）限制文件范围，网络放开；JWT 仍对模型可见，不做 Cumora 的 MCP 中转（2026-09-24） |
| D2 | Engine 未登录或凭证无效（`EngineError::Unauthenticated`）后该 Agent 暂停 15 分钟，数字取自 Cumora `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`；collaboration.md §6、§11 已同步（2026-09-24） |
| D3 | Engine 进程禁止读取整个 `$HOME` 的文件内容，只放行本 Agent 的目录、派生配置、本 Agent token、shim 与 OpenCode 可执行文件；home 之外照常可读（2026-09-24） |
| D4 | 每个 Agent 独立的 OpenCode 数据目录 `agents/<id>/engines/opencode/data`；登录信息由 Computer 每次启动 OpenCode 时从用户的 `opencode/auth.json` 读出，经 `OPENCODE_AUTH_CONTENT` 传入；沙箱禁止读用户自己的 OpenCode 数据（2026-09-24） |
| D5 | 超限文件：`opencode.rs` 改为目录模块，启动与环境拆到 `opencode/launch.rs`；退避规则移到 `scheduling.rs`；`daemon.rs` 为传递沙箱环境增加 5 行，接受（2026-09-24） |

## 3. 检查记录

- 2026-09-24 C1+C2：`scripts/check.sh` 中 fmt、clippy、desktop typecheck/test 通过；cargo test 仅 `openwork-core` 的 `postgres_core_host_flow::parent_next_turn_reconciles_restart_results_exactly_once` 失败（`turn not found`），单独连跑 3 次均通过，判断为全量并发下共享数据库的不稳定，与本改动无关，未列入 CLAUDE.md 已知不稳定清单。Redis 两个显式测试用 `--ignored` 实际跑过并通过。

- 2026-09-24 C3：`scripts/check.sh` 全部步骤通过（首轮 clippy 报 needless_borrow，已修并单独重跑 clippy 通过）。验收：collaboration.md §12 #10 → `computer::home::tests::acc_10_an_agent_engine_cannot_reach_another_agent_or_the_user_home` 与 `openwork-sandbox/tests/engine_confinement.rs` 5 项；反向检查（去掉 `$HOME` 读拒绝 / adapter 绕过沙箱）均使对应测试失败。真实 OpenCode 1.18.18 在同等规则下 `--version` 正常、读不到真实 `auth.json`（手动，未发模型请求）。
- `reported_rate_limit_terminates_a_still_running_opencode_process` 在整文件并行运行时约一半失败：改动前 4/8、改动后 3/8，单线程运行通过；原因是测试 5 秒上限与 adapter 报错后固定等待 2 秒 force-kill 之间余量小，与沙箱无关。

- 2026-09-24 C3 审查后修正：真实 OpenCode 找不到 session 时只写 stderr，adapter 此前识别不到，改为同时匹配 stderr（`a_stale_session_reported_only_on_stderr_starts_a_fresh_session`，先复现失败再修）；删除 fake-opencode 中从未被使用、且与真实行为不符的 `Resume please.` 分支；64 KiB 登录信息上限写入 collaboration.md §3.1；新增大小写变体与符号链接真机测试（反向检查通过）。

- 2026-09-24 真实 OpenCode 冒烟（用户授权并亲自运行）：`runtime_e2e::desktop_server_computer_and_real_opencode_smoke` 通过，模型 `deepseek/deepseek-v4-flash`，9.35 秒。覆盖：真实 OpenCode 1.18.18 在 Seatbelt 内启动（真实 `$HOME` 禁读生效）、每 Agent 数据目录 + `OPENCODE_AUTH_CONTENT` 登录、沙箱内 shim 经 loopback 发布回复，回复作者为该 Agent 且只有一条。

## 4. 待定

- C3–C7 相对 sandbox-and-tools WP4 的先后顺序。
