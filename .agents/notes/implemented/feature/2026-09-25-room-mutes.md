# Agent Note: Agent 静音群聊，并封住未读尾巴

Status: implemented

## 问题

一个 Agent 加入的群越多，与它无关的讨论就越多。每条讨论都会唤醒它，或进入它的 durable inbox，再走一次 triage。Agent 需要一种方式退出与自己无关的讨论，同时不漏掉直接找它的消息。

## 决策

命令与回执照搬 Cumora `server/src/agents/cli.ts` 的 `cmdMute`、`cmdFollow` 与 `parseMuteUntil`：

- `openwork mute <room-id> [--for <N>m|h|d|w | --until <time>]`、`mute list`、`follow`。期限 1 分钟到 90 天。
- 状态存为 `collab_room_members.mute_expires_at`。`infinity` 表示一直静音（迁移 `202609240008_room_mutes.sql`）。
- 静音时把 `last_read_seq` 推进到房间最后一条（`server/room_mutes.rs`）。Cumora `cmdMute` 也把 `last_read_at` 推到当前。
- `@<自己>` 与引用自己的消息仍唤醒并送达（`server/messages.rs` 的 `wake_recipients` 与 `inbox`）。
- Direct Room 不能静音。Desktop 没有静音入口，因为系统不唤醒 Desktop 用户。

规则见 [collaboration.md §10.1](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**布尔的 `muted` 列。** 这是最初的表结构，没有任何入口能把它设为真。没有保留：布尔值表达不了到期时间。迁移删除了这一列。

## 后果

- 恢复后从静音时的位置接着读，静音前的积压不补发。
- 有 `@` 或引用到达时，静音以来的全部未读一起送达，不只是那一条。
- 静音不是完全安静：被点名或被引用仍会唤醒。
- 测试：`tests/mutes.rs::acc_23_a_muted_group_only_delivers_mentions_and_follow_skips_the_backlog`、`::acc_23_direct_rooms_foreign_rooms_and_bad_spans_are_rejected`。
