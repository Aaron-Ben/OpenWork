use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDocument {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SkillParseError {
    #[error("SKILL.md must start with YAML frontmatter delimited by --- lines")]
    MissingFrontmatter,
    #[error("SKILL.md YAML frontmatter is invalid: {0}")]
    InvalidFrontmatter(String),
    #[error("skill frontmatter is missing the required description field")]
    MissingDescription,
    #[error("skill name does not match the required lowercase name format: {0}")]
    InvalidName(String),
    #[error("skill frontmatter name {name:?} must match directory name {directory_name:?}")]
    NameDoesNotMatchDirectory {
        name: String,
        directory_name: String,
    },
    #[error("skill frontmatter field {field} must be {expected}")]
    InvalidFieldType {
        field: &'static str,
        expected: &'static str,
    },
}

/// The markdown body that follows the frontmatter block, with surrounding
/// blank lines removed. Empty when the source has no frontmatter.
pub fn skill_body(source: &str) -> &str {
    match split_frontmatter(source) {
        Ok((_, body)) => body.trim_start_matches(['\r', '\n']).trim_end(),
        Err(_) => "",
    }
}

pub fn parse_skill_document(
    directory_name: &str,
    source: &str,
) -> Result<SkillDocument, SkillParseError> {
    let (frontmatter, _) = split_frontmatter(source)?;
    let mut fields: BTreeMap<String, Value> = serde_saphyr::from_str(frontmatter)
        .map_err(|error| SkillParseError::InvalidFrontmatter(error.to_string()))?;

    let name = normalize_whitespace(
        &take_optional_string(&mut fields, "name")?.unwrap_or_else(|| directory_name.to_string()),
    );
    if !is_valid_name(&name) {
        return Err(SkillParseError::InvalidName(name));
    }
    if name != directory_name {
        return Err(SkillParseError::NameDoesNotMatchDirectory {
            name,
            directory_name: directory_name.to_string(),
        });
    }

    let description: String = normalize_whitespace(
        &take_optional_string(&mut fields, "description")?
            .ok_or(SkillParseError::MissingDescription)?,
    )
    .chars()
    .take(1024)
    .collect();

    Ok(SkillDocument { name, description })
}

pub(crate) fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn take_optional_string(
    fields: &mut BTreeMap<String, Value>,
    field: &'static str,
) -> Result<Option<String>, SkillParseError> {
    let Some(value) = fields.remove(field) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_string()))
        .ok_or(SkillParseError::InvalidFieldType {
            field,
            expected: "a string",
        })
}

fn split_frontmatter(source: &str) -> Result<(&str, &str), SkillParseError> {
    let Some((opening, frontmatter_start)) = next_line(source, 0) else {
        return Err(SkillParseError::MissingFrontmatter);
    };
    if opening != "---" {
        return Err(SkillParseError::MissingFrontmatter);
    }

    let mut line_start = frontmatter_start;
    while let Some((line, next_start)) = next_line(source, line_start) {
        if line == "---" {
            return Ok((
                &source[frontmatter_start..line_start],
                &source[next_start..],
            ));
        }
        if next_start == source.len() {
            break;
        }
        line_start = next_start;
    }
    Err(SkillParseError::MissingFrontmatter)
}

fn next_line(source: &str, start: usize) -> Option<(&str, usize)> {
    if start >= source.len() {
        return None;
    }
    let remainder = &source[start..];
    let (line, next_start) = match remainder.find('\n') {
        Some(newline) => (&remainder[..newline], start + newline + 1),
        None => (remainder, source.len()),
    };
    Some((line.strip_suffix('\r').unwrap_or(line), next_start))
}
