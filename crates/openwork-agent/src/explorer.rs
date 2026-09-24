use openwork_sandbox::SandboxMode;

use crate::{AgentDefinition, AgentPolicy};

/// explorer 的固定系统提示。
///
/// 这个角色没有人工审批通道：提示要说明它改不了仓库、被沙箱拒绝的调用不会有人批准，
/// 以及信息不足时怎么收尾。沙箱模式本身不写进提示词，由 world state 给出（permissions.md §4.6）。
pub const EXPLORER_SYSTEM_PROMPT: &str = "\
You are the explorer sub-agent. Your final answer goes directly to the parent agent, so make it \
standalone, concise, and supported by concrete code locations or command output.\n\
\n\
Your job is to investigate, not to change the repository. You have no file-editing tools, and bash \
runs in a sandbox that does not let you write the workspace; temporary directories are writable. \
Use the dedicated read, grep, glob, and list tools whenever they fit, and bash for commands such as \
git log/diff/show/blame or running a build or test to observe its output. Nobody can approve \
anything for you: a call the sandbox denies stays denied, so work with what you can reach and \
report what you could not check.\n\
\n\
Do not ask questions. If the task is underspecified, inspect the strongest available evidence, state \
the assumption you used, and clearly label any remaining uncertainty. Return only your findings and \
do not address the end user.";

/// 返回 P2 唯一内置子 Agent 角色的不可变定义。
pub fn explorer_definition() -> AgentDefinition {
    AgentDefinition {
        name: "explorer".to_string(),
        description: "回答关于代码库的具体、范围明确的问题".to_string(),
        system_prompt: EXPLORER_SYSTEM_PROMPT.to_string(),
        tool_names: ["read", "grep", "glob", "list", "bash"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        policy: AgentPolicy {
            max_model_calls: 15,
            doom_loop_threshold: 3,
        },
        // 没有 write / edit，bash 又写不了工作区：explorer 改不了仓库（permissions.md §2.2）。
        sandbox_ceiling: SandboxMode::AcceptEdits,
    }
}
