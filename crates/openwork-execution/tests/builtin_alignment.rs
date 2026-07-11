use openwork_capabilities::CapabilityCatalog;
use openwork_execution::PermissionProfile;
use openwork_execution::{BuiltinActionInvoker, ExecutionContext};
use openwork_protocol::capability::CapabilityResolverPort;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn builtin_catalog_and_invoker_names_match() {
    let catalog = CapabilityCatalog::builtin().expect("builtin catalog is valid");
    let declared = catalog
        .list()
        .await
        .expect("builtin catalog resolves")
        .into_iter()
        .map(|spec| spec.name)
        .collect::<Vec<_>>();

    let working_dir = std::env::temp_dir();
    let invoker = BuiltinActionInvoker::new(ExecutionContext::new(
        working_dir.clone(),
        PermissionProfile::workspace_write(working_dir),
        CancellationToken::new(),
    ));
    let implemented = invoker
        .names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

    assert_eq!(implemented, declared);
}
