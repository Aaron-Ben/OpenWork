# Agent Note: HELD 后直接重发改过的内容

Status: implemented

## 问题

HELD 原来的流程是：拒绝发布并签发 token；Agent `glance` 最新消息；带 token 重试。文案只有一句 `HELD: room <id> changed; reconsider, then retry with --held-token <token>`。模型因此学会“带 token 重试”，但多数时候它该发的是改过的内容。

实现逐字重复拦截后的实测中，两个 Agent 各有一次把 `--held-token` 写在正文之后。CLI 以 `put --held-token before the message body` 拒绝了调用，模型多走了一步。

## 决策

HELD 按 Cumora `server/src/agents/cli.ts` 的做法重写：

- `AgentCommands::hold_reply`（`crates/openwork-collab/src/server/agent_commands/reply.rs`）列出 Agent 没看过的消息，最多 8 条（`HELD_MESSAGE_LIMIT`）。它签发 token，并把 seen sequence 推进到列出的最后一条。
- 所以改过的内容不带任何选项重发，就能通过。还有更新的消息时，会再 HELD 一次。
- token 只用于原稿照发。它绑定 Agent、Run、Room、session 与列出的最大 sequence，只能用一次。
- shim 的 `held`（`computer/shim/render.rs`）写明消息没有发出，并告诉模型直接重发。退出码是 10。
- `extract_options`（`computer/shim/parse.rs`）让 `--quote` 与 `--held-token` 写在正文前后都可以。`--` 之后一律当作正文，重复的选项报错。
- 常驻契约照搬 Cumora `glance-protocol.ts` 的原文：读新消息，重算，再发（`computer/home.rs` 的 `GLANCE_AND_YIELD_RULES`）。

规则见 [collaboration.md §9.1、§7.3](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**glance 后带 token 重试。** 这是 collaboration.md §9.1 原来的流程。之后按 Cumora 重写 HELD，取代了它，起因是上面的实测。

**`--held-token` 只能写在正文之前。** 这是原来的 CLI 规定。重写 HELD 时改为前后都可以。Cumora `cli-parse.ts` 的 `parseArgs` 也在 `--` 之前的任何位置识别选项。

## 后果

- 重写后的实测（数 1 到 6）：3 次 HELD 后，模型都直接重发下一个数。没有用 `--held-token`，CLI 也没有拒绝任何参数。
- HELD 本身推进 seen，`inbox`、`glance` 与 `messages` 也推进 seen。读过的消息不再触发 HELD。
- 推进 seen 是尽力而为。Redis 写入失败时只记警告，重发会再 HELD 一次。
- 正文中如果出现 `--quote` 或 `--held-token` 这样的词，会被当作选项。这时要把正文放在 `--` 之后。
- 与 Cumora 的不同：Cumora 照发原稿用 `--send-anyway`，HELD 的退出码是 2。OpenWork 用 token，退出码是 10。
- 验收：`computer::shim::render::tests::held_replies_say_a_plain_resend_goes_through`、`computer::shim::parse::tests::acc_12_reply_takes_a_quote_anywhere_outside_the_body`、`posting::acc_21_held_lists_eight_messages_and_listing_counts_as_seen`。
- 发布前的其他闸见 [三道闸](2026-09-24-reply-gates.md)。
