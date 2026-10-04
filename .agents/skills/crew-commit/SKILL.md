---
name: crew-commit
description: 在 Crew 仓库提交前使用。用户说“提交吧”或要求提交、改写提交时，按这里确认改动、运行检查、写提交说明，并汇报实际运行的命令。
---

# Crew 提交流程

前提：用户在本次对话中明确同意了这次提交。一次同意只对一次操作有效，不包括推送（根 [AGENTS.md](../../../AGENTS.md)“何时直接做，何时先问”）。

## 1. 确认要提交什么

```bash
git status --short
git diff --stat
```

- 用具体路径暂存本次改动。工作区里有与本次无关的改动（包括用户自己的改动）时，先问用户是否一起提交。
- 看一遍暂存区的文件列表，确认没有密钥、凭证与构建产物。

## 2. 自查并运行检查

- 对照 [docs/defensive-patterns.md](../../../docs/defensive-patterns.md) 看本次的子进程、流与清理代码；对照 [docs/testing.md](../../../docs/testing.md) 看测试是否够、能否失败。
- 改动较大时，交给一个全新上下文的 subagent 评审 diff：只列会阻止合并的问题，每条给出文件与行号、错在哪、怎样证明它会失败。核对它给的证据后再采纳。
- 运行 `pnpm check`。用户说“只做语法检查”时，只运行 `pnpm lint` 与 `pnpm typecheck`，并在汇报中写明没有跑测试。
- 检查失败时不提交，修好后重新运行。
- 改了模型可见的文本：确认快照的 diff 已经逐行看过。改了 Engine 调用：问用户是否运行一次真实模型测试。改了旧版 Rust 代码：另外运行 [docs/legacy-rust.md](../../../docs/legacy-rust.md) 中的检查。

## 3. 写提交说明

```text
<type>(<scope>): <中文描述>

- <模块>：<改了什么，为什么>。长句换行后缩进两格，
  与上一行的正文对齐。
```

- type 用 `feat`、`fix`、`refactor`、`docs`、`test`、`chore`；scope 写包或区域，例如 `server`、`computer`、`desktop`、`notes`。
- 正文按模块分条，写改了什么与为什么，不逐行复述 diff。范例：`git show -s d93f71c`。只改一处文字时可以只写标题。
- 多行说明写进临时文件，用 `git commit -F <文件>` 提交。

## 4. 汇报

- 提交的哈希与标题。
- 实际运行过的命令与结果；没有运行的检查写明没有运行。
- 提交后 `git status` 是否干净。
