use std::fs;

use openwork_core::skills::{SkillRoots, SkillSource, discover_skills};
use uuid::Uuid;

#[test]
fn discovery_uses_only_the_agents_user_root() {
    let root = std::env::temp_dir().join(format!(
        "openwork-user-skill-sources-{}",
        Uuid::new_v4().simple()
    ));
    let agents = root.join("home/.agents/skills");
    let claude = root.join("home/.claude/skills");
    let project = root.join("workspace/.openwork/skills");
    write_skill(&agents, "commit", "Agents commit workflow.");
    write_skill(&claude, "commit", "Claude commit workflow.");
    write_skill(&claude, "review", "Claude review workflow.");
    write_skill(&project, "project-only", "Must not be discovered.");

    let discovery = discover_skills(&SkillRoots {
        agents: Some(agents),
    });

    assert_eq!(
        discovery
            .skills
            .iter()
            .map(|skill| (skill.source, skill.name.as_str()))
            .collect::<Vec<_>>(),
        [(SkillSource::Agents, "commit")]
    );
    assert!(discovery.warnings.is_empty());
    assert!(
        discovery
            .skills
            .iter()
            .all(|skill| skill.name != "project-only")
    );

    fs::remove_dir_all(root).expect("cleanup");
}

fn write_skill(root: &std::path::Path, name: &str, description: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("skill directory");
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nBody\n"),
    )
    .expect("SKILL.md");
}
