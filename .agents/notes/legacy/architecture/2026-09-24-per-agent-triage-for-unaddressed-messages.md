# Agent Note: 点名别人的消息，由每个没被点名的 Agent 用自己的 triage 模型判断

Status: legacy

## 问题

人类在群里点名一个 Agent 时，房间里其他 Agent 也会醒来。每个都跑一次主模型很贵，而且它们可能都去回答。Cumora 先判断这条消息是给被点名的人，还是给整个房间（`server/src/agents/routing.ts`），这一步要调用模型。

OpenWork 有一条架构规则：Server 不调用模型，也不持有模型凭证（collaboration.md §1 第 3、6 条）。全部模型调用都在 Computer 的 Engine 中进行。

## 决策

- 点名对象由代码决定。`addressing`（`crates/openwork-collab/src/server/routing.rs`）找精确的 `@<id>`，再加上被引用消息的 Agent 作者。私聊、`@all`、没有点名、点名覆盖全部候选、点名了自己时，返回 `Engage`。
- 本批人类消息全部点名了别人时，`InboxTriage::human_step`（`server/triage.rs`）不给结论，而在 `TriagePayload.routing` 中给出路由题。题面来自 `routing_request`，照搬 Cumora，改为一次判断多条消息。
- `AgentRunner::routed_triage_payload`（`computer/runner/routing.rs`）用本 Agent 的 triage 模型回答，再带 `routed=me|each` 取一次 payload。`parse_route`（`computer/triage.rs`）只有明确的 `"me"` 才收窄。
- 答 `me`：Server 去掉这些人类消息，用其余消息继续判断。没有其余消息时，以 `routing` 跳过，delivery 结算为 `triage_false`。
- 答 `each`、模型出错、超时或答案无法解析：本 Agent 参与。

规则见 [collaboration.md §8.2、§8.3、§15](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**照 Cumora 在 Server 调用一次模型。** Cumora 的 scheduler 对每条消息调用一次云端小模型（`routing.ts` 的 `routeMessage`，由 `scheduler.ts` 调用），出错时唤醒全员。OpenWork 决定不改“Server 不调用模型”的规则，所以每个接收者各自判断。

**没有点名任何人时，选一个 Agent 回答。** Cumora 对这种群消息另有 `routeUnaddressedMessage`，可以答 `one-of-us`，只派一个 Agent。这需要全局唯一的决定，各自判断做不到。将来需要这种决定时，由 Server 把它派给 Computer，统一执行一次，Server 自己仍不调用模型。

## 后果

- Server 与模型凭证仍然分开。每个 Agent 的判断用它自己的 triage 模型，在它自己的沙箱中运行。
- N 个没被点名的 Agent 各调用一次 triage 模型，Cumora 只调用一次。
- 各 Agent 的答案可能不同。失败一律按参与处理，所以不一致只会多唤醒，不会漏掉该回答的 Agent。
- 被点名的 Agent 不答路由题，直接参与。
- 路由题的答案随最终结论写入 `collab_triages.response_mode`。每个 Run 只记录一次 triage。
- 实现点名路由后的实测（真实 deepseek）：`@bo` 提问时，Ada 与 Cy 答 `me`，不跑主模型，只有 Bo 回复。
- `collab_triages.response_mode` 的 CHECK 仍允许 `one_of_us`，但没有代码写入这个值。
- 路由判断之后的 triage 模型失败，见 [triage 失败处理](2026-09-24-triage-failure-handling.md)。
