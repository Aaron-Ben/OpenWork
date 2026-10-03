# Agent Note: 模型可见的消息照搬 Cumora 的上限与原文

Status: implemented

## 问题

Cumora 不做中央编排。每个 Agent 的主模型自己读房间，自己决定谁该回答。所以主模型每轮读到什么，直接决定协作质量。

读到的内容有三个来源：每轮增量、CLI 列出的消息、triage 模型的输入。没有上限时，一个繁忙的房间会挤满上下文。上限定得随意，又会悄悄丢掉消息，或把关键的一句截掉。

Cumora 已经在真实协作中调过这些数字与文案，并在源码注释里记下事故。例如 `renderInboxDigest` 的注释写明：不能静默省略未读，因为本轮会把它们标为已读。自己重新定数字，要重新踩一遍这些坑。

## 决策

数字与文案照搬 Cumora，只在 OpenWork 自己的约束处偏离。

- 每轮增量：`crates/openwork-collab/src/computer/prompt.rs` 的 `DIGEST_MAX_MESSAGE_LINES = 40`、`MESSAGE_BODY_MAX_CHARS = 600`、开头一段 `WOKEN` 与名册 `push_roster`。来源是 Cumora `server/src/agents/computer/daemon.ts` 的 `chatDelta`、`snapshotUnread`、`renderInboxDigest`，名册来自 `personas.ts` 的 `rosterSection`。增量不重复 persona。
- CLI 输出：`computer/shim/render.rs` 的 inbox 240 字、messages 280 字、glance 200 字、HELD 200 字、引用 180 字。HELD 最多 8 条，见 `server/messages.rs` 的 `HELD_MESSAGE_LIMIT`。来源是 Cumora `cli.ts` 的 `cmdInbox`、`cmdMessages`、`cmdGlance` 与 `cmdReply`。
- `messages --json` 输出完整正文。`inbox`、`glance`、`messages` 都推进 seen（Cumora `cmdMessages` 与 `cmdGlance` 的 `recordSeen`）。
- triage 模型的输入：本批未读与近期上下文各取最后 40 条，每条 500 字（`server/triage.rs` 的 `TRIAGE_MESSAGES_MAX`、`TRIAGE_BODY_MAX_CHARS`，Cumora `triage-core.ts` 的 `compactMessages`）。
- 原文：`computer/home.rs` 的契约开头与 `GLANCE_AND_YIELD_RULES` 来自 Cumora `standingPrompt` 与 `glance-protocol.ts` 的 `GLANCE_YIELD_RULES`。改动只有：`cumora` 换成 `openwork`；去掉表情回应的出路；共享交付物只写 board card；“wake brief” 换成 “turn prompt”。增量开头把 “cerebellum triage” 换成 “triage”。

与 Cumora 的不同：

- 时间用 `+08:00`，不用 UTC。
- 截断的正文以 `…` 结尾，列表末尾写明 `messages --json`（`CUT_NOTE`）。Cumora 只截断，不加标记。
- `messages` 默认列 50 条（`shim/parse.rs`）。Cumora `cmdMessages` 的默认值是 20。原始记录没有说明这个差异。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §7.1–§7.3、§8.3 与 §9.1。

## 考虑过的方案

**时间照 Cumora 用 UTC。** Cumora 的增量写 `Current time (UTC)`。没有采用：本仓库的库表与全部模型可见时间都是东八区（collaboration.md §13.1）。时间统一后，模型读到的消息时间、静音到期时间与当前时间不需要换算。

## 后果

- 每个数字都有出处。以后想改，可以先对照 Cumora 的注释与事故记录。
- 增量超过 40 行时，就地写明未显示的条数与读取命令。模型知道还有消息，但要多调一次 `messages` 才能读到。
- 长正文在列表中只显示开头。模型要完整正文时，多调一次 `messages --json`。
- 契约原文是长英文段落，含 Cumora 的中文例句，保持原样。改写会让 OpenWork 与 Cumora 的行为不再可比。
- 增量开头告诉主模型不要重新判断该不该回应。所以 triage 的判断质量直接影响主模型的行为。
- 测试：`acc_08_message_turn_prompt_renders_the_documented_delta`、`acc_21_listings_cut_long_bodies_like_cumora`、`acc_21_messages_json_prints_full_bodies`、`acc_21_triage_input_keeps_the_latest_forty_messages_cut_to_500_chars`。
- CLI 写法见 [CLI 的正文写法与帮助](../feature/2026-09-24-cli-message-body-and-help.md)。
