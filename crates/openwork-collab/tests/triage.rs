use openwork_collab::triage::{
    ResponseMode, TriageSource, parse_decision, resolve_agenda_failure, resolve_failure,
};

#[test]
fn triage_decision_parses_model_json_without_using_response_mode_as_selection() {
    let decision = parse_decision(
        "```json\n{\"actionable\":true,\"responseMode\":\"one-of-us\",\"reason\":\"Alice can help\",\"promptNote\":\"Focus on the database\"}\n```",
    )
    .unwrap();

    assert!(decision.actionable);
    assert_eq!(decision.response_mode, ResponseMode::OneOfUs);
    assert_eq!(decision.reason, "Alice can help");
    assert_eq!(decision.prompt_note, "Focus on the database");
}

#[test]
fn triage_failure_is_open_for_a_waiting_human_and_closed_for_agents() {
    let human = resolve_failure(true, "provider timed out");
    assert!(human.actionable);
    assert_eq!(human.source, TriageSource::FailOpen);
    assert_eq!(human.reason, "provider timed out");

    let agents = resolve_failure(false, "provider timed out");
    assert!(!agents.actionable);
    assert_eq!(agents.source, TriageSource::FailClosed);
}

#[test]
fn agenda_failure_never_silences_an_agent_with_candidate_work() {
    let decision = resolve_agenda_failure("support model unavailable");

    assert!(decision.actionable);
    assert_eq!(decision.source, TriageSource::FailOpen);
    assert_eq!(decision.reason, "support model unavailable");
}

#[test]
fn triage_rejects_an_unknown_response_mode() {
    let error = parse_decision(
        r#"{"actionable":true,"responseMode":"alice","reason":"x","promptNote":"y"}"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("responseMode"));
}
