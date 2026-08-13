use openwork_models::model::ContentBlock;
use serde::Serialize;

mod budget;
mod builder;
mod engine;
mod inspection;
mod limits;
mod normalize;
mod project_instructions;
mod skill_catalog;
mod user_project;

pub(crate) use budget::{ContextBudgetError, ContextBudgetEstimate, estimate_conversation_tokens};
pub(crate) use builder::{SystemContextBuildError, SystemContextBuilder};
pub(crate) use engine::{ContextEngine, PrepareContextInput};
pub use inspection::{
    CONTEXT_WINDOW_INSPECTION_SCHEMA_VERSION, ContextInspectionBudget, ContextInspectionMessage,
    ContextInspectionSystemPart, ContextWindowInspection,
};
pub(crate) use limits::ModelContextLimits;
pub(crate) use normalize::{NormalizationPolicy, normalize_for_request};
use project_instructions::{ProjectInstructionError, ProjectInstructionLoader};
use skill_catalog::SkillCatalogLoader;
pub(crate) use skill_catalog::list_skills;
use user_project::{UserProjectContextError, UserProjectContextLoader};

/// One independently assembled system-context contribution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SystemContextPart {
    pub(crate) key: String,
    pub(crate) content: Vec<ContentBlock>,
}

impl SystemContextPart {
    pub(crate) fn new(key: impl Into<String>, content: Vec<ContentBlock>) -> Self {
        Self {
            key: key.into(),
            content,
        }
    }
}

/// System Context resolved once and reused by every Model Call in one Turn.
///
/// Parts are already in model-visible order. This value contains no
/// Conversation, Tool Surface, persistence policy, or refresh state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedSystemContext {
    parts: Vec<SystemContextPart>,
}

impl ResolvedSystemContext {
    pub(super) fn new(parts: Vec<SystemContextPart>) -> Self {
        Self { parts }
    }

    pub(crate) fn parts(&self) -> &[SystemContextPart] {
        &self.parts
    }
}

#[cfg(test)]
impl ResolvedSystemContext {
    pub(crate) fn for_test(parts: Vec<SystemContextPart>) -> Self {
        Self::new(parts)
    }
}
