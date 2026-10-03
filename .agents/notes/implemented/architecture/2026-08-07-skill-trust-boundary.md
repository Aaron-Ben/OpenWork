# Agent Note: Skill 信任边界

Status: implemented

## 问题

装一个 skill 等于装一段软件：它给模型指令与可执行脚本。一行没有特殊字符的描述，就能诱导模型去读别的文件。字符串净化挡不住它。要决定 OpenWork 在哪里设边界。

## 决策

- 字符串层不做安全过滤。归一化只保证目录格式。
- skill 根是硬保护路径：文件工具与 bash 在任何模式、任何越界下都不能写它（`SandboxEnvironment::hard_protected_roots`，`crates/openwork-sandbox/src/policy.rs`）。分档理由见 [Agent Note：路径四档](2026-08-01-path-tiers-and-credential-read-deny.md)。
- skill 正文没有权限效力。脚本只经 `bash` 运行，受当前模式的沙箱约束。
- 用户看得见装了什么：列表显示名称、描述、启用状态与加载失败，详情显示路径与正文。
- 文案不声称做过安全审查。停用写成“不会向模型展示”。

行为见 [skills.md §5](../../../../docs/subsystems/skills.md)。

## 考虑过的方案

**Skill 级审批卡片。** 没有采用。正文本身没有副作用；bash 与写文件已各有卡片。“是否允许使用 commit skill？”没有信息量，只会让用户习惯不看就允许。

**允许模型创建或修改 skill。** 没有采用：改 skill 能把一次提示注入变成跨 Session 的持久提权。以后若开放，要用独立的 `skill_write` 工具与专门的卡片，不放开写保护。

**支持 `allowed-tools`、`model` / `effort`、`hooks` 与 MCP 依赖。** 没有采用。`allowed-tools` 要在 Turn 中途收窄已 finalize 的工具集。模型总由用户显式选择。`hooks` 等于让 skill 注册任意执行点。OpenWork 没有 MCP。

**远程安装与市场。** 没有采用：没有来源可信度模型。

## 后果

- 模型不能帮用户创建 skill。
- 停用不是访问控制，已知路径仍可读。
