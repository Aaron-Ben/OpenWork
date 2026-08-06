use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use super::parser::{
    SkillDocument, SkillParseError, is_valid_name, parse_skill_document, skill_body,
};

const MAX_SKILL_MD_BYTES: usize = 64 * 1024;
const MAX_DISCOVERED_SKILLS: usize = 100;

/// The complete set of user-level compatibility roots scanned for skills.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillRoots {
    pub agents: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Agents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub source: SkillSource,
    pub name: String,
    pub description: String,
    pub path: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillWarning {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDiscovery {
    pub skills: Vec<SkillSummary>,
    pub warnings: Vec<SkillWarning>,
}

/// One skill with its full markdown body, for the Desktop detail view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub source: SkillSource,
    pub name: String,
    pub description: String,
    pub path: String,
    pub body: String,
}

#[derive(Debug, Error)]
pub enum SkillReadError {
    #[error("path is not a SKILL.md directly inside a configured skill root")]
    OutsideSkillRoots,
    #[error(transparent)]
    Load(#[from] SkillLoadError),
}

/// Reads one skill by absolute path. The path must canonicalize to a
/// `SKILL.md` whose parent directory sits directly inside a configured skill
/// root — the same shape discovery accepts — so the Desktop can only ever
/// read files the agent could also reach.
pub fn read_skill(skill_roots: &SkillRoots, path: &str) -> Result<SkillDetail, SkillReadError> {
    let canonical = fs::canonicalize(path).map_err(|source| SkillLoadError::Inspect { source })?;
    if canonical.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
        return Err(SkillReadError::OutsideSkillRoots);
    }
    let Some(skill_directory) = canonical.parent() else {
        return Err(SkillReadError::OutsideSkillRoots);
    };
    let Some(directory_name) = skill_directory.file_name().and_then(|name| name.to_str()) else {
        return Err(SkillReadError::OutsideSkillRoots);
    };
    if !is_valid_name(directory_name) {
        return Err(SkillReadError::OutsideSkillRoots);
    }
    let Some(root_candidate) = skill_directory.parent() else {
        return Err(SkillReadError::OutsideSkillRoots);
    };

    let mut matched_source = None;
    if let Some(root) = &skill_roots.agents
        && fs::canonicalize(root).is_ok_and(|root| root == root_candidate)
    {
        matched_source = Some(SkillSource::Agents);
    }
    let Some(source) = matched_source else {
        return Err(SkillReadError::OutsideSkillRoots);
    };

    let (document, raw_source) = load_skill(directory_name, &canonical)?;
    Ok(SkillDetail {
        source,
        name: document.name,
        description: document.description,
        path: canonical
            .to_str()
            .expect("validated UTF-8 skill path")
            .to_string(),
        body: skill_body(&raw_source).to_string(),
    })
}

pub fn discover_skills(skill_roots: &SkillRoots) -> SkillDiscovery {
    let mut discovery = SkillDiscovery::default();
    let mut loaded_count = 0;
    if let Some(root) = &skill_roots.agents {
        scan_root(root, SkillSource::Agents, &mut discovery, &mut loaded_count);
    }
    sort_skills(&mut discovery.skills);
    discovery
}

fn sort_skills(skills: &mut [SkillSummary]) {
    skills.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn scan_root(
    root: &Path,
    source: SkillSource,
    discovery: &mut SkillDiscovery,
    loaded_count: &mut usize,
) {
    let root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            discovery.warnings.push(SkillWarning {
                path: root.to_string_lossy().into_owned(),
                reason: format!("failed to read skill root: {error}"),
            });
            return;
        }
    };
    let directory = match fs::read_dir(&root) {
        Ok(directory) => directory,
        Err(error) => {
            discovery.warnings.push(SkillWarning {
                path: root.to_string_lossy().into_owned(),
                reason: format!("failed to read skill root: {error}"),
            });
            return;
        }
    };
    let mut entries = Vec::new();
    for entry in directory {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                discovery.warnings.push(SkillWarning {
                    path: root.to_string_lossy().into_owned(),
                    reason: format!("failed to read skill root entry: {error}"),
                });
            }
        }
    }
    entries.sort_by_key(fs::DirEntry::file_name);

    let mut candidates = Vec::new();
    for entry in entries {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                discovery.warnings.push(SkillWarning {
                    path: entry.path().to_string_lossy().into_owned(),
                    reason: format!("failed to inspect skill root entry: {error}"),
                });
                continue;
            }
        };
        if !file_type.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        if file_name.as_encoded_bytes().first() == Some(&b'.') {
            continue;
        }
        let Some(directory_name) = file_name.to_str().map(str::to_string) else {
            discovery.warnings.push(SkillWarning {
                path: entry.path().to_string_lossy().into_owned(),
                reason: "skill directory name is not valid UTF-8".to_string(),
            });
            continue;
        };
        if !is_valid_name(&directory_name) {
            discovery.warnings.push(SkillWarning {
                path: entry.path().to_string_lossy().into_owned(),
                reason: "skill directory name does not match the required lowercase name format"
                    .to_string(),
            });
            continue;
        }
        candidates.push((directory_name, entry.path()));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));

    for (directory_name, skill_directory) in candidates {
        if *loaded_count >= MAX_DISCOVERED_SKILLS {
            discovery.warnings.push(SkillWarning {
                path: skill_directory.to_string_lossy().into_owned(),
                reason: "skill scan limit exceeded; only the first 100 skills are loaded"
                    .to_string(),
            });
            continue;
        }
        let skill_path = skill_directory.join("SKILL.md");
        if fs::symlink_metadata(&skill_path).is_ok_and(|metadata| metadata.is_symlink()) {
            discovery.warnings.push(SkillWarning {
                path: skill_path.to_string_lossy().into_owned(),
                reason: "SKILL.md symbolic links are not supported".to_string(),
            });
            continue;
        }
        match load_skill(&directory_name, &skill_path) {
            Ok((document, _)) => {
                *loaded_count += 1;
                discovery
                    .skills
                    .push(valid_summary(source, skill_path, document));
            }
            Err(error) => {
                discovery.warnings.push(SkillWarning {
                    path: skill_path.to_string_lossy().into_owned(),
                    reason: error.to_string(),
                });
            }
        }
    }
}

fn load_skill(
    directory_name: &str,
    skill_path: &Path,
) -> Result<(SkillDocument, String), SkillLoadError> {
    let skill_path_text = skill_path.to_str().ok_or(SkillLoadError::PathNotUtf8)?;
    if skill_path_text.chars().any(char::is_control) {
        return Err(SkillLoadError::PathContainsControlCharacter);
    }
    let metadata = fs::metadata(skill_path).map_err(|source| SkillLoadError::Inspect { source })?;
    ensure_skill_size(metadata.len())?;
    let mut bytes = Vec::with_capacity(MAX_SKILL_MD_BYTES + 1);
    fs::File::open(skill_path)
        .map_err(|source| SkillLoadError::Read { source })?
        .take((MAX_SKILL_MD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| SkillLoadError::Read { source })?;
    ensure_skill_size(bytes.len() as u64)?;
    let source = String::from_utf8(bytes).map_err(|_| SkillLoadError::NonUtf8)?;
    let document = parse_skill_document(directory_name, &source)?;
    Ok((document, source))
}

fn ensure_skill_size(actual: u64) -> Result<(), SkillLoadError> {
    if actual > MAX_SKILL_MD_BYTES as u64 {
        return Err(SkillLoadError::TooLarge {
            actual,
            maximum: MAX_SKILL_MD_BYTES as u64,
        });
    }
    Ok(())
}

fn valid_summary(
    source: SkillSource,
    skill_path: PathBuf,
    document: SkillDocument,
) -> SkillSummary {
    SkillSummary {
        source,
        name: document.name,
        description: document.description,
        path: skill_path
            .to_str()
            .expect("validated UTF-8 skill path")
            .to_string(),
        disabled: false,
    }
}

#[derive(Debug, Error)]
pub enum SkillLoadError {
    #[error("SKILL.md absolute path is not valid UTF-8")]
    PathNotUtf8,
    #[error("SKILL.md absolute path must not contain control characters")]
    PathContainsControlCharacter,
    #[error("failed to inspect SKILL.md: {source}")]
    Inspect { source: io::Error },
    #[error("SKILL.md is too large: {actual} bytes (maximum {maximum})")]
    TooLarge { actual: u64, maximum: u64 },
    #[error("failed to read SKILL.md: {source}")]
    Read { source: io::Error },
    #[error("SKILL.md is not valid UTF-8")]
    NonUtf8,
    #[error(transparent)]
    Parse(#[from] SkillParseError),
}
