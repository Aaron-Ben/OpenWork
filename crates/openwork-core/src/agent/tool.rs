//! Core 所有的多智能体控制工具定义与参数契约。

use openwork_models::model::ToolDefinition;
use serde::Deserialize;
use serde_json::{Value, json};

pub const SPAWN_AGENT_TOOL_NAME: &str = "spawn_agent";
pub const WAIT_AGENT_TOOL_NAME: &str = "wait_agent";
pub const LIST_AGENTS_TOOL_NAME: &str = "list_agents";
pub const FOLLOWUP_TASK_TOOL_NAME: &str = "followup_task";
pub const INTERRUPT_AGENT_TOOL_NAME: &str = "interrupt_agent";

pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 60_000;
pub const MIN_WAIT_TIMEOUT_MS: u64 = 10_000;
pub const MAX_WAIT_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTool {
    Spawn,
    Wait,
    List,
    Followup,
    Interrupt,
}

impl AgentTool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Spawn => SPAWN_AGENT_TOOL_NAME,
            Self::Wait => WAIT_AGENT_TOOL_NAME,
            Self::List => LIST_AGENTS_TOOL_NAME,
            Self::Followup => FOLLOWUP_TASK_TOOL_NAME,
            Self::Interrupt => INTERRUPT_AGENT_TOOL_NAME,
        }
    }
}

pub const AGENT_TOOLS: [AgentTool; 5] = [
    AgentTool::Spawn,
    AgentTool::Wait,
    AgentTool::List,
    AgentTool::Followup,
    AgentTool::Interrupt,
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnAgentArgs {
    pub task_name: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitAgentArgs {
    #[serde(default = "default_wait_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowupTaskArgs {
    pub task_name: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptAgentArgs {
    pub task_name: String,
}

pub fn parse_args<T: serde::de::DeserializeOwned>(
    tool: AgentTool,
    input: &Value,
) -> Result<T, String> {
    serde_json::from_value(input.clone())
        .map_err(|error| format!("failed to parse {} arguments: {error}", tool.name()))
}

pub fn validate_wait_timeout(timeout_ms: u64) -> Result<u64, String> {
    if !(MIN_WAIT_TIMEOUT_MS..=MAX_WAIT_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(format!(
            "timeout_ms must be between {MIN_WAIT_TIMEOUT_MS} and {MAX_WAIT_TIMEOUT_MS}"
        ));
    }
    Ok(timeout_ms)
}

pub fn agent_tool_definitions(max_active_turns: usize) -> Vec<ToolDefinition> {
    vec![
        definition(
            SPAWN_AGENT_TOOL_NAME,
            format!(
                "Delegate a specific, bounded, read-only codebase investigation to a new explorer \
                 and return immediately. Use it when multiple independent repository questions can \
                 run in parallel, especially when the research produces substantial intermediate \
                 material that should stay out of the parent context. Do not use it for a single \
                 small question, when the next action depends on this result, for work already \
                 investigated, or repeatedly for the same unresolved question. Do not delegate \
                 writable work or work that needs the parent's live conversation. At most \
                 {max_active_turns} explorer turns may run concurrently."
            ),
            task_message_schema(),
        ),
        definition(
            WAIT_AGENT_TOOL_NAME,
            "Wait until any explorer delivers a message or the timeout expires. Use it after \
             delegating work when no useful parent-side work remains. Do not use it when no \
             explorer is active; it returns only delivery/timeout state, never message content.",
            json!({
                "type": "object",
                "properties": {
                    "timeout_ms": {
                        "type": "integer",
                        "minimum": MIN_WAIT_TIMEOUT_MS,
                        "maximum": MAX_WAIT_TIMEOUT_MS,
                        "default": DEFAULT_WAIT_TIMEOUT_MS
                    }
                },
                "additionalProperties": false
            }),
        ),
        definition(
            LIST_AGENTS_TOOL_NAME,
            "List explorers created by this parent and their current status. Use it for a \
             point-in-time overview. Do not poll it for result delivery; use wait_agent instead.",
            empty_schema(),
        ),
        definition(
            FOLLOWUP_TASK_TOOL_NAME,
            format!(
                "Start another specific, bounded, read-only task on an existing idle explorer. \
                 Use it when prior explorer context is useful. Do not use it for an active or \
                 unknown explorer, or for an unrelated task that should get a new name. The shared \
                 concurrency limit is {max_active_turns} active explorer turns."
            ),
            task_message_schema(),
        ),
        definition(
            INTERRUPT_AGENT_TOOL_NAME,
            "Interrupt an existing explorer's active turn. Use it when its current work is no \
             longer needed. Do not use it for an idle, completed, or unknown explorer.",
            json!({
                "type": "object",
                "properties": {
                    "task_name": {
                        "type": "string",
                        "pattern": "^[a-z][a-z0-9_]{0,47}$"
                    }
                },
                "required": ["task_name"],
                "additionalProperties": false
            }),
        ),
    ]
}

pub fn agent_prompt_rules(max_active_turns: usize) -> String {
    format!(
        "## Sub-agent delegation\n\n\
         You can run up to {max_active_turns} read-only explorer turns concurrently. Delegate when \
         you have multiple specific, bounded codebase questions that can proceed independently, \
         especially when their intermediate research should stay out of the parent context. Keep \
         doing useful parent-side work while explorers run. Handle a single small question locally. \
         Keep critical-path research local when your next action depends on its result. Do not \
         delegate work you already investigated or repeatedly delegate the same unresolved question. \
         Use wait_agent only when waiting is the next useful action; delivered messages appear \
         before the next model call. Reuse an idle explorer with followup_task when its existing \
         context helps."
    )
}

fn definition(name: &str, description: impl Into<String>, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.into(),
        parameters,
    }
}

fn task_message_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_name": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9_]{0,47}$"
            },
            "message": { "type": "string", "minLength": 1 }
        },
        "required": ["task_name", "message"],
        "additionalProperties": false
    })
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

const fn default_wait_timeout_ms() -> u64 {
    DEFAULT_WAIT_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_timeout_defaults_to_one_minute_and_stays_inside_the_documented_bounds() {
        let default =
            parse_args::<WaitAgentArgs>(AgentTool::Wait, &json!({})).expect("default timeout");
        assert_eq!(default.timeout_ms, DEFAULT_WAIT_TIMEOUT_MS);
        assert_eq!(validate_wait_timeout(MIN_WAIT_TIMEOUT_MS), Ok(10_000));
        assert_eq!(validate_wait_timeout(MAX_WAIT_TIMEOUT_MS), Ok(600_000));
        assert!(validate_wait_timeout(MIN_WAIT_TIMEOUT_MS - 1).is_err());
        assert!(validate_wait_timeout(MAX_WAIT_TIMEOUT_MS + 1).is_err());
    }

    #[test]
    fn every_definition_has_a_closed_object_schema() {
        for definition in agent_tool_definitions(3) {
            assert_eq!(
                definition.parameters["additionalProperties"],
                serde_json::Value::Bool(false),
                "{} accepts undeclared fields",
                definition.name
            );
        }
    }
}
