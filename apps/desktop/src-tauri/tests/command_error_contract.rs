use openwork_desktop_lib::{CommandError, CommandErrorCode};
use serde_json::json;

#[test]
fn command_error_exposes_a_stable_machine_code_and_safe_message() {
    let error = CommandError::new(
        CommandErrorCode::ThreadNotFound,
        "Session not found".to_string(),
    );

    assert_eq!(error.code, CommandErrorCode::ThreadNotFound);
    assert_eq!(error.message, "Session not found");
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({
            "code": "thread_not_found",
            "message": "Session not found"
        })
    );
}
