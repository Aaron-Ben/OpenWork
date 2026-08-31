use std::path::{Path, PathBuf};

#[test]
fn r3_has_one_local_engine_path_and_separate_server_computer_facades() {
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
fn r3_has_no_obsolete_identity_transport_or_cli_path() {
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
            "obsolete R3 source path survived: {forbidden}"
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
            "obsolete R3 schema survived: {forbidden}"
        );
    }
}

#[test]
fn r1_business_modules_still_own_persistence_without_a_universal_store() {
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
}

#[test]
fn r4_keeps_persistent_home_separate_from_runtime_credentials() {
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
            "R4 home shape is missing: {required}"
        );
    }
    let protocol = source_text(&crate_root.join("src/protocol"));
    assert!(!protocol.contains("active_agent_ids"));
    assert!(protocol.contains("engine_readiness"));
    assert!(protocol.contains("RunnerStatusView"));
}

#[test]
fn r5_keeps_agent_commands_typed_and_group_membership_user_owned() {
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
            "R5 AgentCommand is missing {required}"
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
fn r6_keeps_board_structure_user_owned_and_card_order_server_owned() {
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
            "R6 Agent Card surface is missing {required}"
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
