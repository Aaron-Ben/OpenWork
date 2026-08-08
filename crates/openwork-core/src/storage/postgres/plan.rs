//! `turn_plans` 的读写。
//!
//! 写入路径只有一条：[`PostgresStorage::commit_plan_update_inner`]，它在一个事务里同时
//! 落计划和成功的 Tool Result。没有单独的"只写计划"接口——那会让"计划变了但历史里没有
//! 对应调用"成为可构造的状态。

use super::*;

use time::PrimitiveDateTime;

use crate::plan::{PlanStep, TurnPlan};

/// 一次成功的 `update_plan` 调用的两项写入。
///
/// 拆出来是为了让 `append_tool_result_inner` 和 `commit_plan_update_inner` 共用同一套
/// tool message 校验，避免两条路径对"什么算合法 Tool Result"产生分歧。
pub(super) struct ValidatedToolResult {
    pub content: Value,
    pub provider_call_id: String,
    pub tool_name: String,
}

pub(super) fn validate_tool_message(
    message: &Message,
) -> Result<ValidatedToolResult, StorageError> {
    if message.role != Role::Tool {
        return Err(StorageError::InvalidInput(
            "expected a tool message".to_string(),
        ));
    }
    let mut tool_results = message.content.iter().filter_map(|block| match block {
        ContentBlock::ToolResult(result) => Some(result),
        _ => None,
    });
    let result = tool_results.next().ok_or_else(|| {
        StorageError::InvalidInput("tool message has no tool result block".to_string())
    })?;
    if tool_results.next().is_some() || message.content.len() != 1 {
        return Err(StorageError::InvalidInput(
            "tool message must contain exactly one tool result block".to_string(),
        ));
    }
    Ok(ValidatedToolResult {
        content: serde_json::to_value(&message.content)?,
        provider_call_id: result.id.clone(),
        tool_name: result.name.clone(),
    })
}

impl PostgresStorage {
    pub(super) async fn load_turn_plan_inner(
        &self,
        turn_id: &TurnId,
    ) -> Result<Option<TurnPlan>, StorageError> {
        let row: Option<(Option<String>, Value, PrimitiveDateTime)> = sqlx::query_as(
            "SELECT explanation, steps, updated_at FROM turn_plans WHERE turn_id = $1",
        )
        .bind(turn_id.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.map(|(explanation, steps, updated_at)| {
            Ok(TurnPlan {
                turn_id: turn_id.clone(),
                explanation,
                steps: decode_steps(steps)?,
                updated_at,
            })
        })
        .transpose()
    }

    pub(super) async fn load_session_turn_plans_inner(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<TurnPlan>, StorageError> {
        // 按 Turn 的时间顺序返回，前端在 transcript 投影边界一次性按 turnId 建索引。
        let rows: Vec<(String, Option<String>, Value, PrimitiveDateTime)> = sqlx::query_as(
            "SELECT p.turn_id, p.explanation, p.steps, p.updated_at
             FROM turn_plans p
             JOIN turns t ON t.id = p.turn_id
             WHERE t.session_id = $1
             ORDER BY t.sequence",
        )
        .bind(session_id.as_str())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(turn_id, explanation, steps, updated_at)| {
                Ok(TurnPlan {
                    turn_id: TurnId::new(turn_id),
                    explanation,
                    steps: decode_steps(steps)?,
                    updated_at,
                })
            })
            .collect()
    }

    pub(super) async fn commit_plan_update_inner(
        &self,
        turn_id: &TurnId,
        plan: &TurnPlan,
        success_tool_result: &Message,
    ) -> Result<(), StorageError> {
        let tool_result = validate_tool_message(success_tool_result)?;
        let steps = serde_json::to_value(&plan.steps)?;

        let mut transaction = self.pool.begin().await?;
        let session_id = lock_turn(&mut transaction, turn_id).await?;

        // 一次调用替换整个计划，所以是 upsert 而不是 append。updated_at 显式绑定
        // `china_now()` 的结果，不依赖列默认值——同一个值还要进 Snapshot 和事件，
        // 让两边读到不同的时间没有意义。
        sqlx::query(
            "INSERT INTO turn_plans (turn_id, explanation, steps, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (turn_id) DO UPDATE
             SET explanation = EXCLUDED.explanation,
                 steps = EXCLUDED.steps,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(turn_id.as_str())
        .bind(plan.explanation.as_deref())
        .bind(&steps)
        .bind(plan.updated_at)
        .execute(&mut *transaction)
        .await?;

        insert_message(
            &mut transaction,
            &session_id,
            Some(turn_id),
            Role::Tool,
            tool_result.content,
            MessageKind::Normal,
            Some((&tool_result.provider_call_id, &tool_result.tool_name)),
        )
        .await?;

        transaction.commit().await?;
        Ok(())
    }
}

fn decode_steps(steps: Value) -> Result<Vec<PlanStep>, StorageError> {
    serde_json::from_value(steps).map_err(StorageError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_models::model::{ToolResultBlock, ToolResultState};
    use serde_json::json;

    fn tool_message(blocks: Vec<ContentBlock>) -> Message {
        Message {
            role: Role::Tool,
            content: blocks,
        }
    }

    fn tool_result_block(id: &str) -> ContentBlock {
        ContentBlock::ToolResult(ToolResultBlock {
            id: id.to_string(),
            name: "update_plan".to_string(),
            output: vec![ContentBlock::text("Plan updated")],
            state: ToolResultState::Success,
            artifacts: Vec::new(),
        })
    }

    #[test]
    fn accepts_exactly_one_tool_result_block() {
        let validated =
            validate_tool_message(&tool_message(vec![tool_result_block("call-1")])).expect("valid");

        assert_eq!(validated.provider_call_id, "call-1");
        assert_eq!(validated.tool_name, "update_plan");
    }

    #[test]
    fn rejects_messages_that_are_not_a_single_tool_result() {
        assert!(validate_tool_message(&Message::text(Role::Assistant, "nope")).is_err());
        assert!(validate_tool_message(&tool_message(Vec::new())).is_err());
        assert!(
            validate_tool_message(&tool_message(vec![
                tool_result_block("call-1"),
                tool_result_block("call-2"),
            ]))
            .is_err()
        );
        assert!(
            validate_tool_message(&tool_message(vec![
                tool_result_block("call-1"),
                ContentBlock::text("stray"),
            ]))
            .is_err(),
            "a tool result plus anything else must not be storable"
        );
    }

    #[test]
    fn decodes_steps_and_rejects_unknown_statuses() {
        let steps =
            decode_steps(json!([{ "step": "a", "status": "in_progress" }])).expect("decode");
        assert_eq!(steps.len(), 1);

        assert!(decode_steps(json!([{ "step": "a", "status": "bogus" }])).is_err());
        assert!(decode_steps(json!({ "not": "an array" })).is_err());
    }
}
