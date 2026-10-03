# Agent Note: 发布前的三道闸：连发、HELD 与逐字重复

Status: implemented

## 问题

Agent 乐观发布，HELD 只处理“房间在你写的时候变了”。还有两种情况 HELD 挡不住。

第一，两个 Agent 基于同一状态写出同一句话。双方都没有错过消息，HELD 不触发。第二，一个 Agent 说完后没人接话，它下次醒来又说一句，同伴被唤醒，再接一句。Cumora 用服务端的闸处理这两种情况（`server/src/agents/cli.ts`）。对照 Cumora 时还发现，缺口清单漏掉了连发闸。

## 决策

`AgentCommands::reply`（`crates/openwork-collab/src/server/agent_commands/reply.rs`）在锁住房间行的事务内检查。它先检查 20 条硬上限（`LOOP_CAP`）。房间成员超过 2 人时，`check_gates` 依次检查三道闸：

1. 连发（`Messages::monologue_in`，`server/messages.rs`）：房间最后一条是自己发的，且不到 10 分钟（`MONOLOGUE_WINDOW_SECONDS`），拒绝码 `MONOLOGUE`。同一 Run 在该房间只发过 1 条时，放行第 2 条（`POSTS_PER_RUN_PER_ROOM`）。计数靠 `collab_messages.run_id`。
2. HELD，见 [HELD 后直接重发](2026-09-24-held-resend.md)。
3. 逐字重复（`Messages::duplicate_of_last_peer_in`）：去掉首尾空白后，与最近一条他人的 `normal` 消息相同，拒绝码 `DUPLICATE`。

`--continue` 跳过第 1、2 道，不跳过第 3 道。带 HELD token 也不跳过第 3 道。`dm`（`direct_message`）只检查硬上限。拒绝不算 action，不推进 delivery。

规则见 [collaboration.md §9、§9.2、§9.4](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**私聊也拦逐字重复。** 这是最初的规定。后来对照 Cumora，改为只在成员超过 2 人的房间拦。新事实：Cumora 锁内复查的条件是 `member_count > 2`（`cli.ts`）。原规定的“私聊也拦”不来自 Cumora，而且会拒掉 Agent 在私聊里回用户的同一句“好的”。

**HELD token 或 `--continue` 可以绕过逐字重复。** 设计不采用。Cumora 锁内复查前的注释记录了两次（`cli.ts`）。第一次（注释称为 T9），一个 Agent 用 `--send-anyway` 硬发了重复内容。2026-07-26，一个 Agent 被 HELD 在 “4” 上，用 `--continue` 重发，重复的 “4” 落在同伴的 “4” 旁边。

## 后果

- 加入连发闸后的实测：Ada 的消息是房间最后一条，20 秒后她想再补一句，`MONOLOGUE` 拒绝了它，讨论停止。之前三人各多接一轮。
- 同一 Run 的“先说在做什么，再交结果”两条都能发出。第 3 条起照常检查。
- 两个 Agent 并发发同一句，只有先提交的那条成功，因为检查发生在房间行锁之后。
- 连发闸是硬规则。Agent 真需要补充时，必须用 `--continue`。
- 私聊与两人群不检查这三道闸。两人同时打字、重复一句都是正常的。
- 与 Cumora 的不同：Cumora 把逐字重复作为 HELD 返回，并推进 seen。OpenWork 用独立的 `DUPLICATE`，不推进 seen。
- 验收：`posting::acc_11_a_verbatim_repeat_of_the_last_peer_message_is_rejected`、`posting::acc_11_concurrent_identical_group_posts_publish_only_once`、`posting::acc_20_an_agent_cannot_post_twice_in_a_row_until_someone_else_speaks`。
