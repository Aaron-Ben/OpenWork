use std::collections::BTreeSet;

use openwork_models::model::ContentBlock;

use crate::skills::{SkillDiscovery, SkillRoots, SkillWarning, discover_skills};

use super::SystemContextPart;

const SKILL_CATALOG_KEY: &str = "skills/catalog";
const MAX_CATALOG_CHARS: usize = 8000;
const CATALOG_HEADER: &str = "<available_skills>\n\
Skill 是一份放在 SKILL.md 里的操作指令。下面是本会话可用的全部 skill。\n\n";
const CATALOG_FOOTER: &str = "\n\
任务落在某条描述的场景里时，先用 read 完整读它的 path 再动手，不要凭描述猜正文。\n\
SKILL.md 里的相对路径相对它所在目录解析。有 scripts/ 就跑现成脚本，不要重写等价代码。\n\
选中的指令文件要读完；不相关的 references/ 不要读。\n\
</available_skills>";

#[derive(Clone)]
pub(crate) struct SkillCatalogLoader {
    skill_roots: SkillRoots,
    disabled_names: BTreeSet<String>,
}

impl SkillCatalogLoader {
    pub(crate) fn new(skill_roots: SkillRoots) -> Self {
        Self {
            skill_roots,
            disabled_names: BTreeSet::new(),
        }
    }

    pub(crate) fn with_disabled_names(mut self, disabled_names: BTreeSet<String>) -> Self {
        self.disabled_names = disabled_names;
        self
    }

    pub(crate) fn load(&self) -> (Option<SystemContextPart>, Vec<SkillWarning>) {
        let mut discovery = discover_skills(&self.skill_roots);
        apply_disabled_names(&mut discovery, &self.disabled_names);
        render_skill_catalog(discovery)
    }
}

pub(crate) fn list_skills(
    skill_roots: &SkillRoots,
    disabled_names: &BTreeSet<String>,
) -> SkillDiscovery {
    let mut discovery = discover_skills(skill_roots);
    apply_disabled_names(&mut discovery, disabled_names);
    let (_, warnings) = render_skill_catalog(discovery.clone());
    SkillDiscovery {
        skills: discovery.skills,
        warnings,
    }
}

fn apply_disabled_names(discovery: &mut SkillDiscovery, disabled_names: &BTreeSet<String>) {
    for skill in &mut discovery.skills {
        skill.disabled = disabled_names.contains(&skill.name);
    }
}

pub(crate) fn render_skill_catalog(
    discovery: SkillDiscovery,
) -> (Option<SystemContextPart>, Vec<SkillWarning>) {
    let SkillDiscovery {
        skills,
        mut warnings,
    } = discovery;
    let active: Vec<_> = skills.into_iter().filter(|skill| !skill.disabled).collect();
    if active.is_empty() {
        return (None, warnings);
    }

    let mut lines: Vec<_> = active
        .into_iter()
        .map(|skill| {
            let line = format!(
                "- {}: {} (file: {})\n",
                skill.name, skill.description, skill.path
            );
            (skill.path, line)
        })
        .collect();
    let mut catalog_chars = CATALOG_HEADER.chars().count()
        + CATALOG_FOOTER.chars().count()
        + lines
            .iter()
            .map(|(_, line)| line.chars().count())
            .sum::<usize>();
    while catalog_chars > MAX_CATALOG_CHARS {
        let Some((path, line)) = lines.pop() else {
            break;
        };
        catalog_chars -= line.chars().count();
        warnings.push(SkillWarning {
            path,
            reason: format!(
                "skill omitted from catalog because the {MAX_CATALOG_CHARS} character limit was exceeded"
            ),
        });
    }
    if lines.is_empty() {
        return (None, warnings);
    }

    let mut catalog = String::with_capacity(catalog_chars);
    catalog.push_str(CATALOG_HEADER);
    for (_, line) in lines {
        catalog.push_str(&line);
    }
    catalog.push_str(CATALOG_FOOTER);
    (
        Some(SystemContextPart::new(
            SKILL_CATALOG_KEY,
            vec![ContentBlock::text(catalog)],
        )),
        warnings,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::skills::{SkillDiscovery, SkillSource, SkillSummary};

    use super::*;

    #[test]
    fn catalog_budget_drops_whole_skills_in_reverse_scan_order_and_warns() {
        let skills = (0..8)
            .map(|index| SkillSummary {
                source: SkillSource::Agents,
                name: format!("skill-{index}"),
                description: "界".repeat(1024),
                path: format!("/tmp/skill-{index}/SKILL.md"),
                disabled: false,
            })
            .collect();
        let discovery = SkillDiscovery {
            skills,
            warnings: Vec::new(),
        };

        let (first_part, first_warnings) = render_skill_catalog(discovery.clone());
        let (second_part, second_warnings) = render_skill_catalog(discovery);

        assert_eq!(first_part, second_part);
        assert_eq!(first_warnings, second_warnings);
        let part = first_part.expect("bounded catalog");
        let ContentBlock::Text(text) = &part.content[0] else {
            panic!("catalog must be text")
        };
        assert!(text.text.chars().count() <= 8000);
        assert!(text.text.contains("- skill-6:"));
        assert!(!text.text.contains("- skill-7:"));
        assert_eq!(first_warnings.len(), 1);
        assert_eq!(first_warnings[0].path, "/tmp/skill-7/SKILL.md");
        assert!(first_warnings[0].reason.contains("8000"));
    }

    #[test]
    fn catalog_keeps_scan_order() {
        let discovery = SkillDiscovery {
            skills: vec![
                SkillSummary {
                    source: SkillSource::Agents,
                    name: "commit".to_string(),
                    description: "Agents commit.".to_string(),
                    path: "/home/.agents/skills/commit/SKILL.md".to_string(),
                    disabled: false,
                },
                SkillSummary {
                    source: SkillSource::Agents,
                    name: "review".to_string(),
                    description: "Review changes.".to_string(),
                    path: "/home/.agents/skills/review/SKILL.md".to_string(),
                    disabled: false,
                },
            ],
            warnings: Vec::new(),
        };

        let (part, warnings) = render_skill_catalog(discovery);

        assert!(warnings.is_empty());
        let part = part.expect("catalog");
        let ContentBlock::Text(text) = &part.content[0] else {
            panic!("catalog must be text")
        };
        assert!(text.text.contains("- commit: Agents commit."));
        assert!(text.text.contains("- review: Review changes."));
        assert!(
            text.text.find("- commit:").expect("commit row")
                < text.text.find("- review:").expect("review row")
        );
    }

    #[test]
    fn catalog_omits_disabled_skills_without_removing_them_from_discovery() {
        let disabled = SkillSummary {
            source: SkillSource::Agents,
            name: "commit".to_string(),
            description: "Create a commit.".to_string(),
            path: "/home/.agents/skills/commit/SKILL.md".to_string(),
            disabled: true,
        };
        let enabled = SkillSummary {
            source: SkillSource::Agents,
            name: "review".to_string(),
            description: "Review changes.".to_string(),
            path: "/home/.agents/skills/review/SKILL.md".to_string(),
            disabled: false,
        };
        let discovery = SkillDiscovery {
            skills: vec![disabled.clone(), enabled],
            warnings: Vec::new(),
        };

        let (part, warnings) = render_skill_catalog(discovery.clone());

        assert!(warnings.is_empty());
        assert_eq!(discovery.skills[0], disabled);
        let part = part.expect("enabled skill catalog");
        let ContentBlock::Text(text) = &part.content[0] else {
            panic!("catalog must be text")
        };
        assert!(!text.text.contains("- commit:"));
        assert!(text.text.contains("- review:"));
    }

    #[test]
    fn skill_listing_and_turn_rendering_produce_identical_warnings() {
        let root = std::env::temp_dir().join(format!(
            "openwork-skill-listing-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let agents_root = root.join(".agents/skills");
        for index in 0..8 {
            let name = format!("skill-{index}");
            let directory = agents_root.join(&name);
            fs::create_dir_all(&directory).expect("skill directory");
            fs::write(
                directory.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: {}\n---\nBody\n",
                    "界".repeat(1024)
                ),
            )
            .expect("skill");
        }
        let invalid = agents_root.join("invalid");
        fs::create_dir_all(&invalid).expect("invalid directory");
        fs::write(invalid.join("SKILL.md"), "---\nname: invalid\n---\nBody\n")
            .expect("invalid skill");

        let roots = SkillRoots {
            agents: Some(agents_root),
        };
        let listing = list_skills(&roots, &BTreeSet::new());
        let (_, turn_warnings) = SkillCatalogLoader::new(roots).load();

        assert_eq!(listing.warnings, turn_warnings);
        assert_eq!(listing.skills.len(), 8);
        assert!(
            listing
                .warnings
                .iter()
                .any(|warning| warning.reason.contains("description"))
        );
        assert!(
            listing
                .warnings
                .iter()
                .any(|warning| warning.reason.contains("8000"))
        );

        fs::remove_dir_all(root).expect("cleanup");
    }
}
