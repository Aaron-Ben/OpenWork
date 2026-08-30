use openwork_collab::computer::triage::parse_triage;

#[test]
fn triage_parser_accepts_bounded_local_model_wrappers_but_rejects_ambiguous_text() {
    let verdict = parse_triage(
        "{\"actionable\":false,\"reason\":\"informational only\",\"promptNote\":\"\"}",
    )
    .unwrap();
    assert!(!verdict.actionable);
    assert_eq!(verdict.reason, "informational only");

    assert!(parse_triage("I think yes").is_err());
    let fenced = parse_triage(
        "```json\n{\"actionable\":true,\"reason\":\"human is waiting\",\"promptNote\":\"reply\",\"confidence\":0.9}\n```",
    )
    .unwrap();
    assert!(fenced.actionable);
    assert_eq!(fenced.prompt_note, "reply");

    let chatty = parse_triage(
        "Here is the result: {\"actionable\":false,\"reason\":\"agent-only acknowledgement\",\"promptNote\":\"\"} Done.",
    )
    .unwrap();
    assert!(!chatty.actionable);

    let truncated = parse_triage(
        "{\"actionable\":true,\"reason\":\"specific agent request\",\"promptNote\":\"advance the work\"",
    )
    .unwrap();
    assert!(truncated.actionable);
    assert_eq!(truncated.reason, "specific agent request");
    assert!(parse_triage("{\"actionable\":true}").is_err());
}
