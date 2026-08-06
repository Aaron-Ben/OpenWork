use std::collections::{BTreeSet, HashSet};

use openwork_models::model::{Message, Role};
use thiserror::Error;

use crate::UserInput;

use super::{SkillRoots, discover_skills, read_skill};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillInjection {
    pub name: String,
    pub path: String,
    pub contents: String,
}

impl SkillInjection {
    pub(crate) fn into_message(self) -> Message {
        Message::text(
            Role::User,
            format!(
                "<skill>\n<name>{}</name>\n<path>{}</path>\n{}\n</skill>",
                self.name, self.path, self.contents
            ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("selected skill is unavailable: {name}")]
pub(crate) struct SkillSelectionError {
    name: String,
}

impl SkillSelectionError {
    fn unavailable(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Resolves the exact files selected by the Desktop into durable body
/// snapshots. Names are checked only as integrity metadata; paths identify the
/// selected files. Duplicate canonical paths are collapsed in input order.
pub(crate) fn resolve_selected_skills(
    skill_roots: &SkillRoots,
    disabled_skill_names: &BTreeSet<String>,
    input: &[UserInput],
) -> Result<Vec<SkillInjection>, SkillSelectionError> {
    let discovered = discover_skills(skill_roots);
    let available_paths: HashSet<_> = discovered
        .skills
        .into_iter()
        .filter(|skill| !disabled_skill_names.contains(&skill.name))
        .map(|skill| (skill.name, skill.path))
        .collect();
    let mut seen_paths = HashSet::with_capacity(input.len());
    let mut resolved = Vec::with_capacity(input.len());

    for item in input {
        let UserInput::Skill { name, path } = item else {
            continue;
        };
        let detail =
            read_skill(skill_roots, path).map_err(|_| SkillSelectionError::unavailable(name))?;
        if detail.name != *name
            || !available_paths.contains(&(detail.name.clone(), detail.path.clone()))
        {
            return Err(SkillSelectionError::unavailable(name));
        }
        if !seen_paths.insert(detail.path.clone()) {
            continue;
        }
        resolved.push(SkillInjection {
            name: detail.name,
            path: detail.path,
            contents: detail.body,
        });
    }

    Ok(resolved)
}
