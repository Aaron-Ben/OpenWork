# 进度：协作 OpenCode 接入修正

本文配合 [collab-opencode.md](collab-opencode.md) 使用。每完成一步，就更新本文。删除计划时，同时删除本文。

分支：`permission`。

## 1. 工作包状态

| 工作包 | 状态 | 说明 |
|---|---|---|
| C1 协作契约进入 OpenCode | 完成，已提交 `f68aede` | `EngineRuntimeConfig.instructions_file`；OpenCode 派生配置的 `instructions` 引用受管 `AGENTS.md`，classify 用 `classify/` 配置目录 |
| C2 需要用户处理的失败暂停 | 完成，已提交 `f68aede` | `runner.rs` `engine_backoff_after`：`Unauthenticated` 暂停 15 分钟，限流按 retry-after 或 60 秒 |
| C3 OpenCode 进 Seatbelt | 完成，已提交 `757f4c8` | `openwork-sandbox::EngineConfinement` + `Seatbelt::confine`。HomeManager 为每个 Agent 构造围栏。OpenCode 经 `sandbox-exec` 启动，`XDG_DATA_HOME` 改为 `agents/<id>/engines/<engine>/data`，登录信息经 `OPENCODE_AUTH_CONTENT` 传入。沙箱不可用时，inventory 为 error |
| C4–C6 | 未开始 | 排期待定 |
| C7 | 并入 collab-core K1 | 见 [collab-core-progress.md](collab-core-progress.md) |

## 2. 已定决策

| # | 结论 |
|---|---|
| D1 | OpenCode 用 `openwork-sandbox`（Seatbelt）限制文件范围，不限制网络。JWT 仍对模型可见；不做 Cumora 的 MCP 中转（2026-09-24） |
| D2 | Engine 未登录或凭证无效（`EngineError::Unauthenticated`）后，该 Agent 暂停 15 分钟。这个数字取自 Cumora `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`。collaboration.md §6、§11 已同步（2026-09-24） |
| D3 | Engine 进程禁止读取整个 `$HOME` 的文件内容。只放行这些：本 Agent 的目录、派生配置、本 Agent token、shim 与 OpenCode 可执行文件。home 之外照常可读（2026-09-24） |
| D4 | 每个 Agent 有独立的 OpenCode 数据目录 `agents/<id>/engines/opencode/data`。Computer 每次启动 OpenCode 时，从用户的 `opencode/auth.json` 读出登录信息，经 `OPENCODE_AUTH_CONTENT` 传入。沙箱禁止读用户自己的 OpenCode 数据（2026-09-24） |
| D5 | 超限文件的处理：`opencode.rs` 改为目录模块，启动与环境拆到 `opencode/launch.rs`。退避规则移到 `scheduling.rs`。`daemon.rs` 为传递沙箱环境增加 5 行，接受这个增量（2026-09-24） |
| D6 | Run 成功完成时，结算它携带的全部 delivery；沉默记为 `completed`。这与 Cumora daemon 在成功 Turn 后 `ackSeen` 相同。失败、取消、中断时仍不结算。collaboration.md §7/§11/§12、collaboration-data-model.md §6.2/§10 已同步（2026-09-24） |

## 3. 检查记录

- 2026-09-24 C1+C2：`scripts/check.sh` 中 fmt、clippy、desktop typecheck/test 通过。cargo test 只有 `openwork-core` 的 `postgres_core_host_flow::parent_next_turn_reconciles_restart_results_exactly_once` 失败（`turn not found`）。这个测试单独连跑 3 次，都通过。判断这是全量并发下共享数据库的不稳定，与本改动无关；它没有列入 CLAUDE.md 的已知不稳定清单。Redis 的两个显式测试用 `--ignored` 实际跑过，并通过。

- 2026-09-24 C3：`scripts/check.sh` 全部步骤通过。首轮 clippy 报 needless_borrow，修复后单独重跑 clippy，通过。验收：collaboration.md §12 #10 → `computer::home::tests::acc_10_an_agent_engine_cannot_reach_another_agent_or_the_user_home` 与 `openwork-sandbox/tests/engine_confinement.rs` 5 项。反向检查做了两项：去掉 `$HOME` 读拒绝，以及让 adapter 绕过沙箱；两项都使对应测试失败。在同等规则下，真实 OpenCode 1.18.18 的 `--version` 正常，且读不到真实 `auth.json`。这是手动检查，没有发模型请求。
- 整个文件并行运行时，`reported_rate_limit_terminates_a_still_running_opencode_process` 约一半失败：改动前 4/8，改动后 3/8。单线程运行时通过。原因：测试的上限是 5 秒，adapter 报错后固定等待 2 秒再 force-kill，两者之间余量小。这与沙箱无关。

- 2026-09-24 C3 审查后的修正：真实 OpenCode 找不到 session 时只写 stderr，此前 adapter 识别不到。现在 adapter 同时匹配 stderr（`a_stale_session_reported_only_on_stderr_starts_a_fresh_session`；先复现失败，再修）。fake-opencode 中的 `Resume please.` 分支从未使用过，且与真实行为不符，已删除。登录信息的 64 KiB 上限已写入 collaboration.md §3.1。新增大小写变体与符号链接的真机测试，反向检查通过。

- 2026-09-24 真实 OpenCode 冒烟（用户授权并亲自运行）：`runtime_e2e::desktop_server_computer_and_real_opencode_smoke` 通过；模型 `deepseek/deepseek-v4-flash`，用时 9.35 秒。覆盖范围：真实 OpenCode 1.18.18 在 Seatbelt 内启动，真实 `$HOME` 禁读生效；每个 Agent 的数据目录与 `OPENCODE_AUTH_CONTENT` 登录；沙箱内的 shim 经 loopback 发布回复。回复的作者是该 Agent，且只有一条。

- 2026-09-24 C4 实测（本机假模型服务，无外部请求）：OpenCode 1.18.18 遇到 `context_length_exceeded` 时，先输出 `error` 事件，再自动压缩并重试，最终退出码为 1。OpenWork 看到 error 就发 SIGINT。但下一轮续接时，OpenCode 会完成压缩并恢复。只有压缩请求本身也超长时，续接才永远失败（4 轮全部失败）。
- 2026-09-24 C7 实测（真实 deepseek，用户同意）：按角色点名 "Reviewer, ..." 两次，两次都只有 Bo 回复，且回答正确。第一次 Ada 额外调了 `openwork participants`、`openwork members`。第三次 Ada/Cy 只调 `openwork glance`；Bo 调了 `openwork --help` 后才会用 reply。按名字点名没有测到：下面第 1 个问题阻断了它。
- 2026-09-24 测量中发现的计划外问题：
  1. 保持沉默的 Agent 从不结算。只有 action/ack/triage_false 才结算 delivery（`server/runs.rs`），而协作契约从未提到 `openwork ack`。用户消息跳过 triage，所以每 20 秒跑一轮完整 Turn，永不停止（第三次测量 5 分钟 34 个 Run）。Cumora 由 daemon 在成功 Turn 后自行 `ackSeen`。
  2. OpenCode 以继承的 `PWD` 作为项目目录（opencode `cli/cmd/run.ts:333`），不是 Agent 的 `work/`。
  3. 每次 RuntimeSession 的 `XDG_CACHE_HOME` 都是空目录。OpenCode 于是回落到自带快照，模型目录与 models.dev 不一致。目录刷新后，OpenCode 在本地把 Desktop 默认模型 `deepseek/deepseek-v4-flash` 判为 deprecated，并拒绝它；DeepSeek API 仍可调用这个模型。→ collab-core E15 解决了模型失效的部分：派生配置标为 active，默认模型改为 `deepseek/deepseek-flash`。缓存目录本身仍在每次会话时清空。
  4. `Model not found` 不触发暂停，每 20 秒重试。

- 2026-09-24 D6 实现（已提交 `d9aa9dd`）：迁移 `202609240001_settle_completed_runs.sql` 扩展了 `eligible_reason`。Run 为 completed 时，`server/runs.rs` 把未 eligible 的 delivery 记为 `completed`，然后统一结算。复现测试 `messaging::a_completed_silent_run_settles_its_delivery_so_the_agent_is_not_woken_again` 先失败（`last_read_seq` 为 0），修复后通过。`scripts/check.sh` 除已知不稳定测试外全部通过；该不稳定测试单独重跑通过。
- 2026-09-24 C7 第四次实测（真实 `deepseek/deepseek-flash`，预置新模型目录）：80 秒内安静，共 10 个 Run，全部 completed。按角色点名 → 只有 Bo 回复，且回答正确。按名字点名 "Ada, ..." → 只有 Ada 回复，且回答正确。可见日志中没有再出现 `participants` / `members` 查询。Bo 仍调了 `openwork --help`。

## 4. 待定

- C3–C7 相对 sandbox-and-tools WP4 的先后顺序。
- 上面 4 个新问题的修法与优先级。其中第 1 个问题要改 collaboration.md §7 的结算语义。
