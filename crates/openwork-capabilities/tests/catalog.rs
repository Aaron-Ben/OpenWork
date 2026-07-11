use openwork_capabilities::{CapabilityCatalog, CatalogError};
use openwork_protocol::capability::{CapabilityResolverPort, CapabilityRiskHint, CapabilitySpec};
use serde_json::json;

fn spec(name: &str) -> CapabilitySpec {
    CapabilitySpec {
        name: name.to_string(),
        description: format!("{name} description"),
        input_schema: json!({"type": "object"}),
        risk_hint: CapabilityRiskHint::ReadOnly,
    }
}

#[test]
fn catalog_rejects_duplicate_names() {
    let error = CapabilityCatalog::try_new([spec("read"), spec("read")])
        .expect_err("duplicate capability names must be rejected");
    assert!(matches!(error, CatalogError::DuplicateName(name) if name == "read"));
}

#[test]
fn catalog_rejects_blank_names_and_descriptions() {
    let mut blank_name = spec("read");
    blank_name.name = "  ".to_string();
    assert!(matches!(
        CapabilityCatalog::try_new([blank_name]),
        Err(CatalogError::BlankName)
    ));

    let mut blank_description = spec("read");
    blank_description.description = "  ".to_string();
    assert!(matches!(
        CapabilityCatalog::try_new([blank_description]),
        Err(CatalogError::BlankDescription(name)) if name == "read"
    ));
}

#[tokio::test]
async fn builtin_catalog_has_expected_names_and_risk_hints() {
    let catalog = CapabilityCatalog::builtin().expect("builtin catalog is valid");
    let specs = catalog.list().await.expect("builtin catalog resolves");
    let actual = specs
        .iter()
        .map(|spec| (spec.name.as_str(), spec.risk_hint))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            ("read", CapabilityRiskHint::ReadOnly),
            ("write", CapabilityRiskHint::WorkspaceMutation),
            ("edit", CapabilityRiskHint::WorkspaceMutation),
            ("grep", CapabilityRiskHint::ReadOnly),
            ("glob", CapabilityRiskHint::ReadOnly),
            ("list", CapabilityRiskHint::ReadOnly),
            ("bash", CapabilityRiskHint::ProcessExecution),
        ]
    );
}
