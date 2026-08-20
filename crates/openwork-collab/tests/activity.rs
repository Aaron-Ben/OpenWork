use openwork_collab::{
    activity::{AgentRuntimeRegistry, normalize_activity},
    model::AgentActivity,
    opencode::GlobalEvent,
};
use serde_json::json;

fn event(payload: serde_json::Value) -> GlobalEvent {
    GlobalEvent {
        directory: None,
        project: None,
        payload,
    }
}

#[test]
fn opencode_events_normalize_to_the_five_roster_presentations() {
    assert_eq!(
        normalize_activity(&event(json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {"type": "tool", "tool": "bash", "state": {"status": "running", "title": "$ cargo test"}}
            }
        }))),
        Some(AgentActivity::Executing {
            detail: "$ cargo test".to_string()
        })
    );
    assert_eq!(
        normalize_activity(&event(json!({
            "type": "message.updated",
            "properties": {"sessionID": "ses_1", "info": {"role": "assistant"}}
        }))),
        Some(AgentActivity::Replying)
    );
    assert_eq!(
        normalize_activity(&event(json!({
            "type": "session.compacted",
            "properties": {"sessionID": "ses_1"}
        }))),
        Some(AgentActivity::Compacting)
    );
    assert_eq!(
        normalize_activity(&event(json!({
            "type": "session.status",
            "properties": {"sessionID": "ses_1", "status": {"type": "busy"}}
        }))),
        Some(AgentActivity::Busy)
    );
    assert_eq!(
        normalize_activity(&event(json!({
            "type": "session.status",
            "properties": {"sessionID": "ses_1", "status": {"type": "idle"}}
        }))),
        Some(AgentActivity::Idle)
    );
}

#[tokio::test]
async fn a_busy_session_without_events_becomes_unresponsive() {
    let registry = AgentRuntimeRegistry::default();
    registry.register_session("ses_1", "alice").await;
    let change = registry
        .observe(&event(json!({
            "type": "session.status",
            "properties": {"sessionID": "ses_1", "status": {"type": "busy"}}
        })))
        .await;
    assert_eq!(change, Some(("alice".to_string(), AgentActivity::Busy)));

    assert_eq!(
        registry.mark_unresponsive(std::time::Duration::ZERO).await,
        vec!["alice"]
    );
    assert_eq!(
        registry.activity("alice").await,
        AgentActivity::Unresponsive
    );
}

#[test]
fn executing_detail_truncation_is_unicode_safe() {
    let detail = "执行🧪".repeat(100);
    let activity = normalize_activity(&event(json!({
        "type": "message.part.updated",
        "properties": {
            "sessionID": "ses_1",
            "part": {"type": "tool", "tool": "bash", "state": {"status": "running", "title": detail}}
        }
    })))
    .unwrap();
    let AgentActivity::Executing { detail } = activity else {
        panic!("expected executing activity");
    };
    assert_eq!(detail.chars().count(), 80);
}
