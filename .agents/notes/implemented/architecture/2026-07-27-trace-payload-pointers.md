# Agent Note: 正文只记 Message 回答不了的内容

Status: implemented

## 问题

压缩之后，模型看到的 Conversation 与原始 `messages` 不再相同。它是 checkpoint 的摘要加边界之后的消息，再加 System Context 与 runtime reminder。排查“模型为什么答错”时，需要的正是这个组装结果，而没有任何业务表保存它。

另一方面，用户输入、成功的响应、工具参数与结果已经在 `messages` 里。`messages` 只增不改不删。

## 决策

- 已经写进 `messages` 或 `conversation_compactions` 的内容，Trace 只留指针：`response_message_id`、`(turn_id, provider_call_id)`、`checkpointId`。
- 只在 Trace 里的内容写进正文槽位：`request`、`system_context`、`tool_definitions`，以及没有产生 Message 的 `response`（`crates/openwork-core/src/session/trace.rs` 的 `TracePayloads` 与 `ModelCallTraceGuard::finish`）。
- Tool Span 只在结果被拒或没有持久化时写 `response`。
- 正文只经 `get_span_payload` 按需读取，`get_trace` 不返回正文。
- 规则见 [trace.md §5、§9](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**把 Message 内容复制一份到 Trace。** 没有采用。理由不是节省空间，而是避免同一份内容有两个可能不一致的版本。Trace 是 best-effort 的。两边不一致时，无法判断该信哪一边。

**存描述符，用 `checkpoint_id` 加消息序号区间重建投影。** 几乎不占空间。没有采用：System Context 与 reminder 会随版本变化，重建出的是今天的组装结果，不是当时的结果。排查时，近似的重建比没有更糟，排查者会对着一份模型从没见过的输入找原因。

**记录未截断的完整工具输出。** 没有采用：模型看到的是截断后的结果，质量分析需要的正是它。完整输出是运维关注点，一次 `bash` 就可能产生上百 MB。

**把正文放进 `get_trace` 的结果。** 没有采用：打开一个 Turn 就要读几 MB JSONB，而用户多数时候只看时间线。列表用 `requestMessageCount` 与 `byte_size` 显示规模。

**把正文放进 `attributes`。** 没有采用：读 Span 时就要同时读正文，白名单校验也无法进行。

## 后果

- `request` 槽位每次都在变长，无法去重。它是体积的主要来源，由截断与保留期限制。
- 成功调用的响应要从 Message 读取。Message 被删除时，`response_message_id` 置空。
- 界面上缺少正文时只能陈述事实。历史记录可能来自旧版本，正文写入也可能单独失败，原因无法从读取结果判断。
