# Agent Note: 工具自己决定结果的形状

Status: implemented

## 问题

工具结果留在 Conversation 里，随每次 Model Call 重发。先全部返回、再由请求投影截断，是最贵的做法：截掉的内容丢了，留下的仍然偏大，模型还要再调一次工具去找被截掉的部分。

投影不知道结果是什么。它不知道源码的关键部分常在中间，也不知道构建日志的错误常在结尾。

## 决策

每个工具按自己的内容决定保留什么（[tools.md §7、§8.2](../../../../docs/subsystems/tools.md)）。上限的数值见 [工具结果上限](2026-09-24-tool-result-bounds.md)。

- read 只在末尾截断，停在最后一个完整行，并写明续读的 `offset`。
- grep 与 glob 达到上限后继续扫描，只计数，结尾报告准确总数。
- glob 按修改时间倒序。最近改过的文件通常与当前任务相关。
- bash 合并 stdout 与 stderr，保留开头 2 KiB 与结尾 14 KiB。
- write 与 edit 只返回一行摘要，完整 diff 只进 Artifact。

## 考虑过的方案

**read 保留头尾。** 没有采用：头尾截断适合日志，不适合源码。从中间截掉，模型只能再读一次。

**grep 与 glob 接受 `maxResults` 参数。** 2026-07 的设计有这个参数，glob 默认 200 条。没有保留：模型拿到“可能还有更多”，无法判断该不该收窄搜索。准确总数给出这个依据。

**bash 只保留尾部。** DSH 这样做（`packages/subprocess/subprocess-local/src/output.ts`，`packages/shell/tool-bash/src/index.ts` 的工具描述）。没有照搬：命令最开始的报错有时在开头。开头只留 2 KiB，因为构建输出的开头多半是 `Compiling ...` 一类的噪音。

**stdout 与 stderr 分别截断。** 2026-07 的 `BashResult` 有两个字段。没有保留：编译错误走 stderr，测试结果走 stdout，分开截断会拆散先后关系。两路各留 16 KiB 也超出输出上限。

**edit 把 diff 返回给模型。** 没有采用：edit 是一个 Turn 里调用最频繁的工具，结果上的每一行都会累加。模型需要确认时，按摘要里的行范围读那一段。

**随包分发 `rg` 二进制。** DSH 的 glob 与 grep 启动打包的 ripgrep（`packages/fs/tool-fs-search/src/glob.ts`）。OpenWork 用 `grep-searcher`、`grep-regex` 与 `ignore` 库。原始记录没有写明比较过程。

**删掉 glob，让模型用 bash 的 `find`。** 没有采用：glob 跨平台、经文件工具围栏、结果有界，也不执行任意代码。

## 后果

- 投影的单条上限只是兜底，正常结果不会被它从中间截断。
- 统计准确总数时，grep 与 glob 仍要扫完整个范围，或扫到 30 秒超时。
- grep 与 glob 跳过隐藏文件，list 不跳过。模型要找隐藏目录里的文件时，需要 list 或 bash。
- bash 的拒绝识别只看截断后的输出。拒绝信息落在省略的中间时，结果没有拒绝标记（[permissions.md §7](../../../../docs/subsystems/permissions.md)）。
