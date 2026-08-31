use std::path::{Path, PathBuf};

#[test]
fn one_local_engine_path_keeps_separate_server_and_computer_facades() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(crate_root.join("Cargo.toml")).unwrap();
    for dependency in [
        "rmcp",
        "openwork-core",
        "openwork-credentials",
        "openwork-models",
    ] {
        assert!(
            !manifest.contains(dependency),
            "legacy collaboration dependency survived: {dependency}"
        );
    }

    let computer = source_text(&crate_root.join("src/computer"));
    assert!(!computer.contains("sqlx::"));
    assert!(!computer.contains("redis::"));
    let server = source_text(&crate_root.join("src/server"));
    assert!(!server.contains("Command::new"));
    assert!(crate_root.join("src/protocol/desktop.rs").exists());
    assert!(crate_root.join("src/protocol/computer.rs").exists());
    assert!(crate_root.join("src/protocol/agent.rs").exists());
}

#[test]
fn product_has_no_retired_identity_transport_or_cli_path() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for removed in [
        "src/launchd.rs",
        "src/server/control.rs",
        "src/server/runtime.rs",
        "src/server/computers.rs",
    ] {
        assert!(!crate_root.join(removed).exists(), "{removed} still exists");
    }

    let source = source_text(&crate_root.join("src"));
    for forbidden in [
        "ControlRequest",
        "CliRequest",
        "/runtime/cli",
        "control.sock",
        "computer.json",
        "device_token",
        "computer_id",
        "generation:",
    ] {
        assert!(
            !source.contains(forbidden),
            "retired collaboration source path survived: {forbidden}"
        );
    }

    let migrations = source_files(&crate_root.join("migrations"), "sql");
    for forbidden in [
        "CREATE TABLE collab_computers",
        "computer_id",
        "device_token",
        "claimed_by",
        "claimed_at",
        "collab_reactions",
        "collab_events",
    ] {
        assert!(
            !migrations.contains(forbidden),
            "retired collaboration schema survived: {forbidden}"
        );
    }
}

#[test]
fn business_modules_own_persistence_without_a_universal_store() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let server_root = crate_root.join("src/server");
    assert!(!server_root.join("storage.rs").exists());
    assert!(!source_text(&server_root).contains("CollaborationStore"));
    for adapter in ["transport.rs", "scheduler.rs", "runtime_session.rs"] {
        let source = std::fs::read_to_string(server_root.join(adapter)).unwrap();
        assert!(
            !source.contains("sqlx::query"),
            "transport/orchestration adapter owns business SQL: {adapter}"
        );
    }
    let commands = std::fs::read_to_string(server_root.join("agent_commands.rs")).unwrap();
    for forbidden in ["sqlx::query", "FromRow", "struct MessageRow", "INSERT INTO"] {
        assert!(
            !commands.contains(forbidden),
            "AgentCommands still owns business persistence: {forbidden}"
        );
    }
    for owner in ["command_requests.rs", "messages.rs", "rooms.rs", "runs.rs"] {
        assert!(
            server_root.join(owner).exists(),
            "SQL owner is missing: {owner}"
        );
    }
}

#[test]
fn engine_selection_is_server_neutral_and_computer_registry_owned() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let agents = std::fs::read_to_string(crate_root.join("src/server/agents.rs")).unwrap();
    let inventory = std::fs::read_to_string(crate_root.join("src/server/inventory.rs")).unwrap();
    let daemon = std::fs::read_to_string(crate_root.join("src/computer/daemon.rs")).unwrap();
    for source in [&agents, &inventory, &daemon] {
        assert!(!source.contains("engine_id != \"opencode\""));
        assert!(!source.contains("EngineId::opencode()"));
    }
    assert!(crate_root.join("src/protocol/engine.rs").exists());
    assert!(daemon.contains("self.engines.adapters()"));
    assert!(daemon.contains("engine_runnable(inventory, &assignment.engine_id)"));
}

#[test]
fn all_sse_consumers_share_the_protocol_reconnect_loop() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_root.parent().unwrap().parent().unwrap();
    assert!(crate_root.join("src/protocol/sse.rs").exists());
    assert!(!crate_root.join("src/computer/sse.rs").exists());

    let collab = source_text(&crate_root.join("src"));
    let desktop_path = workspace_root.join("desktop/src-tauri/src/collab_client.rs");
    let desktop = std::fs::read_to_string(desktop_path).unwrap();
    assert!(!desktop.contains("computer::"));
    assert!(desktop.contains("reconnecting_invalidation_loop"));
    assert_eq!(
        collab
            .matches("async fn reconnecting_invalidation_loop")
            .count(),
        1,
        "SSE reconnect loop must have one implementation"
    );
}

#[test]
fn persistent_home_stays_separate_from_runtime_credentials() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let home = std::fs::read_to_string(crate_root.join("src/computer/home.rs")).unwrap();
    let implementation = home.split("#[cfg(all(test").next().unwrap();
    for obsolete in [
        "join(\"memory\")",
        "join(\"notes\")",
        "join(\"skills\")",
        "join(\"workspace\")",
        ".openwork-standing-prompt.md",
        ".runtime-token",
    ] {
        assert!(
            !implementation.contains(obsolete),
            "obsolete persistent-home shape survived: {obsolete}"
        );
    }
    for required in [
        "join(\"work\")",
        "join(\"engines\")",
        "join(\"runtime-token\")",
        "join(\"derived\")",
        "join(\"session.json\")",
    ] {
        assert!(
            implementation.contains(required),
            "Agent home shape is missing: {required}"
        );
    }
    let protocol = source_text(&crate_root.join("src/protocol"));
    assert!(!protocol.contains("active_agent_ids"));
    assert!(protocol.contains("engine_readiness"));
    assert!(protocol.contains("RunnerStatusView"));
}

#[test]
fn agent_commands_are_typed_and_group_membership_is_user_owned() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let agent_protocol = std::fs::read_to_string(crate_root.join("src/protocol/agent.rs")).unwrap();
    let shim = std::fs::read_to_string(crate_root.join("src/computer/shim.rs")).unwrap();
    let climate = std::fs::read_to_string(crate_root.join("src/server/climate.rs")).unwrap();

    for required in [
        "Rooms",
        "Messages",
        "Members",
        "Participants",
        "ClimateShow",
        "ClimateNote",
    ] {
        assert!(
            agent_protocol.contains(required),
            "AgentCommand is missing {required}"
        );
    }
    for forbidden in ["GroupCreate", "GroupInvite", "GroupLeave", "GroupKick"] {
        assert!(
            !agent_protocol.contains(forbidden),
            "Agent protocol can mutate Group membership: {forbidden}"
        );
    }
    assert!(!shim.contains("openwork group"));
    assert!(climate.contains("collab_agent_climates"));
    for forbidden in ["climate_history", "collab_climate_events"] {
        assert!(
            !source_text(&crate_root.join("src")).contains(forbidden),
            "Climate history/event path survived: {forbidden}"
        );
    }
}

#[test]
fn board_structure_is_user_owned_and_card_order_is_server_owned() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let agent_protocol = std::fs::read_to_string(crate_root.join("src/protocol/agent.rs")).unwrap();
    for required in [
        "BoardShow",
        "CardShow",
        "CardCreate",
        "CardClaim",
        "CardAssign",
        "CardUpdate",
        "CardMove",
    ] {
        assert!(
            agent_protocol.contains(required),
            "Agent Card surface is missing {required}"
        );
    }
    for forbidden in [
        "DeleteBoard",
        "CreateBoardColumn",
        "UpdateBoardColumn",
        "MoveBoardColumn",
        "DeleteBoardColumn",
        "DeleteCard",
    ] {
        assert!(
            !agent_protocol.contains(forbidden),
            "Agent protocol owns Desktop Board structure: {forbidden}"
        );
    }
    let board = std::fs::read_to_string(crate_root.join("src/server/board.rs")).unwrap();
    assert!(board.contains("SET CONSTRAINTS collab_cards_position_unique DEFERRED"));
    assert!(board.contains("ORDER BY id FOR UPDATE"));
    assert!(board.contains("before Card is not in target Column"));
    let migration = source_files(&crate_root.join("migrations"), "sql");
    assert!(migration.contains("is_terminal BOOLEAN NOT NULL"));
    assert!(migration.contains("UNIQUE (board_id, position) DEFERRABLE"));
    assert!(migration.contains("UNIQUE (column_id, position) DEFERRABLE"));
}

#[test]
fn owning_docs_and_product_sources_have_no_retired_collaboration_shape() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_root.parent().unwrap().parent().unwrap();
    let mut current = source_text(&crate_root.join("src"));
    current.push_str(&source_files(&crate_root.join("migrations"), "sql"));
    current.push_str(&std::fs::read_to_string(crate_root.join("README.md")).unwrap());
    for document in [
        "docs/architecture.md",
        "docs/collaboration.md",
        "docs/collaboration-desktop.md",
        "docs/collaboration-data-model.md",
    ] {
        current.push_str(&std::fs::read_to_string(workspace_root.join(document)).unwrap());
    }
    let normalized = current.to_ascii_lowercase();
    for forbidden in [
        "launchd",
        "control.sock",
        "collab_computers",
        "computer_id",
        "device_token",
        "daemon_generation",
        "system_prompt",
        "bio",
        "collab_reactions",
        "collab_events",
        "collab_agent_command_requests",
        "claimed_by",
        "claimed_at",
        "is_done",
        "memory/",
        "notes/",
        "skills/",
        "clirequest { argv }",
    ] {
        assert!(
            !normalized.contains(forbidden),
            "retired collaboration term survived in product or owning docs: {forbidden}"
        );
    }
    for removed in [
        "docs/collaboration-r0-baseline.md",
        "docs/collaboration-target-architecture.md",
        "crates/openwork-collab/tests/r0_baseline.rs",
        "crates/openwork-collab/tests/fixtures/r0-baseline.json",
    ] {
        assert!(
            !workspace_root.join(removed).exists(),
            "temporary collaboration artifact survived: {removed}"
        );
    }
}

fn source_text(root: &Path) -> String {
    source_files(root, "rs")
}

fn source_files(root: &Path, extension: &str) -> String {
    let mut text = String::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        } else if path
            .extension()
            .is_some_and(|candidate| candidate == extension)
        {
            text.push_str(&std::fs::read_to_string(path).unwrap());
        }
    }
    text
}
