//! `turn_plans` 的持久化行为。
//!
//! 重点不是"能存能取"，而是三条会静默出错的性质：提交的原子性、Turn 之间的隔离，
//! 以及级联删除。见 `docs/update-plan.md` §13.3。

use openwork_core::plan::{PlanStep, PlanStepStatus, TurnPlan};
use openwork_core::{
    ClientRequestId, ModelCapabilities, ModelInput, PostgresStorage, ResolvedModel, SessionId,
    SessionInput, SessionStorage, TurnOutcome, session::TurnId,
};
use openwork_models::model::{
    ContentBlock, Message, Role, ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
};
use serde_json::json;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};
use uuid::Uuid;

fn test_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn test_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        context_window_tokens: 200_000,
        max_output_tokens: 32_768,
        max_reasoning_tokens: None,
        accepts_data_blocks: true,
    }
}

fn china_now() -> PrimitiveDateTime {
    let offset = UtcOffset::from_hms(8, 0, 0).unwrap();
    let now = OffsetDateTime::now_utc().to_offset(offset);
    PrimitiveDateTime::new(now.date(), now.time())
}

fn step(text: &str, status: PlanStepStatus) -> PlanStep {
    PlanStep {
        step: text.to_string(),
        status,
    }
}

fn plan(turn_id: &TurnId, steps: Vec<PlanStep>, explanation: Option<&str>) -> TurnPlan {
    TurnPlan {
        turn_id: turn_id.clone(),
        explanation: explanation.map(str::to_string),
        steps,
        updated_at: china_now(),
    }
}

fn tool_result(call_id: &str) -> Message {
    Message {
        role: Role::Tool,
        content: vec![ContentBlock::ToolResult(ToolResultBlock {
            id: call_id.to_string(),
            name: "update_plan".to_string(),
            output: vec![ContentBlock::text("Plan updated")],
            state: ToolResultState::Success,
            artifacts: Vec::new(),
        })],
    }
}

fn tool_call(call_id: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolCall(ToolCallBlock {
            id: call_id.to_string(),
            name: "update_plan".to_string(),
            input: r#"{"plan":[]}"#.to_string(),
            state: ToolCallState::Submitted,
        })],
    }
}

struct Fixture {
    storage: PostgresStorage,
    session_id: SessionId,
}

impl Fixture {
    async fn new(database_url: &str, label: &str) -> Self {
        let storage = PostgresStorage::connect(Some(database_url)).await.unwrap();
        storage.migrate().await.unwrap();

        let model_id = unique("model-plan");
        storage
            .upsert_model(&ModelInput {
                id: model_id.clone(),
                display_name: "Plan test model".to_string(),
                provider_kind: "deepseek".to_string(),
                model_name: "deepseek-v4-flash".to_string(),
                base_url: format!("https://example.invalid/{model_id}"),
                credential_ref: None,
                enabled: true,
                capabilities: test_capabilities(),
                config: json!({}),
            })
            .await
            .unwrap();

        let session_id = SessionId::new(unique(label));
        storage
            .create_session(&SessionInput {
                id: session_id.clone(),
                title: Some("Turn plans".to_string()),
                working_directory: "/tmp/openwork-turn-plans".to_string(),
                default_model_id: Some(model_id),
            })
            .await
            .unwrap();

        Self {
            storage,
            session_id,
        }
    }

    async fn begin_turn(&self) -> TurnId {
        let turn_id = TurnId::new(unique("turn-plan"));
        self.storage
            .begin_turn(
                &self.session_id,
                &turn_id,
                &ClientRequestId::new(unique("request-plan")),
                &ResolvedModel::new(
                    None::<String>,
                    "deepseek",
                    "deepseek-v4-flash",
                    test_capabilities(),
                ),
                &[],
                &Message::text(Role::User, "do the multi-step thing"),
            )
            .await
            .unwrap();
        self.storage.begin_model_call(&turn_id, 1, 1).await.unwrap();
        turn_id
    }

    async fn tool_message_count(&self, turn_id: &TurnId) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE turn_id = $1 AND role = 'tool'")
            .bind(turn_id.as_str())
            .fetch_one(self.storage.pool())
            .await
            .unwrap()
    }

    /// 删除测试 Session，级联清掉它的 Turn。
    ///
    /// 这些测试共用一个数据库，而 `mark_running_interrupted` 之类的查询是全库范围的：
    /// 留下 running Turn 会让别的测试文件看到本不属于它的行。
    async fn cleanup(&self) {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(self.session_id.as_str())
            .execute(self.storage.pool())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn commit_writes_the_plan_and_the_success_tool_result_together() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let fixture = Fixture::new(&database_url, "session-plan-commit").await;
    let turn_id = fixture.begin_turn().await;

    assert!(
        fixture
            .storage
            .load_turn_plan(&turn_id)
            .await
            .unwrap()
            .is_none(),
        "a fresh turn starts with no plan"
    );

    fixture
        .storage
        .append_assistant_message(&turn_id, &tool_call("call-1"), None)
        .await
        .unwrap();
    let submitted = plan(
        &turn_id,
        vec![
            step("read schema", PlanStepStatus::Completed),
            step("add migration", PlanStepStatus::InProgress),
            step("wire the runner", PlanStepStatus::Pending),
        ],
        Some("scoping the work"),
    );
    fixture
        .storage
        .commit_plan_update(&turn_id, &submitted, &tool_result("call-1"))
        .await
        .unwrap();

    let loaded = fixture
        .storage
        .load_turn_plan(&turn_id)
        .await
        .unwrap()
        .expect("plan");
    assert_eq!(loaded.steps, submitted.steps, "order must be preserved");
    assert_eq!(loaded.explanation.as_deref(), Some("scoping the work"));
    assert_eq!(fixture.tool_message_count(&turn_id).await, 1);

    // Assistant Tool Call 必须先于 Tool Result：这是审计顺序，与普通工具一致。
    let sequences: Vec<(String, i64)> = sqlx::query_as(
        "SELECT role, sequence FROM messages WHERE turn_id = $1 AND role IN ('assistant', 'tool')
         ORDER BY sequence",
    )
    .bind(turn_id.as_str())
    .fetch_all(fixture.storage.pool())
    .await
    .unwrap();
    assert_eq!(sequences[0].0, "assistant");
    assert_eq!(sequences[1].0, "tool");
    assert!(sequences[0].1 < sequences[1].1);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_rejected_tool_result_leaves_neither_the_plan_nor_a_message() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let fixture = Fixture::new(&database_url, "session-plan-atomic").await;
    let turn_id = fixture.begin_turn().await;

    let first = plan(
        &turn_id,
        vec![step("keep me", PlanStepStatus::InProgress)],
        None,
    );
    fixture
        .storage
        .commit_plan_update(&turn_id, &first, &tool_result("call-1"))
        .await
        .unwrap();

    // 一条不合法的 Tool Result（角色错误）必须让整个提交失败，而不是先把计划写进去。
    let replacement = plan(
        &turn_id,
        vec![step("must not land", PlanStepStatus::Completed)],
        Some("should roll back"),
    );
    let error = fixture
        .storage
        .commit_plan_update(
            &turn_id,
            &replacement,
            &Message::text(Role::Assistant, "not a tool result"),
        )
        .await
        .expect_err("invalid tool result must fail the whole commit");
    assert!(error.contains("tool message"), "got: {error}");

    let loaded = fixture
        .storage
        .load_turn_plan(&turn_id)
        .await
        .unwrap()
        .expect("plan");
    assert_eq!(
        loaded.steps, first.steps,
        "a failed commit must not change the existing plan"
    );
    assert_eq!(
        fixture.tool_message_count(&turn_id).await,
        1,
        "the rolled-back commit must not leave a tool message behind"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn successive_calls_keep_only_the_latest_snapshot() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let fixture = Fixture::new(&database_url, "session-plan-replace").await;
    let turn_id = fixture.begin_turn().await;

    fixture
        .storage
        .commit_plan_update(
            &turn_id,
            &plan(
                &turn_id,
                vec![
                    step("first", PlanStepStatus::InProgress),
                    step("second", PlanStepStatus::Pending),
                ],
                Some("initial"),
            ),
            &tool_result("call-1"),
        )
        .await
        .unwrap();
    fixture
        .storage
        .commit_plan_update(
            &turn_id,
            &plan(
                &turn_id,
                vec![
                    step("first", PlanStepStatus::Completed),
                    step("second", PlanStepStatus::InProgress),
                ],
                None,
            ),
            &tool_result("call-2"),
        )
        .await
        .unwrap();

    let loaded = fixture
        .storage
        .load_turn_plan(&turn_id)
        .await
        .unwrap()
        .expect("plan");
    assert_eq!(loaded.steps[0].status, PlanStepStatus::Completed);
    assert_eq!(
        loaded.explanation, None,
        "an absent explanation must not inherit the previous one"
    );

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turn_plans WHERE turn_id = $1")
        .bind(turn_id.as_str())
        .fetch_one(fixture.storage.pool())
        .await
        .unwrap();
    assert_eq!(rows, 1, "the table holds one snapshot, not a revision log");
    assert_eq!(
        fixture.tool_message_count(&turn_id).await,
        2,
        "the conversation still records that both calls happened"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_empty_plan_is_stored_as_an_explicit_clear() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let fixture = Fixture::new(&database_url, "session-plan-clear").await;
    let turn_id = fixture.begin_turn().await;

    fixture
        .storage
        .commit_plan_update(
            &turn_id,
            &plan(&turn_id, vec![step("a", PlanStepStatus::Pending)], None),
            &tool_result("call-1"),
        )
        .await
        .unwrap();
    fixture
        .storage
        .commit_plan_update(
            &turn_id,
            &plan(&turn_id, Vec::new(), None),
            &tool_result("call-2"),
        )
        .await
        .unwrap();

    let loaded = fixture
        .storage
        .load_turn_plan(&turn_id)
        .await
        .unwrap()
        .expect("a cleared plan is still a row, not an absent one");
    assert!(loaded.steps.is_empty());

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_new_turn_does_not_inherit_the_previous_plan() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let fixture = Fixture::new(&database_url, "session-plan-isolation").await;

    let first_turn = fixture.begin_turn().await;
    fixture
        .storage
        .commit_plan_update(
            &first_turn,
            &plan(
                &first_turn,
                vec![step("turn one work", PlanStepStatus::Completed)],
                None,
            ),
            &tool_result("call-1"),
        )
        .await
        .unwrap();

    // 一个 Session 同时只允许一个 running Turn（uq_turns_one_running_per_session），
    // 所以下一个 Turn 之前必须先收尾——这也正是 Turn 串行的证据。
    fixture
        .storage
        .finish_turn(
            &first_turn,
            &TurnOutcome::Completed {
                final_text: "done".to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let second_turn = fixture.begin_turn().await;
    assert!(
        fixture
            .storage
            .load_turn_plan(&second_turn)
            .await
            .unwrap()
            .is_none(),
        "plans are scoped to a turn and must not leak into the next one"
    );

    let all = fixture
        .storage
        .load_session_turn_plans(&fixture.session_id)
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].turn_id, first_turn);

    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_the_session_cascades_to_its_plans() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let fixture = Fixture::new(&database_url, "session-plan-cascade").await;
    let turn_id = fixture.begin_turn().await;
    fixture
        .storage
        .commit_plan_update(
            &turn_id,
            &plan(&turn_id, vec![step("a", PlanStepStatus::Pending)], None),
            &tool_result("call-1"),
        )
        .await
        .unwrap();

    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(fixture.session_id.as_str())
        .execute(fixture.storage.pool())
        .await
        .unwrap();

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turn_plans WHERE turn_id = $1")
        .bind(turn_id.as_str())
        .fetch_one(fixture.storage.pool())
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

/// §15.1 的观测信号。
///
/// 这不是约束而是可查询的事实：Turn 因错误或取消终止时留下未完成步骤是**正确**的，
/// 靠 `turns.status` 在查询时区分，不在写入时提前过滤。
mod completion_signal {
    use super::*;

    async fn signal_of(fixture: &Fixture, turn_id: &TurnId) -> Option<i32> {
        sqlx::query_scalar("SELECT plan_unfinished_step_count FROM turns WHERE id = $1")
            .bind(turn_id.as_str())
            .fetch_one(fixture.storage.pool())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_turn_without_a_plan_records_null_not_zero() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let fixture = Fixture::new(&database_url, "session-signal-noplan").await;
        let turn_id = fixture.begin_turn().await;

        fixture
            .storage
            .finish_turn(
                &turn_id,
                &TurnOutcome::Completed {
                    final_text: "simple answer".to_string(),
                },
                None,
            )
            .await
            .unwrap();

        assert_eq!(
            signal_of(&fixture, &turn_id).await,
            None,
            "把'没建计划'和'建了且全部收尾'合并成 0 会让统计的分母失真"
        );

        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn a_fully_finished_plan_records_zero() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let fixture = Fixture::new(&database_url, "session-signal-finished").await;
        let turn_id = fixture.begin_turn().await;

        let done = plan(
            &turn_id,
            vec![
                step("a", PlanStepStatus::Completed),
                step("b", PlanStepStatus::Completed),
            ],
            None,
        );
        fixture
            .storage
            .commit_plan_update(&turn_id, &done, &tool_result("call-1"))
            .await
            .unwrap();
        fixture
            .storage
            .finish_turn(
                &turn_id,
                &TurnOutcome::Completed {
                    final_text: "done".to_string(),
                },
                Some(done.unfinished_step_count()),
            )
            .await
            .unwrap();

        assert_eq!(signal_of(&fixture, &turn_id).await, Some(0));

        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn an_unfinished_plan_on_a_completed_turn_is_the_case_worth_querying() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let fixture = Fixture::new(&database_url, "session-signal-forgot").await;
        let turn_id = fixture.begin_turn().await;

        // 模型干完了活、正常作答，但忘了把剩下两步标成 completed。
        let forgotten = plan(
            &turn_id,
            vec![
                step("a", PlanStepStatus::Completed),
                step("b", PlanStepStatus::InProgress),
                step("c", PlanStepStatus::Pending),
            ],
            None,
        );
        fixture
            .storage
            .commit_plan_update(&turn_id, &forgotten, &tool_result("call-1"))
            .await
            .unwrap();
        fixture
            .storage
            .finish_turn(
                &turn_id,
                &TurnOutcome::Completed {
                    final_text: "here is your answer".to_string(),
                },
                Some(forgotten.unfinished_step_count()),
            )
            .await
            .unwrap();

        assert_eq!(signal_of(&fixture, &turn_id).await, Some(2));

        // 文档 §15.1 给出的查询要能把它算进来。
        let forgot_to_finish: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM turns
             WHERE session_id = $1
               AND status = 'completed'
               AND plan_unfinished_step_count > 0",
        )
        .bind(fixture.session_id.as_str())
        .fetch_one(fixture.storage.pool())
        .await
        .unwrap();
        assert_eq!(forgot_to_finish, 1);

        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn a_cancelled_turn_keeps_its_unfinished_steps_without_being_flagged() {
        let Some(database_url) = test_database_url() else {
            return;
        };
        let fixture = Fixture::new(&database_url, "session-signal-cancelled").await;
        let turn_id = fixture.begin_turn().await;

        let interrupted = plan(
            &turn_id,
            vec![
                step("a", PlanStepStatus::Completed),
                step("b", PlanStepStatus::InProgress),
            ],
            None,
        );
        fixture
            .storage
            .commit_plan_update(&turn_id, &interrupted, &tool_result("call-1"))
            .await
            .unwrap();
        fixture
            .storage
            .finish_turn(
                &turn_id,
                &TurnOutcome::Cancelled,
                Some(interrupted.unfinished_step_count()),
            )
            .await
            .unwrap();

        // 计数照记 —— 它是事实，不是指控。
        assert_eq!(signal_of(&fixture, &turn_id).await, Some(1));

        // 但 §15.1 的查询按 status 过滤，取消的 Turn 不该算进"模型忘了收尾"。
        let forgot_to_finish: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM turns
             WHERE session_id = $1
               AND status = 'completed'
               AND plan_unfinished_step_count > 0",
        )
        .bind(fixture.session_id.as_str())
        .fetch_one(fixture.storage.pool())
        .await
        .unwrap();
        assert_eq!(
            forgot_to_finish, 0,
            "一个被取消的 Turn 留下未完成步骤是正确的记录，不是违规"
        );

        fixture.cleanup().await;
    }
}
