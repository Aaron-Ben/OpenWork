# Agent Note: 可操作的错误与重复提醒

Status: proposed

## 问题

工具失败时，模型拿不到出路，最常见的反应是换个写法重试同一件事，直到耗尽 `max_model_calls`。

部分错误文本只说出了什么错，不说下一步：

- edit：`oldString not found in <绝对路径>`（`crates/openwork-tools/src/builtins/filesystem/edit.rs`）。
- 文件工具的路径不存在：`failed to resolve <绝对路径>: <系统错误>`（`crates/openwork-tools/src/checked_path.rs`）。
- 重复调用：同一工具、同一参数连续调用达到 `doom_loop_threshold`（默认 3，`crates/openwork-agent/src/policy.rs`）时，Turn 直接停止，结果文本是 `doom loop detected for tool '<name>'`（`crates/openwork-core/src/session/run_loop/mod.rs`）。停止之前没有提醒。

另一部分已经给出下一步，例如 read 遇到二进制文件或目录、grep 的正则无效、glob 的模式无效（[tools.md §7](../../../../docs/subsystems/tools.md)）。

## 提议

逐个工具改写错误文本，让它说明下一步：

| 现在 | 改为 |
|---|---|
| `oldString not found in src/a.rs` | `oldString not found in src/a.rs. Read the file again and copy the exact text, including indentation.` |
| `doom loop detected for tool 'grep'` | `You have called grep with identical arguments 3 times and got the same result. Look at the previous result, or change the pattern or path.` |
| `failed to resolve /abs/src/x.rs: No such file or directory (os error 2)`（`crates/openwork-tools/src/checked_path.rs`） | `src/x.rs does not exist. Use glob to find the file.` |

- 错误文本中的路径在工作区内时相对工作区。
- 同一参数的重复调用，在达到 `doom_loop_threshold` 之前先返回一次提醒。达到阈值时，仍按现有规则停止 Turn。

## 考虑过的方案

<!-- agent-note: 原始记录没有备选方案 -->

## 验收条件

- tools.md 验收一节原第 43 条：工具失败文本都说明下一步该做什么。逐个工具列出改前与改后的文本。
- 同参数的第 N 次调用（N 小于阈值）得到提醒；第 `doom_loop_threshold` 次仍停止 Turn。

## 风险

- 提醒占一次 Model Call 的结果位置。阈值是 3 时，只剩一次机会提醒。提醒放在第几次，需要在实现时定。
- 错误文本写死了建议的下一步。工具改名或换参数时，要同时改这些文本与测试中逐字的断言。
