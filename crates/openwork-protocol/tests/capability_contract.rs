use openwork_protocol::capability::{
    ActionRequest, CapabilityRiskHint, CapabilitySpec, Observation, ObservationErrorCode,
    ObservationStatus,
};
use serde_json::json;

#[test]
fn capability_spec_maps_to_model_tool_definition() {
    let spec = CapabilitySpec {
        name: "read".to_string(),
        description: "Read a file".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        }),
        risk_hint: CapabilityRiskHint::ReadOnly,
    };

    let definition = spec.model_definition();
    assert_eq!(definition.name, "read");
    assert_eq!(definition.description, "Read a file");
    assert_eq!(definition.parameters, spec.input_schema);
}

#[test]
fn action_request_and_observation_round_trip_through_json() {
    let request = ActionRequest::new("read", json!({"path": "README.md"}));
    let encoded = serde_json::to_value(&request).expect("request serializes");
    let decoded: ActionRequest = serde_json::from_value(encoded).expect("request deserializes");
    assert_eq!(decoded, request);

    let observation = Observation::failed(
        ObservationErrorCode::InvalidArguments,
        "missing path",
        false,
    );
    let encoded = serde_json::to_value(&observation).expect("observation serializes");
    let decoded: Observation = serde_json::from_value(encoded).expect("observation deserializes");
    assert_eq!(decoded, observation);
    assert_eq!(decoded.status, ObservationStatus::Failed);
    assert!(decoded.is_error());
    assert_eq!(decoded.text_content(), "missing path");
}

#[test]
fn observation_terminal_states_are_distinct() {
    assert_eq!(
        Observation::succeeded("ok").status,
        ObservationStatus::Succeeded
    );
    assert_eq!(
        Observation::denied("no access").status,
        ObservationStatus::Denied
    );
    let approval_denied = Observation::approval_denied("user denied");
    assert_eq!(approval_denied.status, ObservationStatus::Denied);
    assert_eq!(
        approval_denied.error.expect("error details").code,
        ObservationErrorCode::ApprovalDenied
    );
    assert_eq!(
        Observation::cancelled("cancelled").status,
        ObservationStatus::Cancelled
    );
    assert_eq!(
        Observation::outcome_unknown("unknown").status,
        ObservationStatus::OutcomeUnknown
    );
}
