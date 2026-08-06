use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{
    SkillDocument, SkillParseError, SkillReadError, SkillSource, discover_skills,
    parse_skill_document, read_skill, resolve_selected_skills, skill_body,
};

struct SkillFixture {
    root: PathBuf,
    agents: PathBuf,
}

impl SkillFixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("openwork-skills-{}", Uuid::new_v4().simple()));
        let agents = root.join("home/.agents/skills");
        fs::create_dir_all(&agents).expect("skill root");
        Self { root, agents }
    }

    fn write_skill(&self, root: &Path, directory_name: &str, source: impl AsRef<[u8]>) {
        let directory = root.join(directory_name);
        fs::create_dir_all(&directory).expect("skill directory");
        fs::write(directory.join("SKILL.md"), source).expect("SKILL.md");
    }

    fn discover(&self) -> super::SkillDiscovery {
        discover_skills(&super::SkillRoots {
            agents: Some(self.agents.clone()),
        })
    }
}

impl Drop for SkillFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn missing_name_uses_the_skill_directory_name() {
    let document = parse_skill_document(
        "commit",
        "---\ndescription: Create a commit when requested.\n---\nBody\n",
    )
    .expect("skill should parse");

    assert_eq!(document.name, "commit");
}

#[test]
fn missing_description_is_a_specific_parse_failure() {
    let error = parse_skill_document("commit", "---\nname: commit\n---\nBody\n")
        .expect_err("description is required");

    assert_eq!(error, SkillParseError::MissingDescription);
}

#[test]
fn name_whitespace_is_normalized_before_validation() {
    let document = parse_skill_document(
        "commit",
        "---\nname: \"  commit\\n\\t\"\ndescription: Create commits.\n---\nBody\n",
    )
    .expect("normalized name should be valid");

    assert_eq!(document.name, "commit");
}

#[test]
fn invalid_name_is_rejected_after_whitespace_normalization() {
    let error = parse_skill_document(
        "commit",
        "---\nname: \"  Invalid Name  \"\ndescription: Create commits.\n---\nBody\n",
    )
    .expect_err("normalized name must match the required format");

    assert_eq!(
        error,
        SkillParseError::InvalidName("Invalid Name".to_string())
    );
}

#[test]
fn frontmatter_name_must_match_the_skill_directory_name() {
    let error = parse_skill_document(
        "commit-workflow",
        "---\nname: commit\ndescription: Create commits.\n---\nBody\n",
    )
    .expect_err("frontmatter name must match the directory name");

    assert_eq!(
        error,
        SkillParseError::NameDoesNotMatchDirectory {
            name: "commit".to_string(),
            directory_name: "commit-workflow".to_string(),
        }
    );
}

#[test]
fn description_whitespace_is_normalized_instead_of_rejected() {
    let document = parse_skill_document(
        "commit",
        "---\nname: commit\ndescription: |-\n  Create   commits.\n  \tUse when changes are ready.\n---\nBody\n",
    )
    .expect("whitespace is normalized");

    assert_eq!(
        document.description,
        "Create commits. Use when changes are ready."
    );
}

#[test]
fn description_is_truncated_to_1024_characters() {
    let description = format!("{} trailing", "界".repeat(1025));
    let source = format!("---\nname: commit\ndescription: {description}\n---\nBody\n");

    let document = parse_skill_document("commit", &source).expect("description is truncated");

    assert_eq!(document.description, "界".repeat(1024));
}

#[test]
fn unknown_frontmatter_keys_are_ignored_by_the_two_field_parser() {
    let document = parse_skill_document(
        "review",
        "---\nname: review\ndescription: Review changes when requested.\nuser-invocable: not-a-bool\nmetadata:\n  owner: team-a\n---\nBody\n",
    )
    .expect("unknown keys do not affect loading");

    let SkillDocument { name, description } = document;
    assert_eq!(name, "review");
    assert_eq!(description, "Review changes when requested.");
}

#[test]
fn invalid_skill_only_adds_a_warning_and_does_not_hide_a_valid_sibling() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        "good",
        "---\nname: good\ndescription: Use for valid work.\n---\nBody\n",
    );
    fixture.write_skill(&fixture.agents, "bad", "---\nname: bad\n---\nBody\n");

    let discovery = fixture.discover();

    assert_eq!(discovery.skills.len(), 1);
    assert_eq!(discovery.skills[0].name, "good");
    assert_eq!(discovery.warnings.len(), 1);
    assert!(discovery.warnings[0].path.ends_with("bad/SKILL.md"));
    assert!(discovery.warnings[0].reason.contains("description"));
}

#[test]
fn invalid_skill_does_not_consume_the_hundred_skill_load_limit() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        "aaa-invalid",
        "---\nname: aaa-invalid\n---\nBody\n",
    );
    for index in 0..100 {
        let name = format!("skill-{index:03}");
        fixture.write_skill(
            &fixture.agents,
            &name,
            format!("---\nname: {name}\ndescription: Skill {index}.\n---\nBody\n"),
        );
    }

    let discovery = fixture.discover();

    assert_eq!(discovery.skills.len(), 100);
    assert_eq!(discovery.warnings.len(), 1);
    assert!(discovery.warnings[0].path.ends_with("aaa-invalid/SKILL.md"));
}

#[test]
fn a_mismatched_frontmatter_name_is_reported_as_a_warning() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        "commit-workflow",
        "---\nname: commit\ndescription: Commit workflow.\n---\nBody\n",
    );

    let discovery = fixture.discover();

    assert!(discovery.skills.is_empty());
    assert_eq!(discovery.warnings.len(), 1);
    assert!(
        discovery.warnings[0]
            .path
            .ends_with("commit-workflow/SKILL.md")
    );
    assert!(discovery.warnings[0].reason.contains("directory name"));
}

#[test]
fn invalid_directory_names_are_skipped_with_warnings() {
    let fixture = SkillFixture::new();
    let too_long = "a".repeat(65);
    for directory_name in ["Uppercase", "has space", "line\nbreak", &too_long] {
        fixture.write_skill(
            &fixture.agents,
            directory_name,
            "---\ndescription: Use for invalid directory tests.\n---\nBody\n",
        );
    }

    let discovery = fixture.discover();

    assert!(discovery.skills.is_empty());
    assert_eq!(discovery.warnings.len(), 4);
    assert!(discovery.warnings.iter().all(|warning| {
        warning.reason == "skill directory name does not match the required lowercase name format"
    }));
}

#[test]
fn file_size_encoding_and_control_path_failures_have_distinct_warnings() {
    let fixture = SkillFixture::new();
    fixture.write_skill(&fixture.agents, "oversized", vec![b'x'; 64 * 1024 + 1]);
    fixture.write_skill(&fixture.agents, "binary", [0xff, 0xfe]);
    let control_root = fixture.root.join("claude\troot");
    fs::create_dir_all(&control_root).expect("control root");
    fixture.write_skill(
        &control_root,
        "control-path",
        "---\ndescription: Use for path validation.\n---\nBody\n",
    );

    let discovery = fixture.discover();
    let control_discovery = discover_skills(&super::SkillRoots {
        agents: Some(control_root),
    });

    assert!(discovery.skills.is_empty());
    assert!(control_discovery.skills.is_empty());
    let reasons: Vec<_> = discovery
        .warnings
        .iter()
        .chain(control_discovery.warnings.iter())
        .map(|warning| warning.reason.as_str())
        .collect();
    assert!(reasons.contains(&"SKILL.md is too large: 65537 bytes (maximum 65536)"));
    assert!(reasons.contains(&"SKILL.md is not valid UTF-8"));
    assert!(reasons.contains(&"SKILL.md absolute path must not contain control characters"));
}

#[cfg(unix)]
#[test]
fn hidden_and_symlinked_skill_directories_are_not_followed() {
    use std::os::unix::fs::symlink;

    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        ".hidden",
        "---\nname: hidden\ndescription: Hidden.\n---\nBody\n",
    );
    fixture.write_skill(
        &fixture.agents,
        "visible",
        "---\nname: visible\ndescription: Visible.\n---\nBody\n",
    );
    let outside = fixture.root.join("outside");
    fixture.write_skill(
        &outside,
        "linked",
        "---\nname: linked\ndescription: Linked.\n---\nBody\n",
    );
    symlink(outside.join("linked"), fixture.agents.join("linked")).expect("directory symlink");

    let discovery = fixture.discover();

    assert_eq!(discovery.skills.len(), 1);
    assert_eq!(discovery.skills[0].name, "visible");
    assert!(discovery.warnings.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_skill_files_are_not_discovered() {
    use std::os::unix::fs::symlink;

    let fixture = SkillFixture::new();
    let target = fixture.root.join("linked-skill.md");
    fs::write(
        &target,
        "---\nname: linked-file\ndescription: Linked file.\n---\nBody\n",
    )
    .expect("target");
    let directory = fixture.agents.join("linked-file");
    fs::create_dir_all(&directory).expect("skill directory");
    symlink(&target, directory.join("SKILL.md")).expect("SKILL.md symlink");

    let discovery = fixture.discover();

    assert!(discovery.skills.is_empty());
    assert_eq!(discovery.warnings.len(), 1);
    assert!(discovery.warnings[0].path.ends_with("linked-file/SKILL.md"));
    assert_eq!(
        discovery.warnings[0].reason,
        "SKILL.md symbolic links are not supported"
    );
}

#[test]
fn scan_limit_keeps_the_first_hundred_and_warns_for_the_rest() {
    let fixture = SkillFixture::new();
    for index in 0..101 {
        let name = format!("skill-{index:03}");
        fixture.write_skill(
            &fixture.agents,
            &name,
            format!("---\nname: {name}\ndescription: Skill {index}.\n---\nBody\n"),
        );
    }

    let discovery = fixture.discover();

    assert_eq!(discovery.skills.len(), 100);
    assert_eq!(discovery.warnings.len(), 1);
    assert!(discovery.warnings[0].path.ends_with("skill-100"));
    assert_eq!(
        discovery.warnings[0].reason,
        "skill scan limit exceeded; only the first 100 skills are loaded"
    );
}

#[test]
fn read_skill_returns_the_body_without_frontmatter() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        "commit",
        "---\nname: commit\ndescription: Create commits.\n---\n\nIntro paragraph.\n\n## Section\n\nBody **text**.\n",
    );

    let path = fixture.agents.join("commit/SKILL.md");
    let detail = read_skill(
        &super::SkillRoots {
            agents: Some(fixture.agents.clone()),
        },
        path.to_str().expect("utf-8 path"),
    )
    .expect("skill should read");

    assert_eq!(detail.source, SkillSource::Agents);
    assert_eq!(detail.name, "commit");
    assert_eq!(detail.description, "Create commits.");
    assert!(detail.path.ends_with("commit/SKILL.md"));
    assert_eq!(
        detail.body,
        "Intro paragraph.\n\n## Section\n\nBody **text**."
    );
}

#[test]
fn read_skill_rejects_paths_outside_the_configured_roots() {
    let fixture = SkillFixture::new();
    let outside = fixture.root.join("outside");
    fixture.write_skill(
        &outside,
        "linked",
        "---\nname: linked\ndescription: Linked.\n---\nBody\n",
    );
    let roots = super::SkillRoots {
        agents: Some(fixture.agents.clone()),
    };

    let outside_path = outside.join("linked/SKILL.md");
    let result = read_skill(&roots, outside_path.to_str().expect("utf-8 path"));
    assert!(matches!(result, Err(SkillReadError::OutsideSkillRoots)));

    let nested = fixture.agents.join("container/nested/SKILL.md");
    fs::create_dir_all(nested.parent().expect("nested parent")).expect("nested directory");
    fs::write(
        &nested,
        "---\nname: nested\ndescription: Nested.\n---\nBody\n",
    )
    .expect("nested SKILL.md");
    let result = read_skill(&roots, nested.to_str().expect("utf-8 path"));
    assert!(matches!(result, Err(SkillReadError::OutsideSkillRoots)));

    let not_skill_md = fixture.agents.join("container/README.md");
    fs::write(&not_skill_md, "not a skill").expect("README.md");
    let result = read_skill(&roots, not_skill_md.to_str().expect("utf-8 path"));
    assert!(matches!(result, Err(SkillReadError::OutsideSkillRoots)));
}

#[test]
fn selected_skills_resolve_exact_paths_and_deduplicate_body_snapshots() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        "commit",
        "---\nname: commit\ndescription: Create commits.\n---\n\nCreate the requested commit.\n",
    );
    let path = fixture.agents.join("commit/SKILL.md");
    let selection = crate::UserInput::skill("commit", path.to_string_lossy());

    let resolved = resolve_selected_skills(
        &super::SkillRoots {
            agents: Some(fixture.agents.clone()),
        },
        &BTreeSet::new(),
        &[selection.clone(), selection],
    )
    .expect("selection should resolve");

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "commit");
    assert_eq!(resolved[0].contents, "Create the requested commit.");
    assert_eq!(
        resolved[0].path,
        fs::canonicalize(path)
            .expect("canonical path")
            .to_string_lossy()
    );
    assert_eq!(
        resolved[0].clone().into_message(),
        openwork_models::model::Message::text(
            openwork_models::model::Role::User,
            format!(
                "<skill>\n<name>commit</name>\n<path>{}</path>\nCreate the requested commit.\n</skill>",
                fs::canonicalize(fixture.agents.join("commit/SKILL.md"))
                    .expect("canonical path")
                    .to_string_lossy()
            )
        )
    );
}

#[test]
fn selected_skills_reject_disabled_or_mismatched_names() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents,
        "commit",
        "---\nname: commit\ndescription: Create commits.\n---\nBody\n",
    );
    let roots = super::SkillRoots {
        agents: Some(fixture.agents.clone()),
    };
    let path = fixture
        .agents
        .join("commit/SKILL.md")
        .to_string_lossy()
        .into_owned();

    let mismatched = resolve_selected_skills(
        &roots,
        &BTreeSet::new(),
        &[crate::UserInput::skill("review", path.clone())],
    )
    .expect_err("name mismatch must fail");
    assert_eq!(mismatched.name(), "review");

    let disabled = resolve_selected_skills(
        &roots,
        &BTreeSet::from(["commit".to_string()]),
        &[crate::UserInput::skill("commit", path)],
    )
    .expect_err("disabled skill must fail");
    assert_eq!(disabled.name(), "commit");
}

#[test]
fn selected_skills_reject_valid_files_outside_the_current_discovery_limit() {
    let fixture = SkillFixture::new();
    for index in 0..101 {
        let name = format!("skill-{index:03}");
        fixture.write_skill(
            &fixture.agents,
            &name,
            format!("---\nname: {name}\ndescription: Skill {index}.\n---\nBody\n"),
        );
    }
    let roots = super::SkillRoots {
        agents: Some(fixture.agents.clone()),
    };
    let unavailable = resolve_selected_skills(
        &roots,
        &BTreeSet::new(),
        &[crate::UserInput::skill(
            "skill-100",
            fixture.agents.join("skill-100/SKILL.md").to_string_lossy(),
        )],
    )
    .expect_err("a valid file outside the bounded discovery must not be selectable");

    assert_eq!(unavailable.name(), "skill-100");
}

#[test]
fn skill_body_strips_frontmatter_and_blank_edges() {
    assert_eq!(
        skill_body("---\nname: a\ndescription: A.\n---\n\nBody line.\n\n"),
        "Body line."
    );
    assert_eq!(skill_body("no frontmatter"), "");
    assert_eq!(skill_body("---\ndescription: A.\n---\n"), "");
}

#[test]
fn absent_user_root_is_silently_empty() {
    let fixture = SkillFixture::new();
    let without_root = discover_skills(&super::SkillRoots { agents: None });
    assert!(without_root.skills.is_empty());
    assert!(without_root.warnings.is_empty());

    let missing_root = discover_skills(&super::SkillRoots {
        agents: Some(fixture.root.join("missing-agents")),
    });
    assert!(missing_root.skills.is_empty());
    assert!(missing_root.warnings.is_empty());
}

#[test]
fn unreadable_user_root_warns() {
    let fixture = SkillFixture::new();
    let not_a_directory = fixture.root.join("not-a-directory");
    fs::write(&not_a_directory, "not a directory").expect("root-shaped file");

    let discovery = discover_skills(&super::SkillRoots {
        agents: Some(not_a_directory.clone()),
    });

    assert!(discovery.skills.is_empty());
    assert_eq!(discovery.warnings.len(), 1);
    assert_eq!(
        discovery.warnings[0].path,
        fs::canonicalize(&not_a_directory)
            .expect("canonical root-shaped file")
            .to_string_lossy()
    );
    assert!(discovery.warnings[0].reason.contains("skill root"));
}

#[test]
fn discovery_is_non_recursive_and_byte_stable() {
    let fixture = SkillFixture::new();
    fixture.write_skill(
        &fixture.agents.join("container"),
        "nested",
        "---\nname: nested\ndescription: Nested.\n---\nBody\n",
    );
    for (root, name) in [
        (&fixture.agents, "zulu"),
        (&fixture.agents, "alpha"),
        (&fixture.agents, "middle"),
    ] {
        fixture.write_skill(
            root,
            name,
            format!("---\nname: {name}\ndescription: Use {name}.\n---\nBody\n"),
        );
    }

    let first = fixture.discover();
    let second = fixture.discover();

    let ordered: Vec<_> = first
        .skills
        .iter()
        .map(|skill| (skill.source, skill.name.as_str()))
        .collect();
    assert_eq!(
        ordered,
        [
            (SkillSource::Agents, "alpha"),
            (SkillSource::Agents, "middle"),
            (SkillSource::Agents, "zulu"),
        ]
    );
    assert_eq!(
        serde_json::to_vec(&first).expect("first bytes"),
        serde_json::to_vec(&second).expect("second bytes")
    );
    assert!(!first.skills.iter().any(|skill| skill.name == "nested"));
}
