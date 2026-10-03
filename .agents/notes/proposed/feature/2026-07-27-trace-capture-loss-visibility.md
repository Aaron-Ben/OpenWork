# Agent Note: 显示 Trace 的采集损失

Status: proposed

## 问题

Trace 写入有损。`PostgresTraceRecorder` 统计两个数：队列满时丢弃的信号数 `dropped_signals`，写入失败的信号数 `write_failures`（`crates/openwork-core/src/storage/trace.rs`）。`flush_turn` 与 `flush_session` 返回这两个数，`PostgresTraceRecorder::metrics` 也能读到它们。

调用方都不读结果：Turn 结束后的 `flush_turn` 与手动压缩后的 `flush_session` 都丢弃返回值。用户只能从完整度的 `Partial` 或 `None` 间接看到损失，看不到损失有多大，也看不到损失来自队列满还是数据库失败。

现在只有一个 Agent Loop 写入，损失很少。并发工具、子 Agent 与后台任务增多后，队列满的概率会上升，这一项的优先级随之上升。

## 提议

- 在 Desktop 的 Trace 页面显示进程启动以来的 `dropped_signals` 与 `write_failures`，非零时醒目显示。
- 计数来自 `PostgresTraceRecorder::metrics`，经一个只读查询提供给 Desktop。
- 只显示，不改变任何业务结果。计数不写入数据库，进程重启后归零。

写入与计数的现状见 [trace.md §6](../../../../docs/subsystems/trace.md)。

## 考虑过的方案

<!-- agent-note: 原始记录没有备选方案 -->

## 验收条件

- 队列容量设为 1 并连续写入多条信号时，界面显示非零的丢弃计数。
- 正文写入失败时，界面显示的写入失败计数增加，Span 本身仍可读。
- 计数为零时，界面不显示警告。
- 读取计数不阻塞 Turn，也不影响 Trace 写入。

## 风险

- 计数是进程级的，不能对应到某一条 Trace。用户看到非零值后，仍要靠完整度判断哪条 Trace 受影响。
- 计数在进程重启后归零，看不到上一次运行的损失。
- 正文写入失败与整批失败都计入 `write_failures`，两者在界面上无法区分。
