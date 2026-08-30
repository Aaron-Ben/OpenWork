use openwork_collab::computer::triage::parse_triage;

#[test]
fn triage_parser_accepts_exact_json_but_rejects_wrappers_and_ambiguous_text() {
    let verdict = parse_triage(
        "{\"actionable\":false,\"reason\":\"informational only\",\"promptNote\":\"\"}",
    )
    .unwrap();
    assert!(!verdict.actionable);
    assert_eq!(verdict.reason, "informational only");

    assert!(parse_triage("I think yes").is_err());
    assert!(
        parse_triage("```json\n{\"actionable\":true,\"reason\":\"x\",\"promptNote\":\"\"}\n```")
            .is_err()
    );
    assert!(parse_triage("{\"actionable\":true}").is_err());
}
