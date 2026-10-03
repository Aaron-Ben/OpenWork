# Agent Note: Trace 的标识与外键

Status: implemented

## 问题

Trace 写入是有损的：队列满时丢弃信号，一批写入失败时整批丢弃。任何一条 Span 都可能单独缺失。

有些操作没有 Turn，例如手动压缩与 rewind。它们的 Span 也要能归到一条 Trace 里。

这两点决定了哪些引用可以建外键，以及 Trace 的根是什么。

## 决策

- 三个结构标识：`trace_id`、`id`、`parent_span_id`。`trace_id` 必填、不能空白、没有外键。`parent_span_id` 没有外键。
- `session_id` 与 `turn_id` 是业务标签，建外键并随业务行级联删除。`turn_id` 可空。
- Turn 内的 `trace_id` 等于 `turn_id`。无 Turn 的操作在开始时生成 `trace-<uuid>`（`crates/openwork-core/src/session/compaction/mod.rs` 的 `new_trace_id`）。
- `trace_span_payloads.span_id` 建外键并级联删除。
- `trace_spans` 没有序号列，排序用 `started_at, id`。
- 规则见 [trace.md §2、§3.3、§6](../../../../docs/subsystems/trace.md)。

判别法有两条。第一，一件事没有业务行时，能不能直接给它分配这个 id？`trace_id` 能；`turn_id` 不能，它要先在 `turns` 插一行，那一行要求 `client_request_id`、`sequence > 0` 与 `resolved_provider_kind`。第二，引用发生时，被引用的行有没有可能还不存在或已经丢失？有可能，就不建外键。

## 考虑过的方案

**给 `parent_span_id` 建外键。** 没有采用：父 Span 丢失时，外键让所有子 Span 一起插入失败，单点丢失被放大成级联丢失。孤儿 Span 仍有诊断价值，读取时计入完整度缺口。

**让 `turn_id` 兼任 Trace 根。** 早期 schema 这样做，无 Turn 的 Span 因此需要补丁约束 `trace_spans_scope_valid`。基线迁移 `202607260001_initial_schema.sql` 的开头注释记录了这件事。没有保留：业务标签与结构根是两件事。

**用序号 `sequence` 与 `UNIQUE (turn_id, sequence)` 排序。** 单个串行 Agent Loop 下可行，但它把“同一时刻只有一个执行体写入”写进了唯一约束。并发工具、子 Agent 或后台任务向同一 Trace 写入时，两个独立分配的序号会冲突。写入的冲突子句只覆盖主键，唯一约束冲突会让整批事务回滚，一次最多丢 64 条 Span。没有采用。

**`trace_span_payloads.span_id` 也不建外键。** 没有采用：挂载行与它的 Span 在同一批里写入，不会指向不存在的行。

## 后果

- 父 Span 缺失时，子 Span 照常可读，完整度显示 `Partial`。
- 去掉序号只失去“写入顺序”这一项冗余信息，`started_at` 已经表达了它。
- `turn_id` 与 `trace_id` 在 Turn 内取值相同，但语义不同：前者可空并有外键，后者必填且没有外键。
- `sessions.spawn_span_id` 按同一规则不建外键。
