# Agent Note: 正文的存储与清扫

Status: implemented

## 问题

一个 Session 内，System Context 与工具定义几乎不变，但每次 Model Call 都重复发送。20 KB 的工具定义经过 400 次调用，按行存是 8 MB。

记录正文后，Trace 第一次成为体积无界的表。用户删除 Session 后，其中的源码内容也必须真正从库里消失。

## 决策

- `trace_payloads` 以内容哈希为主键，没有 `session_id`；`trace_span_payloads` 把正文挂到 Span（`crates/openwork-core/src/storage/trace.rs`）。
- 删除 Session 时，在同一个事务里清扫：先取出被删 Span 引用的哈希，再只检查这些候选（`crates/openwork-core/src/storage/postgres/session.rs` 的 `delete_session`）。
- 正文挂载与清扫共享一把事务级 advisory lock；`payload_hash` 为 `ON DELETE RESTRICT`。
- 正文按天保留，默认 30 天。过期按 Span 的 `started_at` 计算，只删挂载行，Span 与 token 保留。带标注的 Trace 不过期。
- 保留清理只在启动时执行一次（`purge_expired_trace_payloads`）。
- 截断后必须写原始大小，数据库 CHECK 强制。
- 规则见 [trace.md §5、§10](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

**给 `trace_payloads` 加 `session_id`。** 没有采用：同样的工具定义跨 Session 本来就相同，加上它等于放弃去重。代价是删除 Session 不会级联删除正文。

**只靠周期任务清扫孤儿正文。** 没有采用：这是隐私要求，不是空间优化。删除操作返回时，内容必须已经消失。

**全表 `NOT EXISTS` 扫描。** 没有采用。并发时它不够：正文已插入、挂载还没写入时，扫描会把新正文当成孤儿删掉，随后挂载因外键失败。这个问题是实现时发现的。限定候选还让清扫代价与被删 Session 的大小成正比。

**只用 `RESTRICT`，或只用锁。** 两条防线防的不是同一件事。只有 `RESTRICT` 时，并发写入会随机失败；只有锁时，已提交的引用没有保护。

**按 `trace_payloads.created_at` 计算过期。** 没有采用：正文按哈希去重，一条新 Span 可能引用数月前首次插入的正文。

**加一个常驻定时任务执行保留清理。** 没有采用：仓库没有调度器，桌面应用的启动频率足以支持按天保留。

## 后果

- 保留清理复用删除 Session 的清扫函数，不另起全表扫描。
- 30 天只是起点，不表示正文价值在 30 天处有明确分界。token 用量要能跨长时间比较，所以 Span 与 token 不过期。
- 标为 `bad` 的调用是以后做回归的样本，带标注的 Trace 因此不过期。
- 清理出错时，启动失败。
- 截断时如果不写原始大小，界面上的“已截断”无法量化。
