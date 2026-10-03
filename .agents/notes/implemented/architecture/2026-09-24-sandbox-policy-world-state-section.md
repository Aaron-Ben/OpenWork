# Agent Note: 沙箱策略作为最后一个 world state section

Status: implemented

## 问题

模型需要知道三件事：当前模式、工作区根、bash 能写什么。最直接的位置是系统提示词。但模式可以在会话中途切换。写进系统前缀，每次切换都会作废提示缓存。

world state 已经有三个 section，每个 section 是一条单独的消息。新 section 放在哪里，会影响已有消息的位置与字节。

## 决策

- `SandboxPolicyState`（`crates/openwork-core/src/context/world_state/sandbox_policy.rs`）的 ID 是 `runtime/sandbox-policy`。正文是 `<sandbox_policy>` 包裹的四行：`mode`、`workspace`、`write / edit`、`bash`。
- bash 一行有三种写法：能写工作区与临时目录；只能写临时目录，写工作区要 `sandboxPermissions`；沙箱不可用时的 `BASH_UNAVAILABLE` 说明。前两种由 `SandboxPolicy::bash_writes_workspace` 决定。
- run loop 每次采样时，用会话模式、不带越界的策略渲染它（`sandbox_policy_state`）。模式没变就不发。模式变了，就按 diff 机制追加一条带取代声明的新快照，不改写历史。
- `render_diff` 的顺序固定为 project_context → agents_md → skills_catalog → sandbox_policy（`crates/openwork-core/src/context/world_state/mod.rs`）。新 section 排在最后，前三个 section 的消息字节不变。
- 正文只陈述事实，不提网络，也不说“只读”。

设计见 [permissions.md §11](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**把沙箱模式写进系统提示词。** 没有采用，理由有两条。第一，模式切换会改写系统前缀，作废缓存。DSH 也把策略放在上下文消息里，系统提示词跨模式逐字节不变（`packages/sandbox/sandbox-policy/README.md`）。第二，原设计文档记载了 DSH 的一次人工测试。系统提示词写“bash 运行在只读沙箱中”时，12 个回合里有 5 个以零工具调用结束。这组数字没有在 DSH 源码里找到（未确认）。

## 后果

- 模式切换只追加一条消息。系统前缀与已有消息都不变。
- 会随模式变化的内容排在稳定内容之后，所以模式不影响前三个 section 的字节。
- 模型只看到事实陈述。边界由工具结果里的拒绝标记在相关时刻指出。
- 主目录工作区下，`auto` 的 bash 一行也写“写工作区需要 sandboxPermissions”，与内核行为一致。
- 压缩或 rewind 换掉旧消息后，`RetainedSections::scan` 发现这条消息已不在投影里，于是重发。
- 这个 section 在会话里始终存在，它的移除声明实际发不出来。保留它，只是为了与另外三个 section 共用同一套状态机。
