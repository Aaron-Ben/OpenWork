# 进度：协作 OpenCode 接入修正

配合 [collab-opencode.md](collab-opencode.md) 使用。每完成一步就更新；计划删除时一并删除。

分支：`permission`。

## 1. 工作包状态

| 工作包 | 状态 | 说明 |
|---|---|---|
| C1 协作契约进入 OpenCode | 完成（未提交） | `EngineRuntimeConfig.instructions_file`；OpenCode 派生配置 `instructions` 引用受管 `AGENTS.md`，classify 用 `classify/` 配置目录 |
| C2 需要用户处理的失败暂停 | 完成（未提交） | `runner.rs` `engine_backoff_after`：`Unauthenticated` 暂停 15 分钟，限流按 retry-after 或 60 秒 |
| C3 OpenCode 进 Seatbelt | 未开始 | |
| C4–C7 | 未开始 | 排期待定 |

## 2. 已定决策

| # | 结论 |
|---|---|
| D1 | OpenCode 用 `openwork-sandbox`（Seatbelt）限制文件范围，网络放开；JWT 仍对模型可见，不做 Cumora 的 MCP 中转（2026-09-24） |
| D2 | Engine 未登录或凭证无效（`EngineError::Unauthenticated`）后该 Agent 暂停 15 分钟，数字取自 Cumora `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS`；collaboration.md §6、§11 已同步（2026-09-24） |

## 3. 检查记录

- 2026-09-24 C1+C2：`scripts/check.sh` 中 fmt、clippy、desktop typecheck/test 通过；cargo test 仅 `openwork-core` 的 `postgres_core_host_flow::parent_next_turn_reconciles_restart_results_exactly_once` 失败（`turn not found`），单独连跑 3 次均通过，判断为全量并发下共享数据库的不稳定，与本改动无关，未列入 CLAUDE.md 已知不稳定清单。Redis 两个显式测试用 `--ignored` 实际跑过并通过。

## 4. 待定

- C3–C7 相对 sandbox-and-tools WP4 的先后顺序。
