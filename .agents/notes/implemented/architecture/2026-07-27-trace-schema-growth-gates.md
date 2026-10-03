# Agent Note: Trace 结构增长的门槛

Status: implemented

## 问题

Trace 有新增 `kind` 的门槛，却没有新增属性的门槛。结果 kind 只有 3 个，属性一度增加到 78 个，其中有两个嵌套类型。详情面板平铺 50 多行，就等于没有面板。

一个属性的真实代价不是存储，JSONB 里的空字段不占空间。代价是每加一个属性要改多处：Rust 类型、前端白名单与归类、三个语言包。还有理解成本。

## 决策

新增属性要同时通过三个问题：

1. 有人会因为它做出不同的决定吗？“看着有用”不算。
2. 能从别的字段算出来吗？能算出来就不存。
3. 是不是几乎所有行上都是空或同一个值？是的话，它属于别的层级。

新增 kind 要同时满足三条：有自己的起止与成败；不属于任何 Model Call 或 Tool Call，模型不知道它发生；可能脱离 Turn 发生。`compaction` 满足全部三条。

- 属性是三个版本化类型，`deny_unknown_fields`（`crates/openwork-core/src/session/trace.rs`、`tool_trace_attributes.rs`）。
- Desktop 用 `TRACE_ATTRIBUTE_PLACEMENT` 给每个键归类：某个 kind 的主字段，或“详细”折叠区（`desktop/src/features/traces/traceViewModel.ts`）。
- 规则见 [trace.md §12](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**保留逐次传输明细 `attempts[]`。** 每个元素有 10 个子字段，包括 `retryDelayMs`。它回答“哪个 Provider 在抖”，属于运维定位。删除后保留 `attempt_count`、`errorPhase`、`deliveryState`、`httpStatus`、`providerCode`、`error_code`；`attempt_count > 1` 加总耗时足以判断慢是不是重试造成的。确实失去的是每次尝试的等待时间与中间尝试的错误码，排查退避策略时会缺数据。顶层的 `httpStatus`、`providerCode` 因此只有一个来源，含义是最后一次尝试。

**只删字段，不分层。** 删字段解决“字段太多”，分层解决“不知道哪几个重要”。实际的障碍是后者，所以两者都做。

**保留内容的影子字段。** 没有正文时，曾用 13 个字节数与分角色计数近似内容，例如 `requestContentBytes`、`inputBytes`。正文与 `trace_payloads.byte_size` 接管后删除。`requestMessageCount` 与 `toolDefinitionCount` 是语义规模，列表不加载正文也要显示，所以保留。

**其他删除项。** `summaryEstimatedInputTokens` 是三项估算之和（第 2 问）。`progressEventCount`、`validationMs`、`resultPersistMs`、`resultPersistErrorCode` 没人据此做决定（第 1 问）；`resultPersisted` 保留，因为模型有没有看到结果是质量问题。`appVersion` 每行存一次进程常量，移到 `turns.app_version`（第 3 问）。三个本地亚毫秒耗时合并为 `prepareMs`。

**给 Plan、Todo、Memory 各建一个 kind。** 没有采用：模型通过工具使用它们，它们本来就是 `tool_call`。会自己运行多轮流程的能力，更可能是子 Session。

## 后果

- `artifactCount` 与 `artifactTypes` 看起来可派生，实际不是：后者排序去重，前者是原始个数。回答第 2 问时看实现，不看名字。
- `temperature`、`topP`、`toolChoice` 是后来加入的，它们通过第 1 问：参数改了，行为就会变。
- `summaryRetryDelayMs` 保留在摘要采样子 Span 上。它是摘要重试的节奏，不是传输明细。
- 新增 kind 的成本：迁移改 `kind` 与独占列的 CHECK；新的属性类型与白名单；新 Guard 与 drop 处理；完整度与正文槽位的决定；前端分支与三语文案。可能没有 Turn 的 kind 还必须同时提供读取入口，否则只写不读。
- `thresholdPercent` 与 `outputTruncated` 在类型与白名单里，Core 不写入它们。
