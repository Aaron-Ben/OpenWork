//! Turn 级任务清单（`update_plan`）的领域类型与校验。
//!
//! 这里回答的是"这次任务现在做到哪一步"，不是"先与用户讨论出一份方案"。后者属于未来的
//! Plan mode，两者不共享状态机。见 `docs/update-plan.md`。
//!
//! 校验必须发生在 Core 运行时，不能只靠 Tool description 里的自然语言约束——"最多一个
//! `in_progress`"是跨数组元素的条件，JSON Schema 表达不了，而模型提示不是数据完整性边界。

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::PrimitiveDateTime;

use crate::session::TurnId;

mod projection;
mod tool;

pub use projection::{PlanStateContributor, TURN_PLAN_STATE_KEY, TurnPlanRecord, TurnPlanSnapshot};
pub use tool::{
    UPDATE_PLAN_PROMPT_RULES, UPDATE_PLAN_TOOL_NAME, parse_update_plan_arguments,
    update_plan_definition, update_plan_success_output,
};

/// 步骤数量上限。
///
/// 计划会被投影进压缩 reminder，而 reminder 有 `MAX_REMINDER_CHARS` 硬上限。不设界的
/// 计划可以让压缩整体失败，进而拖垮整个 Turn——那是比"拒绝一份超长计划"糟糕得多的
/// 失败模式。上限与 `FileChangeStateContributor` 的 `MAX_EDITED_PATHS` 取同一量级。
pub const MAX_PLAN_STEPS: usize = 128;

/// 单个步骤的字符上限，理由同 [`MAX_PLAN_STEPS`]。
pub const MAX_STEP_CHARS: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStepStatus {
    /// 与工具参数一致的 wire 形式。Desktop 直接消费这个字符串，不再维护第二套状态映射。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub step: String,
    pub status: PlanStepStatus,
}

/// `update_plan` 的原始参数。一次调用替换整个计划，不做局部 patch。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlanArgs {
    #[serde(default)]
    pub explanation: Option<String>,
    pub plan: Vec<PlanStep>,
}

/// 一个 Turn 的当前计划。每次成功调用都完整替换它。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPlan {
    pub turn_id: TurnId,
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub updated_at: PrimitiveDateTime,
}

impl TurnPlan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == PlanStepStatus::Completed)
            .count()
    }

    /// Turn 收尾时还没标成 `completed` 的步骤数。
    ///
    /// 这**不是**不变量：Turn 可能因错误、取消或达到调用上限而终止，那时留下未完成步骤
    /// 是正确的记录，强制全部 completed 等于要求一个崩溃的 Turn 谎报自己干完了。
    ///
    /// 但它必须可观测，否则 Prompt 里"结束前把所有步骤置为 completed"这条规则是否生效
    /// 永远无法证伪 —— 只能靠人翻聊天记录。见 `docs/update-plan.md` §15.1。
    pub fn unfinished_step_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status != PlanStepStatus::Completed)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlanValidationError {
    #[error("plan step {index} must not be blank")]
    BlankStep { index: usize },
    #[error("plan step {index} exceeds {MAX_STEP_CHARS} characters")]
    StepTooLong { index: usize },
    #[error("plan must not exceed {MAX_PLAN_STEPS} steps")]
    TooManySteps,
    #[error(
        "at most one step may be in_progress, but steps {first} and {second} are both in_progress"
    )]
    MultipleInProgress { first: usize, second: usize },
}

/// 校验模型提交的步骤列表。
///
/// 返回新的 `Vec`，不改写入参：违反不变量的调用必须整体失败且不留下任何痕迹。
pub fn validate_steps(steps: &[PlanStep]) -> Result<Vec<PlanStep>, PlanValidationError> {
    if steps.len() > MAX_PLAN_STEPS {
        return Err(PlanValidationError::TooManySteps);
    }

    let mut first_in_progress: Option<usize> = None;
    for (index, step) in steps.iter().enumerate() {
        if step.step.trim().is_empty() {
            return Err(PlanValidationError::BlankStep { index });
        }
        if step.step.chars().count() > MAX_STEP_CHARS {
            return Err(PlanValidationError::StepTooLong { index });
        }
        if step.status == PlanStepStatus::InProgress {
            if let Some(first) = first_in_progress {
                return Err(PlanValidationError::MultipleInProgress {
                    first,
                    second: index,
                });
            }
            first_in_progress = Some(index);
        }
    }

    // 存储原始非空文本，不做 trim——步骤文本属于模型的表达，Core 只判断它不是空白。
    Ok(steps.to_vec())
}

/// 校验整组参数并物化成一个 [`TurnPlan`]。
///
/// `explanation` 未提供时保存 `None`，不沿用旧说明：每次调用都是完整快照。
pub fn validate_args(
    turn_id: &TurnId,
    args: UpdatePlanArgs,
    updated_at: PrimitiveDateTime,
) -> Result<TurnPlan, PlanValidationError> {
    let steps = validate_steps(&args.plan)?;
    Ok(TurnPlan {
        turn_id: turn_id.clone(),
        explanation: args.explanation,
        steps,
        updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn at() -> PrimitiveDateTime {
        datetime!(2026-08-07 18:30:00)
    }

    fn step(text: &str, status: PlanStepStatus) -> PlanStep {
        PlanStep {
            step: text.to_string(),
            status,
        }
    }

    #[test]
    fn round_trips_every_status_through_the_wire_form() {
        for (status, wire) in [
            (PlanStepStatus::Pending, "pending"),
            (PlanStepStatus::InProgress, "in_progress"),
            (PlanStepStatus::Completed, "completed"),
        ] {
            let encoded = serde_json::to_string(&status).expect("serialize");
            assert_eq!(encoded, format!("\"{wire}\""));
            let decoded: PlanStepStatus = serde_json::from_str(&encoded).expect("deserialize");
            assert_eq!(decoded, status);
            assert_eq!(status.as_str(), wire);
        }
    }

    #[test]
    fn rejects_missing_plan_unknown_fields_and_unknown_status() {
        assert!(serde_json::from_str::<UpdatePlanArgs>(r#"{"explanation":"x"}"#).is_err());
        assert!(
            serde_json::from_str::<UpdatePlanArgs>(r#"{"plan":[],"surprise":true}"#).is_err(),
            "unknown top-level fields must fail"
        );
        assert!(
            serde_json::from_str::<UpdatePlanArgs>(
                r#"{"plan":[{"step":"a","status":"pending","extra":1}]}"#
            )
            .is_err(),
            "unknown step fields must fail"
        );
        assert!(
            serde_json::from_str::<UpdatePlanArgs>(r#"{"plan":[{"step":"a","status":"done"}]}"#)
                .is_err(),
            "unknown status must fail"
        );
    }

    #[test]
    fn accepts_an_absent_explanation_without_inheriting_an_old_one() {
        let args: UpdatePlanArgs = serde_json::from_str(r#"{"plan":[]}"#).expect("parse");

        assert_eq!(args.explanation, None);

        let plan = validate_args(&TurnId::new("turn-1"), args, at()).expect("valid");
        assert_eq!(plan.explanation, None);
    }

    #[test]
    fn rejects_blank_steps() {
        for blank in ["", "   ", "\t\n"] {
            let steps = vec![step(blank, PlanStepStatus::Pending)];

            assert_eq!(
                validate_steps(&steps),
                Err(PlanValidationError::BlankStep { index: 0 })
            );
        }
    }

    #[test]
    fn keeps_the_original_step_text_including_surrounding_whitespace() {
        let steps = vec![step("  padded  ", PlanStepStatus::Pending)];

        let validated = validate_steps(&steps).expect("valid");

        assert_eq!(validated[0].step, "  padded  ");
    }

    #[test]
    fn rejects_two_in_progress_steps() {
        let steps = vec![
            step("a", PlanStepStatus::InProgress),
            step("b", PlanStepStatus::Pending),
            step("c", PlanStepStatus::InProgress),
        ];

        assert_eq!(
            validate_steps(&steps),
            Err(PlanValidationError::MultipleInProgress {
                first: 0,
                second: 2
            })
        );
    }

    #[test]
    fn accepts_all_completed_and_an_empty_plan() {
        let all_done = vec![
            step("a", PlanStepStatus::Completed),
            step("b", PlanStepStatus::Completed),
        ];
        assert_eq!(validate_steps(&all_done).expect("valid").len(), 2);

        assert!(validate_steps(&[]).expect("valid").is_empty());
    }

    #[test]
    fn preserves_the_submitted_order() {
        let steps = vec![
            step("zebra", PlanStepStatus::Completed),
            step("apple", PlanStepStatus::InProgress),
            step("mango", PlanStepStatus::Pending),
        ];

        let validated = validate_steps(&steps).expect("valid");

        let texts: Vec<_> = validated.iter().map(|item| item.step.as_str()).collect();
        assert_eq!(texts, ["zebra", "apple", "mango"]);
    }

    #[test]
    fn bounds_step_count_and_step_length() {
        let too_many = vec![step("a", PlanStepStatus::Pending); MAX_PLAN_STEPS + 1];
        assert_eq!(
            validate_steps(&too_many),
            Err(PlanValidationError::TooManySteps)
        );

        let too_long = vec![step(
            &"x".repeat(MAX_STEP_CHARS + 1),
            PlanStepStatus::Pending,
        )];
        assert_eq!(
            validate_steps(&too_long),
            Err(PlanValidationError::StepTooLong { index: 0 })
        );
    }

    #[test]
    fn reports_completion_and_unfinished_work() {
        let plan = TurnPlan {
            turn_id: TurnId::new("turn-1"),
            explanation: None,
            steps: vec![
                step("a", PlanStepStatus::Completed),
                step("b", PlanStepStatus::InProgress),
            ],
            updated_at: at(),
        };

        assert_eq!(plan.completed_count(), 1);
        assert_eq!(plan.unfinished_step_count(), 1);
        assert!(!plan.is_empty());

        let mixed = TurnPlan {
            steps: vec![
                step("a", PlanStepStatus::Completed),
                step("b", PlanStepStatus::InProgress),
                step("c", PlanStepStatus::Pending),
                step("d", PlanStepStatus::Pending),
            ],
            ..plan.clone()
        };
        assert_eq!(
            mixed.unfinished_step_count(),
            3,
            "in_progress 和 pending 都算没收尾"
        );

        let finished = TurnPlan {
            steps: vec![step("a", PlanStepStatus::Completed)],
            ..plan.clone()
        };
        assert_eq!(finished.unfinished_step_count(), 0);

        let cleared = TurnPlan {
            steps: Vec::new(),
            ..plan
        };
        assert!(cleared.is_empty());
        assert_eq!(
            cleared.unfinished_step_count(),
            0,
            "显式清空的计划没有欠账，与 None（从未建过计划）在调用方区分"
        );
    }
}
