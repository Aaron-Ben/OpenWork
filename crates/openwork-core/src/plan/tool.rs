//! `update_plan` 的工具定义、参数解析与成功输出。
//!
//! 工具由 Core 拥有而不是注册进 `openwork-tools`：它修改的是 SessionActor 所有的 Turn
//! 状态，而非工作区。普通 `ToolCallContext` 不知道当前 Turn，为它扩充所有工具的 Context
//! 会制造只服务一个工具的 Data Clump。

use openwork_models::model::ToolDefinition as ModelToolDefinition;
use serde_json::{Value, json};

use super::{MAX_PLAN_STEPS, UpdatePlanArgs};

pub const UPDATE_PLAN_TOOL_NAME: &str = "update_plan";

/// Tool Result 的成功输出。
///
/// 固定为一句话：计划本体已经在 Assistant Tool Call 的参数里，Tool Result 再复制一遍
/// 只会让同一份内容在 Conversation 中出现两次并可能漂移。
pub const UPDATE_PLAN_SUCCESS_OUTPUT: &str = "Plan updated";

pub fn update_plan_success_output() -> &'static str {
    UPDATE_PLAN_SUCCESS_OUTPUT
}

/// 追加到系统提示的计划使用规则。
///
/// 仅提供工具定义不足以得到稳定行为：模型要么不用，要么给"回答用户"这种无信息步骤凑数。
///
/// 这段文本与工具是否出现在 `definitions` 中同生共死——广告了就必须有规则，没广告就不能
/// 有，否则提示里会讲一个不存在的工具。两者由同一个 `update_plan_enabled` 开关控制。
///
/// Prompt 负责行为引导，Core 校验负责数据安全。写了这段**不能**因此删掉运行时不变量：
/// 提示会被忽略，校验不会。
pub const UPDATE_PLAN_PROMPT_RULES: &str = "\
## Task checklists

You have an `update_plan` tool for tracking multi-step work.

- Skip it for simple, single-step tasks. An unnecessary checklist is noise.
- For complex, multi-stage work, or work that needs repeated verification, create a plan first.
- Mark a step `in_progress` before you start it, and `completed` as soon as it is done. \
Do not batch all the updates until the end.
- At most one step may be `in_progress` at a time during normal execution.
- Before you finish the task, mark every step `completed`.
- Do not pad the plan with contentless steps like \"read the code\" or \"answer the user\".
- After a successful call, just keep working. Do not paste the plan back into the chat.";

fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "explanation": {
                "type": "string",
                "description": "Optional one-line note about why the plan changed."
            },
            "plan": {
                "type": "array",
                "description":
                    "The complete checklist. Each call replaces the entire plan, so always \
                     send every step, not just the ones that changed.",
                "maxItems": MAX_PLAN_STEPS,
                "items": {
                    "type": "object",
                    "properties": {
                        "step": {
                            "type": "string",
                            "description": "What this step accomplishes."
                        },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"]
                        }
                    },
                    "required": ["step", "status"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["plan"],
        "additionalProperties": false
    })
}

pub fn update_plan_definition() -> ModelToolDefinition {
    ModelToolDefinition {
        name: UPDATE_PLAN_TOOL_NAME.to_string(),
        description: "Maintain a checklist for the task you are working on right now.\n\n\
             Use it for multi-step work that benefits from tracking; skip it for simple, \
             single-step requests. Each call replaces the whole plan, so send the full list \
             every time. Mark a step in_progress before starting it and completed as soon as \
             it is done — at most one step may be in_progress at a time. Before finishing the \
             task, mark every step completed."
            .to_string(),
        parameters: input_schema(),
    }
}

/// 解析工具参数。
///
/// 失败信息直接回给模型，所以要说清是哪里不合法，让它能自行改正后重试。
pub fn parse_update_plan_arguments(arguments: &Value) -> Result<UpdatePlanArgs, String> {
    serde_json::from_value::<UpdatePlanArgs>(arguments.clone())
        .map_err(|error| format!("failed to parse update_plan arguments: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlanStepStatus;

    #[test]
    fn advertises_the_three_statuses_and_the_single_in_progress_rule() {
        let definition = update_plan_definition();

        assert_eq!(definition.name, UPDATE_PLAN_TOOL_NAME);
        assert!(
            definition.description.contains("one step may be in_progress"),
            "the model needs the cross-element rule in prose; JSON Schema cannot express it"
        );
        assert!(definition.description.contains("replaces the whole plan"));

        let statuses = &definition.parameters["properties"]["plan"]["items"]["properties"]
            ["status"]["enum"];
        assert_eq!(
            statuses,
            &json!(["pending", "in_progress", "completed"]),
            "schema statuses must match the wire form of PlanStepStatus"
        );
    }

    #[test]
    fn schema_rejects_extra_fields_and_requires_plan() {
        let schema = update_plan_definition().parameters;

        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["plan"]));
        assert_eq!(
            schema["properties"]["plan"]["items"]["additionalProperties"],
            json!(false)
        );
        assert_eq!(
            schema["properties"]["plan"]["items"]["required"],
            json!(["step", "status"])
        );
    }

    #[test]
    fn parses_a_well_formed_call() {
        let args = parse_update_plan_arguments(&json!({
            "explanation": "starting",
            "plan": [
                { "step": "read schema", "status": "completed" },
                { "step": "add migration", "status": "in_progress" }
            ]
        }))
        .expect("parse");

        assert_eq!(args.explanation.as_deref(), Some("starting"));
        assert_eq!(args.plan.len(), 2);
        assert_eq!(args.plan[1].status, PlanStepStatus::InProgress);
    }

    #[test]
    fn parse_failures_explain_themselves_to_the_model() {
        let error = parse_update_plan_arguments(&json!({ "explanation": "no plan" }))
            .expect_err("missing plan must fail");

        assert!(error.contains("update_plan"), "got: {error}");
        assert!(error.contains("plan"), "got: {error}");
    }

    #[test]
    fn an_empty_plan_parses_as_an_explicit_clear() {
        let args = parse_update_plan_arguments(&json!({ "plan": [] })).expect("parse");

        assert!(args.plan.is_empty());
        assert_eq!(args.explanation, None);
    }
}
