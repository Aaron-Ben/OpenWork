# Agent Note: Trace 标注的写入与界面

Status: proposed

## 问题

Trace 能回答模型看到了什么、回复了什么。它还不能回答“这次结果好不好”。只有人能判断，而这个判断目前没有地方记录。

`trace_annotations` 表已经存在：评价整条 Trace 或其中一个 Span，`rating` 为 `good`、`bad`、`unsure`，每个目标最多一条。保留清理不过期带标注的 Trace。但 Core 没有写入标注的方法，Desktop 也没有评价的入口，表一直是空的。

## 提议

- Core 增加 `upsert_annotation(trace_id, span_id, rating, note)`。同一目标重复评价时更新原行，不追加一条相反的标注。写入时校验 `trace_id` 属于调用方的 Session。
- `list_traces` 的每一行带上整条 Trace 的标注；`get_trace` 与 `get_trace_by_id` 带上各 Span 的标注。
- Desktop 在 Trace 详情与 Span 详情上提供三档评价与可选备注。
- 标注是业务数据，不是 best-effort 数据。写入失败要报错，不静默丢弃。

现有的表结构、唯一索引与保留规则见 [trace.md §13、§10](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**修改评价时追加一条新标注。** 没有采用：同一目标会有多条相反的评价，读取时还要决定以哪条为准。唯一索引 `uq_trace_annotations_target` 已经按“一个目标一条”建模。

**自动打分、评测集、A/B 对照。** 不在这个提议里。它们需要一整套离线运行与对照基线，给 Span 加几列做不到。这里只提供一个信号：人的判断。

## 验收条件

- 同一目标重复标注是更新，不新增一行。
- 标注既能挂到整条 Trace（`span_id` 为空），也能挂到单个 Span。
- 保留清理不清理带标注的 Trace（已有测试 `retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies`）。
- 删除 Session 时，它的标注随之删除，并有测试断言。
- Desktop 能对一条 Trace 与一个 Span 评价，重新打开后显示已有评价。

## 风险

- `trace_annotations.trace_id` 没有外键。写入路径必须自己校验 Trace 存在且属于该 Session，否则会留下指向不存在 Trace 的标注。
- Span 级标注随 Span 级联删除。Span 本身不过期，但删除 Session 会删掉它们。
- 标为 `bad` 的 Trace 永不过期，正文体积会随标注数量增长。
