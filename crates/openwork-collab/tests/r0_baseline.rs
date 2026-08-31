use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Baseline {
    version: u32,
    status: String,
    public_seams: Vec<String>,
    current_interfaces: CurrentInterfaces,
    behavior_coverage: Vec<BehaviorCoverage>,
    reset: ResetInventory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentInterfaces {
    desktop_control_transport: String,
    agent_command_transport: String,
    control_commands: Vec<String>,
    agent_commands: Vec<String>,
    http_routes: Vec<String>,
    sse_routes: Vec<String>,
    desktop_behavior: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BehaviorCoverage {
    behavior: String,
    test_file: String,
    test_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetInventory {
    dry_run_only: bool,
    postgres_tables: Vec<String>,
    redis_namespaces: Vec<String>,
    filesystem: FilesystemInventory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilesystemInventory {
    preserve: Vec<String>,
    review_before_delete: Vec<String>,
    regenerate: Vec<String>,
    retire_after_cutover: Vec<String>,
}

fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn load_baseline() -> Baseline {
    let path = crate_root().join("tests/fixtures/r0-baseline.json");
    let bytes = fs::read(&path)
        .unwrap_or_else(|error| panic!("R0 baseline must exist at {}: {error}", path.display()));
    serde_json::from_slice(&bytes).expect("R0 baseline must be valid JSON")
}

#[test]
fn r0_inventory_matches_the_current_implementation_without_authorizing_deletion() {
    let baseline = load_baseline();

    assert_eq!(baseline.version, 1);
    assert_eq!(baseline.status, "frozen-current-state");
    assert!(baseline.reset.dry_run_only, "R0 must never execute a reset");
    assert_eq!(
        baseline.public_seams,
        [
            "desktop-control-plane",
            "server-http-sse",
            "agent-shim",
            "computer-engine-process",
        ]
    );

    let expected_tables = migration_tables();
    let declared_tables = baseline
        .reset
        .postgres_tables
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(declared_tables, expected_tables);
    assert_eq!(
        baseline.reset.postgres_tables.len(),
        declared_tables.len(),
        "PostgreSQL inventory must not contain duplicates"
    );

    let redis_sources = rust_source_tree(&crate_root().join("src"));
    for line in redis_sources
        .lines()
        .filter(|line| line.contains("openwork:"))
    {
        assert!(
            baseline
                .reset
                .redis_namespaces
                .iter()
                .any(|namespace| line.contains(namespace)),
            "Redis use is missing from the R0 inventory: {line}"
        );
    }
    for namespace in &baseline.reset.redis_namespaces {
        assert!(
            redis_sources.contains(namespace),
            "declared Redis namespace is not used: {namespace}"
        );
    }

    assert!(
        baseline
            .reset
            .filesystem
            .preserve
            .contains(&"~/.openwork/agents/*/work/**".to_string())
    );
    assert!(
        baseline
            .reset
            .filesystem
            .preserve
            .contains(&"~/.openwork/computer/agents/*/workspace/**".to_string()),
        "legacy Agent workspace data must survive the cutover review"
    );
    assert!(
        baseline
            .reset
            .filesystem
            .review_before_delete
            .iter()
            .any(|path| path.contains("memory"))
    );
    assert!(!baseline.reset.filesystem.regenerate.is_empty());
    assert!(!baseline.reset.filesystem.retire_after_cutover.is_empty());
}

#[test]
fn r0_interfaces_and_behavior_coverage_point_to_real_public_boundary_tests() {
    let baseline = load_baseline();

    assert_eq!(
        baseline.current_interfaces.desktop_control_transport,
        "Unix socket JSON ControlRequest/ControlResponse"
    );
    assert_eq!(
        baseline.current_interfaces.agent_command_transport,
        "HTTP POST /runtime/cli with raw argv"
    );

    let runtime = fs::read_to_string(crate_root().join("src/server/runtime.rs")).unwrap();
    assert_eq!(
        baseline
            .current_interfaces
            .http_routes
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        runtime_routes(&runtime)
    );
    assert_eq!(
        baseline.current_interfaces.sse_routes,
        ["GET /runtime/wake-stream"]
    );
    assert_eq!(
        baseline
            .current_interfaces
            .control_commands
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        control_commands()
    );
    assert_eq!(
        baseline.current_interfaces.agent_commands,
        [
            "inbox",
            "glance",
            "reply",
            "ack",
            "dm",
            "react",
            "group create",
            "group invite",
            "group leave",
            "group kick",
            "card list",
            "card create",
            "card claim",
            "card move",
        ]
    );
    assert_eq!(
        baseline.current_interfaces.desktop_behavior,
        [
            "Desktop installs and reconciles Server and Computer launchd jobs",
            "launchd keeps Server and Computer alive after Desktop exits",
            "Desktop does not yet own or stop the child-process group",
        ]
    );

    let behaviors = baseline
        .behavior_coverage
        .iter()
        .map(|coverage| coverage.behavior.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        behaviors,
        BTreeSet::from([
            "agent-creation",
            "room-message-and-inbox-settlement",
            "held-reply",
            "card-claim-and-agenda-focus",
            "computer-opencode-shim-round-trip",
            "current-server-shutdown-handoff",
            "current-desktop-startup-handoff-decision",
            "current-launchd-unload-wait",
        ])
    );
    for coverage in &baseline.behavior_coverage {
        let path = crate_root().join(&coverage.test_file);
        let test = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("coverage test file {} is missing: {error}", path.display())
        });
        assert!(
            test.contains(&format!("fn {}(", coverage.test_name)),
            "{} does not contain test {}",
            coverage.test_file,
            coverage.test_name
        );
    }
}

fn migration_tables() -> BTreeSet<String> {
    let mut tables = BTreeSet::from(["collab_schema_migrations".to_string()]);
    let migrations = crate_root().join("migrations");
    for entry in fs::read_dir(migrations).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "sql") {
            continue;
        }
        for line in fs::read_to_string(path).unwrap().lines() {
            let Some(rest) = line.trim().strip_prefix("CREATE TABLE ") else {
                continue;
            };
            let table = rest
                .strip_prefix("IF NOT EXISTS ")
                .unwrap_or(rest)
                .split_whitespace()
                .next()
                .unwrap()
                .trim_end_matches('(');
            assert!(table.starts_with("collab_"));
            assert!(tables.insert(table.to_string()), "duplicate table {table}");
        }
    }
    tables
}

fn runtime_routes(source: &str) -> BTreeSet<String> {
    let mut routes = BTreeSet::new();
    let mut remaining = source;
    while let Some(route_start) = remaining.find(".route(") {
        remaining = &remaining[route_start + ".route(".len()..];
        let quote_start = remaining.find('"').expect("route has a string path") + 1;
        let after_start = &remaining[quote_start..];
        let quote_end = after_start.find('"').expect("route path is terminated");
        let path = &after_start[..quote_end];
        let route_tail = &after_start[quote_end + 1..];
        let next_route = route_tail.find(".route(").unwrap_or(route_tail.len());
        let with_state = route_tail.find(".with_state").unwrap_or(route_tail.len());
        let route_expression = &route_tail[..next_route.min(with_state)];
        let method = if route_expression.contains("get(") {
            "GET"
        } else if route_expression.contains("post(") {
            "POST"
        } else {
            panic!("R0 route parser does not recognize handler for {path}")
        };
        assert!(routes.insert(format!("{method} {path}")));
        remaining = route_tail;
    }
    routes
}

fn control_commands() -> BTreeSet<String> {
    let protocol = fs::read_to_string(crate_root().join("src/protocol.rs")).unwrap();
    let body = protocol
        .split_once("pub enum ControlRequest {")
        .expect("ControlRequest exists")
        .1
        .split_once("pub enum ControlResponse")
        .expect("ControlResponse follows ControlRequest")
        .0;
    body.lines()
        .filter_map(|line| {
            let variant = line.strip_prefix("    ")?;
            if variant.starts_with(' ') || !variant.chars().next().is_some_and(char::is_uppercase) {
                return None;
            }
            let name = variant.split(['{', ',']).next().unwrap().trim();
            (!name.is_empty()).then(|| snake_case(name))
        })
        .collect()
}

fn snake_case(name: &str) -> String {
    let mut result = String::new();
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            if !result.is_empty() {
                result.push('_');
            }
            result.push(character.to_ascii_lowercase());
        } else {
            result.push(character);
        }
    }
    result
}

fn rust_source_tree(root: &Path) -> String {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(fs::read_to_string(path).unwrap());
            }
        }
    }
    sources.join("\n")
}
