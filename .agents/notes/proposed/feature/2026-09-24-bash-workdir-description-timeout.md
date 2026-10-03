# Agent Note: bash 的 workdir、description 与超时

Status: proposed

## 问题

bash 现在只有 `command` 与 `timeoutMs` 两个参数（`crates/openwork-tools/src/builtins/process/bash.rs`），见 [tools.md §7.9](../../../../docs/subsystems/tools.md)。

- 命令总在工作区根运行。模型要在子目录运行时，只能在命令前写 `cd`，命令原文因此变长。
- 卡片与工具列表只能显示命令原文。一长串命令比“运行测试”难懂。
- 超时默认 30 秒，最大 120 秒。大项目的 `cargo build` 常常超过 2 分钟，会直接以超时失败。

## 提议

| 参数 | 默认 | 说明 |
|---|---|---|
| `workdir` | 工作区根 | 本次命令的工作目录。相对路径以工作区根解析，并经文件工具围栏检查 |
| `description` | 必填 | 5–10 个词的动作描述，只给界面显示，不进入模型可见的结果 |
| `timeoutMs` | 120,000 | 上限 600,000（10 分钟） |

- 每次调用仍是新的 shell，不保留 `cd`。
- 工具描述告诉模型用 `workdir`，不要用 `cd`。

DSH 的 bash 有 `description` 与 `workdir` 两个参数，工具描述写明“pass `workdir` instead of using `cd`”（`packages/shell/tool-bash/src/index.ts`）。

## 考虑过的方案

**界面继续显示命令原文。** 这是现状。没有采用：卡片上的长命令难懂。命令原文仍在卡片上完整显示（[permissions.md §12](../../../../docs/subsystems/permissions.md)），`description` 只做标题。

## 验收条件

- tools.md 验收一节原第 35 条：`workdir` 相对工作区根解析，必须通过沙箱策略。
- 原第 36 条：`timeoutMs` 上限 600,000。
- 原第 37 条：`description` 显示在界面上，不进入模型可见的结果。

## 风险

- 10 分钟的超时让一次失控的命令占住 Turn 更久。用户可以取消 Turn。
- `description` 由模型填写，可能与命令实际做的事不符。危险命令与越界卡片必须继续显示完整的命令原文。
- 新增必填参数会让旧的调用写法失败。按本仓库规则，不保留兼容层。
