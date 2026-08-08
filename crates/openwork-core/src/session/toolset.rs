//! Turn 执行工具面：普通能力工具 + Core 拥有的控制工具。
//!
//! `openwork-tools` 继续拥有文件、进程等普通工具；`update_plan` 归 Core，因为它改的是
//! SessionActor 所有的 Turn 状态而不是工作区。依赖方向不能反转——工具 crate 不该知道
//! SessionActor 的存在。

use std::sync::Arc;

use openwork_models::model::ToolDefinition as ModelToolDefinition;
use openwork_tools::{FinalizedToolset, ToolId};
use thiserror::Error;

use crate::agent::{AGENT_TOOLS, AgentTool, agent_prompt_rules, agent_tool_definitions};
use crate::plan::{UPDATE_PLAN_PROMPT_RULES, UPDATE_PLAN_TOOL_NAME, update_plan_definition};

/// 一次调用解析出的工具身份。
///
/// 解析只发生一次，结果贯穿校验、权限、执行三个阶段——现有 Runner 会分别调用
/// `validate` / `authorize` / `call`，若每一步都用字符串再查一遍表，就等于把
/// "什么是控制工具"这个判断复制了三份，迟早有一份忘了改。
///
/// 刻意返回**拥有**的 `ToolId` 而不是借用定义：Runner 在解析之后还要调用
/// `&mut self` 的方法（doom loop 计数、append），持有一个指向 `self.request.tools`
/// 的借用会和它们冲突。ToolId 是个字符串 newtype，每次调用克隆一次可以忽略不计。
pub(super) enum ResolvedTurnTool {
    UpdatePlan,
    Agent(AgentTool),
    Registered(ToolId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlToolSurface {
    Root { max_active_sub_agent_turns: usize },
    SubAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TurnToolsetError {
    #[error(
        "control tool `{0}` collides with a registered tool; \
         rename the registered tool or stop advertising the control tool"
    )]
    NameCollision(String),
}

pub struct TurnToolset {
    definitions: Vec<ModelToolDefinition>,
    tools: Arc<FinalizedToolset>,
    /// Default mode 下为 true。Plan mode 落地后由 collaboration mode 决定工具面，
    /// 而不是在 handler 里补一个兼容拒绝分支。
    update_plan_enabled: bool,
    max_active_sub_agent_turns: Option<usize>,
}

impl TurnToolset {
    pub fn new(
        tools: Arc<FinalizedToolset>,
        control_surface: ControlToolSurface,
    ) -> Result<Self, TurnToolsetError> {
        let mut definitions = tools.definitions().to_vec();
        let max_active_sub_agent_turns = match control_surface {
            ControlToolSurface::Root {
                max_active_sub_agent_turns,
            } => Some(max_active_sub_agent_turns),
            ControlToolSurface::SubAgent => None,
        };
        let mut control_definitions = Vec::new();
        if let Some(max_active_sub_agent_turns) = max_active_sub_agent_turns {
            definitions.push(update_plan_definition());
            control_definitions.extend(agent_tool_definitions(max_active_sub_agent_turns));
        }
        for control in &control_definitions {
            if definitions
                .iter()
                .any(|definition| definition.name == control.name)
            {
                return Err(TurnToolsetError::NameCollision(control.name.clone()));
            }
        }
        if max_active_sub_agent_turns.is_some()
            && tools
                .definitions()
                .iter()
                .any(|definition| definition.name == UPDATE_PLAN_TOOL_NAME)
        {
            return Err(TurnToolsetError::NameCollision(
                UPDATE_PLAN_TOOL_NAME.to_string(),
            ));
        }
        definitions.extend(control_definitions);
        Ok(Self {
            definitions,
            tools,
            update_plan_enabled: max_active_sub_agent_turns.is_some(),
            max_active_sub_agent_turns,
        })
    }

    pub fn definitions(&self) -> &[ModelToolDefinition] {
        &self.definitions
    }

    pub(super) fn registered(&self) -> &Arc<FinalizedToolset> {
        &self.tools
    }

    pub fn update_plan_enabled(&self) -> bool {
        self.update_plan_enabled
    }

    pub fn max_active_sub_agent_turns(&self) -> Option<usize> {
        self.max_active_sub_agent_turns
    }

    /// 将控制工具规则和工具定义作为同一个工具面的两个投影统一组装。
    ///
    /// 运行 Turn 和只读上下文检查必须走这里；否则很容易出现模型实际拿到一套工具，
    /// 检查器却展示另一套提示词的情况。
    pub(crate) fn system_prompt(&self, base: &str) -> String {
        let mut prompt = base.to_string();
        if self.update_plan_enabled {
            prompt.push_str("\n\n");
            prompt.push_str(UPDATE_PLAN_PROMPT_RULES);
        }
        if let Some(max) = self.max_active_sub_agent_turns {
            prompt.push_str("\n\n");
            prompt.push_str(&agent_prompt_rules(max));
        }
        prompt
    }

    /// 解析工具名。
    ///
    /// 返回 `None` 表示未知工具，由调用方生成失败 Tool Result——与既有行为一致。
    pub(super) fn resolve(&self, name: &str) -> Option<ResolvedTurnTool> {
        if self.update_plan_enabled && name == UPDATE_PLAN_TOOL_NAME {
            return Some(ResolvedTurnTool::UpdatePlan);
        }
        if self.max_active_sub_agent_turns.is_some()
            && let Some(tool) = AGENT_TOOLS.into_iter().find(|tool| tool.name() == name)
        {
            return Some(ResolvedTurnTool::Agent(tool));
        }
        self.tools
            .resolve(name)
            .map(|definition| ResolvedTurnTool::Registered(definition.id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_tools::{PermissionProfile, ToolSessionContext, ToolsetConfig, builtin_registry};

    fn toolset(names: &[&str]) -> Arc<FinalizedToolset> {
        Arc::new(
            builtin_registry()
                .finalize(
                    &ToolsetConfig::from_names(names.iter().map(|name| name.to_string())),
                    ToolSessionContext::local(
                        std::env::temp_dir(),
                        PermissionProfile::from_builtin_rules(std::env::temp_dir()),
                    ),
                )
                .expect("finalize"),
        )
    }

    #[test]
    fn advertises_update_plan_exactly_once_and_resolves_it() {
        let turn_tools = TurnToolset::new(
            toolset(&["read", "bash"]),
            ControlToolSurface::Root {
                max_active_sub_agent_turns: 3,
            },
        )
        .expect("toolset");

        let advertised = turn_tools
            .definitions()
            .iter()
            .filter(|definition| definition.name == UPDATE_PLAN_TOOL_NAME)
            .count();
        assert_eq!(advertised, 1);
        assert_eq!(
            turn_tools.definitions().len(),
            8,
            "the control tool is added on top of the registered surface, not instead of it"
        );
        assert!(matches!(
            turn_tools.resolve(UPDATE_PLAN_TOOL_NAME),
            Some(ResolvedTurnTool::UpdatePlan)
        ));
        assert!(matches!(
            turn_tools.resolve("read"),
            Some(ResolvedTurnTool::Registered(_))
        ));
        for name in [
            "spawn_agent",
            "wait_agent",
            "list_agents",
            "followup_task",
            "interrupt_agent",
        ] {
            assert!(matches!(
                turn_tools.resolve(name),
                Some(ResolvedTurnTool::Agent(_))
            ));
        }
        assert!(turn_tools.resolve("nope").is_none());
    }

    #[test]
    fn a_disabled_control_tool_is_neither_advertised_nor_dispatched() {
        let turn_tools =
            TurnToolset::new(toolset(&["read"]), ControlToolSurface::SubAgent).expect("toolset");

        assert!(
            !turn_tools
                .definitions()
                .iter()
                .any(|definition| definition.name == UPDATE_PLAN_TOOL_NAME)
        );
        assert!(
            turn_tools.resolve(UPDATE_PLAN_TOOL_NAME).is_none(),
            "not advertising it must also mean not dispatching it"
        );
        for name in [
            "spawn_agent",
            "wait_agent",
            "list_agents",
            "followup_task",
            "interrupt_agent",
        ] {
            assert!(
                turn_tools.resolve(name).is_none(),
                "sub-agents must not receive collaboration controls: {name}"
            );
        }
    }

    /// 一个恰好占用了 `update_plan` 这个名字的普通工具。
    struct CollidingTool;

    struct SpawnCollidingTool;

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct NoInput {}

    #[async_trait::async_trait]
    impl openwork_tools::Tool for CollidingTool {
        type Input = NoInput;
        type Output = openwork_tools::TextToolOutput;

        fn id(&self) -> openwork_tools::ToolId {
            openwork_tools::ToolId::new(UPDATE_PLAN_TOOL_NAME)
        }

        fn description(&self) -> &'static str {
            "a registered tool that squats on the control tool's name"
        }

        fn risk(&self) -> openwork_tools::ToolRisk {
            openwork_tools::ToolRisk::ReadOnly
        }

        async fn execute(
            &self,
            _session: &ToolSessionContext,
            _call: openwork_tools::ToolCallContext,
            _input: Self::Input,
        ) -> Result<Self::Output, openwork_tools::ToolExecutionError> {
            Ok(openwork_tools::TextToolOutput::new("never runs"))
        }
    }

    #[async_trait::async_trait]
    impl openwork_tools::Tool for SpawnCollidingTool {
        type Input = NoInput;
        type Output = openwork_tools::TextToolOutput;

        fn id(&self) -> openwork_tools::ToolId {
            openwork_tools::ToolId::new("spawn_agent")
        }

        fn description(&self) -> &'static str {
            "a registered tool that squats on a collaboration control name"
        }

        fn risk(&self) -> openwork_tools::ToolRisk {
            openwork_tools::ToolRisk::ReadOnly
        }

        async fn execute(
            &self,
            _session: &ToolSessionContext,
            _call: openwork_tools::ToolCallContext,
            _input: Self::Input,
        ) -> Result<Self::Output, openwork_tools::ToolExecutionError> {
            Ok(openwork_tools::TextToolOutput::new("never runs"))
        }
    }

    #[test]
    fn a_registered_tool_with_the_same_name_fails_at_construction() {
        let colliding = Arc::new(
            builtin_registry()
                .register(CollidingTool)
                .finalize(
                    &ToolsetConfig::from_names([UPDATE_PLAN_TOOL_NAME]),
                    ToolSessionContext::local(
                        std::env::temp_dir(),
                        PermissionProfile::from_builtin_rules(std::env::temp_dir()),
                    ),
                )
                .expect("finalize"),
        );

        let error = match TurnToolset::new(
            colliding,
            ControlToolSurface::Root {
                max_active_sub_agent_turns: 3,
            },
        ) {
            Err(error) => error,
            Ok(_) => panic!("a collision must fail while building the surface, not at dispatch"),
        };

        assert_eq!(
            error,
            TurnToolsetError::NameCollision(UPDATE_PLAN_TOOL_NAME.to_string())
        );
    }

    #[test]
    fn every_advertised_name_resolves() {
        let turn_tools = TurnToolset::new(
            toolset(&["read", "write", "bash"]),
            ControlToolSurface::Root {
                max_active_sub_agent_turns: 3,
            },
        )
        .expect("ok");

        for definition in turn_tools.definitions() {
            assert!(
                turn_tools.resolve(&definition.name).is_some(),
                "advertised but not dispatchable: {}",
                definition.name
            );
        }
    }

    #[test]
    fn every_collaboration_control_name_participates_in_collision_detection() {
        let colliding = Arc::new(
            builtin_registry()
                .register(SpawnCollidingTool)
                .finalize(
                    &ToolsetConfig::from_names(["spawn_agent"]),
                    ToolSessionContext::local(
                        std::env::temp_dir(),
                        PermissionProfile::from_builtin_rules(std::env::temp_dir()),
                    ),
                )
                .expect("finalize"),
        );

        let error = TurnToolset::new(
            colliding,
            ControlToolSurface::Root {
                max_active_sub_agent_turns: 3,
            },
        )
        .err()
        .expect("collision");
        assert_eq!(
            error,
            TurnToolsetError::NameCollision("spawn_agent".to_string())
        );
    }

    #[test]
    fn collaboration_descriptions_explain_when_not_to_delegate_and_the_limit() {
        let turn_tools = TurnToolset::new(
            toolset(&["read"]),
            ControlToolSurface::Root {
                max_active_sub_agent_turns: 3,
            },
        )
        .expect("toolset");
        let spawn = turn_tools
            .definitions()
            .iter()
            .find(|definition| definition.name == "spawn_agent")
            .expect("spawn definition");

        assert!(spawn.description.contains("specific, bounded"));
        assert!(spawn.description.contains("Do not use"));
        assert!(spawn.description.contains("intermediate material"));
        assert!(spawn.description.contains("next action depends"));
        assert!(spawn.description.contains("already investigated"));
        assert!(spawn.description.contains("same unresolved question"));
        assert!(spawn.description.contains("3"));
    }
}
