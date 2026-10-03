# Agent Note: 子 Agent 只用于上下文隔离：只读、单层、复用 Session

Status: implemented

## 问题

主 Agent 回答一个代码库问题，常常要读二十个文件。这些中间材料留在主对话里，占满上下文，也会提前触发压缩。主对话需要的只是结论。

多智能体还有别的用途，例如并行写代码与交叉验证。这两件事需要权限冒泡、写冲突规则与失败回滚，工程量高一个量级。

## 决策

- 子 Agent 只做一件事：在主对话之外完成只读调查，再把结论交回。
- 唯一角色 `explorer` 写死在 `crates/openwork-agent/src/explorer.rs`，没有角色文件，也没有角色选择参数。
- 子 Agent 是完整的 Session，复用 `SessionActor` 与唯一的 run loop（`OpenWorkCore::build_session_handle`，`crates/openwork-core/src/core.rs`）。
- 深度上限 1。`AgentControl::spawn`（`crates/openwork-core/src/agent/control.rs`）总以根会话为父；`create_sub_agent_session` 拒绝以子 Agent 为父。
- 子 Agent 取派生时父会话的模型，`spawn_agent` 没有模型参数。
- 创建、寻址与限额归 `AgentControl`，不归父 `SessionActor`。`AgentControl` 以 `Weak` 持有 Core，打断引用环。

规则见 [multi-agent.md §1–§4](../../../../docs/subsystems/multi-agent.md)。

## 考虑过的方案

**可写的 worker 角色。** 需要权限冒泡、写集合划分与失败回滚。没有采用：上下文隔离不需要写权限。

**多层嵌套。** 对上下文隔离没有收益，还要路径解析、兄弟寻址与级联关闭。没有采用。Codex 的默认深度也是 1，但可以配置（`codex-rs/core/src/config/mod.rs` 的 `DEFAULT_AGENT_MAX_DEPTH`）。

**从 `~/.openwork/agents/*.md` 加载角色。** 只有一个角色时，这等于把 Skill 系统再做一遍：目录发现、frontmatter、启停状态与管理界面。没有采用。Codex 内置的 `codex-rs/core/assets/agent/builtins/explorer.toml` 是 0 字节文件，起作用的是描述文本。

**用户用 `@explorer` 显式触发。** 只有一个角色时，它只表达“用或不用”，而工具描述已经回答这个问题。没有采用。

**由角色或模型参数选择子 Agent 的模型。** 与“模型总由用户显式选择”冲突（[architecture.md](../../../../docs/architecture.md) §3 第 4 条）。没有采用。

**父 `SessionActor` 自己管理子 Session。** 它要再承担创建、注册表、限额与寻址四份状态。没有采用。

**独立的 `openwork-subagent` crate。** 只有 Core 一个消费者，拆 crate 是投机抽象。没有采用。

**fork 父历史，复用 prompt cache。** 收益真实，但隔离不需要它。子 Agent 从空历史与一条任务开始。

## 后果

- 子 Agent 与根会话共用存储、压缩、Trace 与恢复路径，没有第二套循环。
- 子 Agent 不能并行写代码。需要写的任务仍由主 Agent 自己做。
- 增加第二个角色时，要改三处写死 `explorer` 的代码：`AgentControl::spawn` 的 `agent_role`、`SandboxRuntime::build_explorer_agent_and_tools` 与 `sub_agent_mode`。
- 用户界面不提供给子 Agent 发消息的入口，但 Core 的 `start_turn` 不拒绝子 Session。
