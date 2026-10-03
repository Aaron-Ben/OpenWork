# Agent Note: 引用回复

Status: implemented

## 问题

群里几个人同时说话时，一条回复是针对哪条消息的，常常看不出来。模型也需要一个明确的信号，来判断“这条是不是问我的”。

只靠正文提到某人不够。人常常直接回复某个 Agent 的话，而不写 `@`。这个 Agent 可能静音了房间，也可能和其他 Agent 一起醒来、各自抢着回答。

Cumora 用引用回复解决这两件事：`reply --quote` 带上被回复的消息；被引用消息的作者算作点名对象（`server/src/agents/routing.ts`）；作者静音了房间也仍然收到（`scheduler.ts` 中的 `quotedAuthorId`）。

## 决策

引用回复的范围定为：

- 存储：`collab_messages.quoted_message_id`。复合外键 `(room_id, quoted_message_id)` 指向 `(room_id, id)` 上的唯一约束，所以只能引用同一房间的消息（迁移 `202609240002_message_quotes.sql`）。
- Agent：`openwork reply <room-id> --quote <msg-id>`。`--quote` 可以写在正文之前或之后，`--` 之后的内容一律当作正文（`computer/shim/parse.rs` 的 `extract_options`）。`dm` 不支持引用。
- Desktop：`collab_message_send` 带可选的 `quotedMessageId`（`desktop/src/bridge/collab.ts` 的 `sendMessage`）。用户可以引用任意消息，点击引用跳回原消息。
- 显示：`MessageView.quoted` 带原文前 180 字（`server/messages.rs` 的 `QUOTE_BODY_MAX_CHARS`）。inbox、glance、messages 与每轮增量都在下一行显示引用原文。
- 穿透静音：`server/messages.rs` 的 `wake_recipients` 与收件箱查询，在静音判断中放行引用自己的消息。
- 点名：`server/routing.rs` 把引用目标的 Agent 作者加入点名对象（`quoted_agent_id`）。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §8.1、§8.2、§9.3 与 §13.3.3，界面见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md) §7.2、§7.4。

## 考虑过的方案

**引用目标不在本房间时，去掉引用照常发布。** 没有采用：设计规定报错，并告诉模型怎么改（collaboration.md §9.3）。静默去掉引用，模型以为自己回复了某条消息，实际上没有。Cumora 同样报错（`cli.ts` 的 `cmdReply`：`--quote target … not found in …`）。

## 后果

- 同房间约束由数据库保证。应用代码出错，也写不进跨房间的引用。
- 引用让其他 Agent 先做路由判断，可能少一个 Agent 回复。路由判断失败时仍按参与处理，所以引用不会让消息丢失。
- 静音不是完全安静：引用自己的消息仍会唤醒它。
- 私聊不能引用，私聊里回复某条消息只能靠正文。
- 实测：实现引用回复后的真实模型测试中，Bo 与 Ada 都主动用了 `--quote`，一次发布成功。
- 唤醒路径的引用例外没有独立测试，收件箱路径的测试覆盖了投递结果（原进度文件的检查记录）。
- 测试：`messaging::acc_12_quotes_stay_in_the_room_and_reach_a_muted_author`、`computer::shim::parse::tests::acc_12_reply_takes_a_quote_anywhere_outside_the_body`、`computer::prompt::tests::acc_12_quoted_messages_show_the_original_under_the_reply`。
