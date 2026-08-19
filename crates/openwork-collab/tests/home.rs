use openwork_collab::{home::HomeManager, model::Agent};

#[tokio::test]
async fn startup_repair_overwrites_managed_files_but_preserves_memory() {
    let root = tempfile::tempdir().unwrap();
    let manager = HomeManager::new(root.path(), "http://127.0.0.1:1234/mcp");
    let agent = Agent {
        id: "alice".to_string(),
        display_name: "Alice".to_string(),
        role: Some("Researcher".to_string()),
        bio: None,
        system_prompt: "PROMPT_V2".to_string(),
        provider_id: "opencode".to_string(),
        model_id: "hy3-free".to_string(),
        opencode_session_id: None,
        enabled: true,
        scanner_enabled: false,
    };

    let home = manager.repair(&agent, "token-v2").await.unwrap();
    tokio::fs::write(home.join("opencode.json"), "broken")
        .await
        .unwrap();
    tokio::fs::write(home.join("AGENTS.md"), "broken")
        .await
        .unwrap();
    tokio::fs::write(home.join("memory/MEMORY.md"), "keep me")
        .await
        .unwrap();

    manager.repair(&agent, "token-v2").await.unwrap();

    let config: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(home.join("opencode.json")).await.unwrap())
            .unwrap();
    assert_eq!(config["agent"]["alice"]["prompt"], "PROMPT_V2");
    assert_eq!(config["agent"]["alice"]["mode"], "primary");
    assert_eq!(
        config["agent"]["alice"]["permission"],
        serde_json::json!({})
    );
    assert_eq!(
        config["mcp"]["openwork"]["headers"]["X-OpenWork-Token"],
        "token-v2"
    );
    assert!(
        tokio::fs::read_to_string(home.join("AGENTS.md"))
            .await
            .unwrap()
            .contains("PROMPT_V2")
    );
    let standing_prompt = tokio::fs::read_to_string(home.join("AGENTS.md"))
        .await
        .unwrap();
    for rule in [
        "When a human names a teammate, check who was named",
        "Reply from real published state",
        "Send optimistically; the server is the safety net",
        "Do not repeat what a teammate already said",
        "Do not claim chat turns",
    ] {
        assert!(standing_prompt.contains(rule), "missing rule: {rule}");
    }
    assert_eq!(
        standing_prompt
            .lines()
            .filter(|line| {
                line.trim_start()
                    .split_once(". ")
                    .is_some_and(|(number, _)| number.parse::<u8>().is_ok())
            })
            .count(),
        5,
        "the standing coordination protocol must stay at exactly five numbered rules"
    );
    assert!(standing_prompt.contains("openwork_glance"));
    assert!(standing_prompt.contains("openwork_react"));
    assert!(standing_prompt.contains(
        "Put work teammates and users should see on the shared board; use session todo only for steps in your current turn."
    ));
    assert_eq!(
        tokio::fs::read_to_string(home.join("memory/MEMORY.md"))
            .await
            .unwrap(),
        "keep me"
    );
}
