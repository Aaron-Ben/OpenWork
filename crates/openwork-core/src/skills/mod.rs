mod discovery;
mod parser;
mod selection;

pub use discovery::{
    SkillDetail, SkillDiscovery, SkillLoadError, SkillReadError, SkillRoots, SkillSource,
    SkillSummary, SkillWarning, discover_skills, read_skill,
};
pub use parser::{SkillDocument, SkillParseError, parse_skill_document, skill_body};
pub(crate) use selection::resolve_selected_skills;

#[cfg(test)]
mod tests;
