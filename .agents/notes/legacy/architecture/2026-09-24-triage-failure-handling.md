# Agent Note: triage 模型失败时，限流退避，其他错误 fail closed

Status: legacy

## 问题

triage 模型判断一批 Agent 消息是否值得跑一轮主模型。这个模型也会失败：限流、超时、输出无法解析、Engine 出错。直接的两种处理都有问题。

fail open 会唤醒主模型。失败的原因是限流时，主模型会在同一份额度上继续失败。保留 delivery 并重试，对一个持续输出无法解析内容的模型，又会在每次 poll 时重复失败。

## 决策

`handle_failure`（`crates/openwork-collab/src/computer/runner/classify.rs`）按失败类型处理：

- `RateLimited` 与 `Timeout`：返回 `Classified::Retry`。Run 记为 `failed`，错误码为 `TRIAGE_RATE_LIMITED` 或 `TRIAGE_ERROR`，delivery 保留。`AgentRunner::note_triage_failure`（`runner/mod.rs`）设置退避：从 30 秒开始，每次翻倍。
- `Cancelled`：Run 记为 `interrupted`，不退避。
- 输出无法解析与其他 Engine 错误：fail closed。结论为 `actionable = false`、`source = fail_closed`，原因保留错误的前 120 字（`FAILURE_DETAIL_MAX_CHARS`）。Server 把 delivery 结算为 `triage_false`，不退避。

成功的 triage 清零退避。迁移 `202609240005_monologue_fail_closed.sql` 给 `collab_triages.source` 加入 `fail_closed`。`InboxTriage::record`（`server/triage.rs`）拒绝 actionable 的 `fail_closed`。

规则见 [collaboration.md §8.3](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**有人类消息时 fail open。** Cumora daemon 的本地 triage 在批次含人类消息时返回 `fail-open`，然后同样退避，不唤醒主模型（`server/src/agents/computer/daemon.ts`）。OpenWork 不需要这一支。人类消息在第 2 步确定性参与，或交给路由题，不会走到 triage 模型这一步。走到这一步的批次只含 Agent 消息，正好是 Cumora `failClosed` 为真的条件（`server/src/agents/triage-core.ts`）。

**限流时 fail open。** 设计拒绝它，因为主模型会在同一份额度上继续失败。Cumora 的注释把这称为耗光账户额度的失控路径（`daemon.ts` 本地 triage 的限流分支）。

## 后果

- 限流不会升级为主模型调用，消息也不会丢失。
- 持续坏掉的输出不会在每次 poll 时重复失败。代价是漏回一条 Agent 消息。下一条真实消息会再唤醒 Agent。
- 退避期间，`drive_once` 不读 inbox。所以该 Agent 的全部工作都推迟，包括人类消息。
- `note_triage_failure` 把指数限制在 4，退避最长 480 秒，代码中 10 分钟的上限达不到。Cumora 的同一公式没有指数上限，第 6 次起为 10 分钟。
- triage 中的 `Unauthenticated` 走 fail closed，不触发 15 分钟暂停。正式 Turn 的暂停见 [Engine 失败暂停](../feature/2026-09-24-engine-failure-backoff.md)。
- 路由题失败另按参与处理，见 [点名路由](2026-09-24-per-agent-triage-for-unaddressed-messages.md)。
- 验收：`computer::runner::classify::tests::acc_22_triage_failures_back_off_or_fail_closed_like_cumora`、`posting::acc_22_a_failed_triage_model_fails_closed_for_agent_only_messages`。
