# Agent Note: 危险命令清单是封闭的

Status: implemented

## 问题

`auto` 模式下 bash 可以写工作区，所以 `rm -rf src` 在沙箱里会成功。bash 的删除不可撤销：`file_change` 撤销只覆盖 `write` 与 `edit`。未提交的改动与未跟踪的文件因此暴露。

但任何按命令文本判断的机制都有一个倾向：它会慢慢长成“可疑命令”黑名单。发展到最后，又回到按文本判断安全，打断重新出现。

## 决策

- `DangerKey`（`crates/openwork-tools/src/permission/danger.rs`）只有四个键：`rm_recursive_or_force`、`find_delete`、`git_clean_force`、`nesting_too_deep`。加一条的标准只有一个：它在沙箱允许的范围内能批量丢弃未提交的工作。
- 检测只看程序名与标志。操作数含 `$VAR` 或 glob 时照常检测。程序名是动态的、或语法树有错误时，`detect` 返回 `None`，命令直接在沙箱内执行。
- 包装器最多剥 `MAX_WRAPPERS = 16` 轮。`bash -c` 嵌套超过 `MAX_NESTING = 8` 层时按命中处理。
- `gate`（`crates/openwork-core/src/session/approval.rs`）只在 `SandboxMode::Auto` 下为命中单独出卡。越界卡片上，只要命令命中，就附上危险命令标注，不区分模式，也不区分越界是否写工作区。
- 批准后，命令仍在当前模式的沙箱内执行。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**把丢弃改动的 git 命令也放进清单。** `git reset --hard`、`git checkout -- .`、`git restore`、`git stash` 都能丢弃未提交的改动。没有采用：`.git` 在沙箱内只读，这些命令拿不到 `.git/index.lock`，工作区不变。开发机在 2026-09-24 实测过。它们会走写 `.git` 的越界卡片，再问一次只是重复。`git clean -f` 不写 `.git`，所以它在清单上。

**收录更多可疑命令。** 单文件 `rm`、`mv` 覆盖、`> file` 截断都不在清单上。没有采用：清单会长成黑名单。maka 记录过这个教训：八轮审查反复枚举危险形态后，结论是从静态字符串判定 shell 的运行时效果不可判定。maka 的危险分类因此只用于给出准确的确认理由（`packages/core/src/permission.ts`，“Shell command categorization” 一节的注释）。

**看不懂就问。** 没有采用：检测不是边界，边界是沙箱。看不懂就问，打断会回到 `for f in *.rs; do ...; done` 这类最常见的形态上。

**`accept-edits` 下也单独出卡。** 没有采用：内核本来就拒绝 bash 写工作区，命令之后走越界卡片。

**不做检测，或只检测强制删除。** 在 DSH 的 `packages/shell/` 与 `packages/sandbox/` 源码中没有找到危险命令检测。Codex 只把带强制选项的 `rm` 一类算作危险（`codex-rs/shell-command/src/command_safety/is_dangerous_command.rs`）。没有采用：工作区内的删除不可撤销，沙箱不管。

## 后果

- 漏报的后果限于工作区内未提交的工作。误报的后果是多一张卡片。两者都不越过边界。
- `$CMD -rf x` 与语法错误的命令不检测。
- 嵌套深度上限与 Codex 相同（`MAX_DANGEROUS_COMMAND_WRAPPER_DEPTH = 8`）。
- Trace 以 `dangerMatch` 记录命中的键。调整清单时，可以按键反查影响面。
- `crates/openwork-sandbox/tests/matrix.rs` 的 `auto_builds_and_reads_but_git_writes_need_an_escalation` 固定了 `reset --hard` 与 `stash` 被拒、`git clean -fd` 成功。`git restore` 与 `git checkout -- .` 没有自动测试。
- 测试：`danger::tests::acc_26_27_28_29_the_listed_commands_hit_or_miss_as_specified`；core `acc_26_32_34_a_dangerous_command_asks_only_in_auto_and_is_traced`。
- 沙箱不可用时不检测，见 [沙箱不可用时不做危险命令检测](2026-09-24-sandbox-unavailable-skips-danger-detection.md)。
