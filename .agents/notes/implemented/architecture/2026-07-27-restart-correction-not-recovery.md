# Agent Note: 进程重启只修正状态，不恢复执行

Status: implemented

## 问题

进程可能在 Turn 的任何一步退出：模型正在流式输出，工具正在执行，或用户正在看审批卡片。重启后，数据库里留下 `running` 的 Turn、`running` 的 Trace Span，以及已经写库、但没有结果的 Tool Call。

工具可能已经产生了副作用，也可能还没有开始。数据库无法区分这两种情况。下一次请求又必须满足 Provider 的要求：每个 Tool Call 都要有结果。

## 决策

- 启动时，`PostgresStorage::mark_running_interrupted`（`crates/openwork-core/src/storage/postgres/session.rs`）在一个事务里把 `running` 的 Turn 改为 `interrupted`，把 `running` 的 Span 改为 `outcome_unknown`。
- 系统不重建活动 Turn，不恢复审批等待，不重发 Provider 请求，不执行缺少结果的 Tool Call。
- 缺少结果的 Tool Call 在数据库里保持原样。`normalize_for_request`（`crates/openwork-core/src/context/normalize.rs`）在每份请求副本里补一条 `interrupted` 结果，正文是固定常量。
- 运行时 Phase、草稿与等待中的审批只在 Actor 内存里。Live Update 不写库，没有 `session_updates` 表。

事实见 [session-runtime.md §8、§10](../../../../docs/subsystems/session-runtime.md) 与 [context-window.md §4.2](../../../../docs/subsystems/context-window.md)。

## 考虑过的方案

**重启后从数据库继续 Agent Loop。** 没有采用。`running` 状态说明不了工具做到哪一步。自动继续可能重复一次已经发生的写入或命令。

**启动时向 `messages` 追加一条 `outcome_unknown` 结果。** 原设计文档写的是这个做法。当前代码改为只在请求副本里补结果，数据库里的原始消息不因合法化而改变。补出的正文是常量，同一段历史每次得到相同的字节，前缀缓存不会失效。

**持久化运行时状态与等待中的审批。** 没有采用。它们只能服务一个已经不存在的进程。重连靠同进程的 Snapshot 与 Update ring buffer。

## 后果

- 用户看到 `interrupted` 的 Turn，需要自己决定是否重做。系统不会替用户重试有副作用的操作。
- 修正与部分唯一索引 `uq_turns_one_running_per_session` 配套。不修正时，遗留的 `running` 行会让 Session 无法开始新 Turn。
- 补出的结果不在数据库里，界面的历史中看不到它，只有请求副本与上下文检查里有。
- Update 丢失不会改变 Turn 状态，但进程退出后，界面只能从数据库读取终态。
