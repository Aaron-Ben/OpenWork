use openwork_desktop_lib::{CommandError, CommandErrorCode};
use serde_json::json;

#[test]
fn command_error_exposes_a_stable_machine_code_and_safe_message() {
    let error = CommandError::new(
        CommandErrorCode::CollaborationUnavailable,
        "Collaboration is unavailable".to_string(),
    );

    assert_eq!(error.code, CommandErrorCode::CollaborationUnavailable);
    assert_eq!(error.message, "Collaboration is unavailable");
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({
            "code": "collaboration_unavailable",
            "message": "Collaboration is unavailable"
        })
    );
}
