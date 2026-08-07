//! 计划的两个出口：Session DTO 与压缩 reminder。
//!
//! reminder 一侧实现 `CompactionStateContributor`，复用 collector 已有的 key 去重、失败
//! 策略、`extensions` 持久化和长度校验，而不是另建一条投影管线。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::{
    CompactionStateCollectInput, CompactionStateContributor, CompactionStateError,
    CompactionStateFailurePolicy, ReminderSection,
};
use crate::storage::time::to_wire;

use super::{PlanStep, PlanStepStatus, TurnPlan};

pub const TURN_PLAN_STATE_KEY: &str = "turn_plan";
const TURN_PLAN_STATE_SCHEMA_VERSION: u16 = 1;

/// 活动 Turn 的计划快照，随 `SessionRuntimeSnapshot` 一起下发。
///
/// 不带 `turn_id`：承载它的 Snapshot 变体已经有了。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPlanSnapshot {
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub updated_at: String,
}

/// 历史 Turn 的计划，按 Session 一次性加载。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPlanRecord {
    pub turn_id: String,
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub updated_at: String,
}

impl TurnPlan {
    /// 时间序列化必须带 `+08:00`。库里存的是东八区墙上时间，标成 `Z` 会让前端在已经是
    /// 东八区的值上再换算一次，最终偏 16 小时且全程不报错。见 `.claude/rules/database.md`。
    fn wire_updated_at(&self) -> String {
        to_wire(self.updated_at).unwrap_or_default()
    }

    pub fn to_snapshot(&self) -> TurnPlanSnapshot {
        TurnPlanSnapshot {
            explanation: self.explanation.clone(),
            steps: self.steps.clone(),
            updated_at: self.wire_updated_at(),
        }
    }

    pub fn to_record(&self) -> TurnPlanRecord {
        TurnPlanRecord {
            turn_id: self.turn_id.to_string(),
            explanation: self.explanation.clone(),
            steps: self.steps.clone(),
            updated_at: self.wire_updated_at(),
        }
    }
}

/// 步骤在 reminder 里的状态标记。
///
/// 直接用 wire 形式的状态名，不发明 `[x]` / `[>]` 之类的符号：一来 reminder 是 XML 包裹
/// 的，`>` 会被 `escape_reminder_text` 转成 `&gt;`，模型读到的是转义后的噪声；二来 schema、
/// 数据库、事件、UI 已经统一用这套名字，再加一套符号就是第二份需要对齐的词汇表。
fn status_marker(status: PlanStepStatus) -> String {
    format!("[{}]", status.as_str())
}

/// 把计划投影进压缩 reminder。
///
/// 压缩把模型看到的对话整体替换成"用户消息重放 + 摘要 + reminder"三条，原来的
/// `update_plan` Tool Call 和它的结果都不在其中。所以压缩之后，reminder 是当前计划
/// **唯一**的载体——它错了模型就完全失忆，而不是"少了一层冗余"。
pub struct PlanStateContributor;

#[async_trait]
impl CompactionStateContributor for PlanStateContributor {
    fn key(&self) -> &'static str {
        TURN_PLAN_STATE_KEY
    }

    fn schema_version(&self) -> u16 {
        TURN_PLAN_STATE_SCHEMA_VERSION
    }

    fn failure_policy(&self) -> CompactionStateFailurePolicy {
        CompactionStateFailurePolicy::RequiredWhenEnabled
    }

    async fn collect(
        &self,
        input: &CompactionStateCollectInput<'_>,
    ) -> Result<Option<Value>, CompactionStateError> {
        // `None` 与空计划必须区分，且这里错了不会报错，只会让模型继续看到已经删掉的计划：
        //
        // - `Some(plan)`（含空计划）→ 返回 `Some`，覆盖 extensions 里的旧值；
        // - `None`（rewind，没有 Turn 上下文）→ 返回 `None`，collector 结转上次的值。
        //
        // 清空计划时若返回 `None`，上一版计划会在 extensions 里存活并继续注入。空计划要
        // 显式写成空值覆盖，由 `render` 决定不出 section。
        let Some(plan) = input.plan else {
            return Ok(None);
        };
        Ok(Some(json!({
            "explanation": plan.explanation,
            "steps": plan.steps,
        })))
    }

    fn render(
        &self,
        schema_version: u16,
        value: &Value,
    ) -> Result<Option<ReminderSection>, CompactionStateError> {
        if schema_version != self.schema_version() {
            return Err(CompactionStateError::Contributor {
                key: self.key().to_string(),
                message: format!("unsupported schema version: {schema_version}"),
            });
        }

        let steps: Vec<PlanStep> = value
            .get("steps")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| CompactionStateError::Contributor {
                key: self.key().to_string(),
                message: format!("invalid steps: {error}"),
            })?
            .ok_or_else(|| CompactionStateError::Contributor {
                key: self.key().to_string(),
                message: "steps must be present".to_string(),
            })?;

        if steps.is_empty() {
            return Ok(None);
        }

        let explanation = value.get("explanation").and_then(Value::as_str);
        let mut lines = Vec::with_capacity(steps.len() + 1);
        if let Some(explanation) = explanation.filter(|text| !text.trim().is_empty()) {
            lines.push(explanation.to_string());
        }
        lines.extend(
            steps
                .iter()
                .map(|step| format!("- {} {}", status_marker(step.status), step.step)),
        );

        Ok(Some(ReminderSection {
            title: "Current plan".to_string(),
            lines,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::TurnId;
    use time::macros::datetime;

    fn plan(steps: Vec<PlanStep>, explanation: Option<&str>) -> TurnPlan {
        TurnPlan {
            turn_id: TurnId::new("turn-1"),
            explanation: explanation.map(str::to_string),
            steps,
            updated_at: datetime!(2026-08-07 18:30:00),
        }
    }

    fn step(text: &str, status: PlanStepStatus) -> PlanStep {
        PlanStep {
            step: text.to_string(),
            status,
        }
    }

    fn collected(plan: Option<&TurnPlan>) -> Option<Value> {
        let input = CompactionStateCollectInput {
            messages: &[],
            plan,
        };
        tokio_test_block_on(PlanStateContributor.collect(&input)).expect("collect")
    }

    fn tokio_test_block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    #[test]
    fn serializes_time_with_an_east_eight_offset() {
        let snapshot = plan(vec![step("a", PlanStepStatus::Pending)], None).to_snapshot();

        assert!(
            snapshot.updated_at.ends_with("+08:00"),
            "a Z suffix would make the UI 16 hours off: {}",
            snapshot.updated_at
        );
        assert!(snapshot.updated_at.starts_with("2026-08-07T18:30:00"));
    }

    #[test]
    fn record_carries_the_turn_id_but_snapshot_does_not() {
        let plan = plan(vec![step("a", PlanStepStatus::Pending)], Some("why"));

        assert_eq!(plan.to_record().turn_id, "turn-1");
        assert_eq!(plan.to_snapshot().explanation.as_deref(), Some("why"));
    }

    #[test]
    fn renders_every_status_with_a_distinct_marker() {
        let value = collected(Some(&plan(
            vec![
                step("done", PlanStepStatus::Completed),
                step("doing", PlanStepStatus::InProgress),
                step("todo", PlanStepStatus::Pending),
            ],
            None,
        )))
        .expect("some");

        let section = PlanStateContributor
            .render(TURN_PLAN_STATE_SCHEMA_VERSION, &value)
            .expect("render")
            .expect("section");

        assert_eq!(section.title, "Current plan");
        assert_eq!(
            section.lines,
            [
                "- [completed] done",
                "- [in_progress] doing",
                "- [pending] todo"
            ],
            "markers reuse the wire status names so nothing gets XML-escaped"
        );
    }

    #[test]
    fn renders_the_explanation_above_the_steps_when_present() {
        let value = collected(Some(&plan(
            vec![step("a", PlanStepStatus::Pending)],
            Some("scoping the work"),
        )))
        .expect("some");

        let section = PlanStateContributor
            .render(TURN_PLAN_STATE_SCHEMA_VERSION, &value)
            .expect("render")
            .expect("section");

        assert_eq!(section.lines[0], "scoping the work");
        assert_eq!(section.lines[1], "- [pending] a");
    }

    #[test]
    fn blank_explanations_do_not_leave_an_empty_line() {
        let value = collected(Some(&plan(
            vec![step("a", PlanStepStatus::Pending)],
            Some("   "),
        )))
        .expect("some");

        let section = PlanStateContributor
            .render(TURN_PLAN_STATE_SCHEMA_VERSION, &value)
            .expect("render")
            .expect("section");

        assert_eq!(section.lines, ["- [pending] a"]);
    }

    #[test]
    fn an_empty_plan_overwrites_rather_than_carrying_the_old_one_forward() {
        // 这条守 docs/update-plan.md §8.1 那张表的第二行。collector 在 `collect` 返回
        // `None` 时会保留并继续渲染旧值，所以清空计划**必须**返回 `Some(空)`。
        let value = collected(Some(&plan(Vec::new(), None))).expect("empty plan still overwrites");

        assert_eq!(value["steps"], json!([]));
        assert!(
            PlanStateContributor
                .render(TURN_PLAN_STATE_SCHEMA_VERSION, &value)
                .expect("render")
                .is_none(),
            "an empty plan must not render a section"
        );
    }

    #[test]
    fn no_turn_context_carries_the_previous_value_forward() {
        assert!(
            collected(None).is_none(),
            "rewind has no turn, so the collector must keep whatever it already had"
        );
    }

    #[test]
    fn rejects_unknown_schema_versions_and_malformed_values() {
        assert!(
            PlanStateContributor
                .render(TURN_PLAN_STATE_SCHEMA_VERSION + 1, &json!({ "steps": [] }))
                .is_err()
        );
        assert!(PlanStateContributor
            .render(TURN_PLAN_STATE_SCHEMA_VERSION, &json!({}))
            .is_err());
        assert!(
            PlanStateContributor
                .render(
                    TURN_PLAN_STATE_SCHEMA_VERSION,
                    &json!({ "steps": [{ "step": "a", "status": "bogus" }] })
                )
                .is_err()
        );
    }
}
