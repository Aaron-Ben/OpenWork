# Agent Note: CLI 的正文写法、帮助与 card update 的可选字段

Status: implemented

## 问题

Agent 只能通过 `openwork` CLI 协作。模型第一次用某个命令时，会按常见 CLI 的习惯猜写法。每猜错一次，Turn 就多一次工具调用，有时还要再读一次帮助。

真实模型实测中，这类多余步骤反复出现：

- 实现每轮增量后的实测：Ada 写成 `openwork reply <room> "<body>"`。CLI 只接受 `--stdin`、`--file` 或 `--`，拒绝了这次调用。
- 改为位置参数正文后的复测：Bo 先猜 `--body`，再调 `openwork reply --help`。CLI 把子命令后的 `--help` 当作房间 id，只返回错误。
- 改为位置参数正文后的复测：Ada 在 glance 中看到 `(no messages)`，以为命令出了错，又调了 `--help` 与 `messages`。
- 实现卡片唤醒后的实测：Ada 只想改卡片描述。`card update` 要求 `--title`，也不支持 `--stdin`，Ada 多走了几步才成功。

这些步骤不改变结果，但花时间和 token，也增加模型中途放弃的机会。

## 决策

- `reply` 与 `dm` 的正文直接写在 id 后面，多个参数按空格拼接。`--stdin`、`--file <path>` 与 `--` 仍然可用。实现是 `crates/openwork-collab/src/computer/shim/parse.rs` 的 `parse_body`。缺正文时，`missing_body` 的错误文本直接给出正确写法。
- `--` 之前任何位置出现 `--help` 或 `-h`，都显示帮助。子命令后的 `--help` 只显示匹配最长前缀的用法行。实现是 `computer/shim/mod.rs` 的 `help_request`。
- glance 没有新消息时，输出 `No new messages since you last read this room (latest sequence N).`（`computer/shim/render.rs` 的 `render`）。
- `AGENTS.md` 契约写明发消息的写法与 `openwork <command> --help`（`computer/home.rs`）。
- `card update` 的 `--title` 与描述都可选，但至少给一个。描述只能有一个来源：`--description`、`--stdin` 或 `--file`。`--description ""` 清空描述。shim 在 `card_update_args` 中检查。Server 在 `server/agent_commands/cards.rs` 中再检查一次，拒绝文本相同（`NOTHING_TO_UPDATE`）。

正文写法以 Cumora 为参照：用法行 `reply <convo_id> "<body>"`（`server/src/agents/cli.ts`），选项位置不限、`--` 之后一律当正文（`cli-parse.ts` 的 `parseArgs`）。`card update` 的可选字段与清空规则照 Cumora `card edit`（`cli.ts` 约 5845 行）。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §7.1、§9 开头与 §11.2。

## 考虑过的方案

**保留旧的正文语法。** 旧语法只接受 `--stdin`、`--file` 或 `-- <body>`，可以避开 shell 引号问题。没有采用：实现每轮增量后的实测中，模型每次都先写成位置参数，失败后才改对。新写法保留这三种方式，只增加位置参数，没有损失。

**`card update` 保持 `--title` 必填。** 这是原来的实现：标题必填，没给描述就清空描述。没有采用：只改描述也要重抄标题，忘了给描述还会静默清空描述。

**照 Cumora `card edit` 只接受 `--description`。** Cumora 的描述没有 `--stdin` 或 `--file`。没有采用：描述常含引号、换行与 `$`，放在命令行里容易写错。`reply` 已经有 `--stdin`，`card update` 用同一套写法，模型不需要学第二套。

## 后果

- 效果已经实测：修正 `--help`、空 glance 文本与契约写法之后的复测中，Ada 与 Bo 只调了 glance 与 reply，没有 `--help`，也没有失败重试（原进度文件 2026-09-24 的检查记录）。
- 正文不加引号时，shell 会先处理它：连续空白压成一个，`$` 被展开。契约因此提示含引号或 `$` 时用 `--stdin`。
- 以 `--` 开头的正文必须在前面再加 `--`，否则 CLI 报 unknown option。`--` 之后的 `--help` 是正文，不显示帮助。
- fake OpenCode 在卡片 Turn 中用 `card update --stdin` 改描述（`tests/fixtures/fake-opencode.zsh`），端到端覆盖了 `card update` 的可选字段。
- 测试：`help_after_a_subcommand_shows_only_that_usage`、`empty_glance_says_nothing_new_since_the_last_read`、`reply_and_dm_accept_the_body_as_plain_arguments`、`card_update_takes_either_field_and_one_description_source`。
- 列表输出的截断规则见 [模型可见的消息上限](../architecture/2026-09-24-model-visible-message-limits.md)。
