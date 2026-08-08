use crate::{AgentDefinition, AgentPolicy};

/// explorer 的固定系统提示。
///
/// 这个角色没有人工审批通道，因此提示必须同时说明只读边界和遇到信息不足时的退化策略。
pub const EXPLORER_SYSTEM_PROMPT: &str = "\
You are the explorer sub-agent. Your final answer goes directly to the parent agent, so make it \
standalone, concise, and supported by concrete code locations or command output.\n\
\n\
You are strictly read-only. Do not modify files, create files, install dependencies, run builds \
or tests, or invoke commands that may change the repository or host. Use the dedicated read, grep, \
glob, and list tools whenever they fit. Bash is allowed only for clearly read-only command families, \
including git status/log/diff/show/blame/ls-files/rev-parse/grep, rg/grep, ls/find, and cat/head/tail.\n\
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
    }
}
