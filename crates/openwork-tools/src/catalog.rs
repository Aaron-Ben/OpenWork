use std::collections::HashMap;

use thiserror::Error;

use crate::builtins::builtin_definitions;
use crate::schema::validate_input;
use crate::{PermissionMode, PolicyDecision, ToolDefinition, ToolInvocation};

#[derive(Debug, Clone)]
pub struct ToolCatalog {
    definitions: Vec<ToolDefinition>,
    index: HashMap<String, usize>,
}

impl ToolCatalog {
    pub fn try_new(
        definitions: impl IntoIterator<Item = ToolDefinition>,
    ) -> Result<Self, CatalogError> {
        let mut ordered = Vec::new();
        let mut index = HashMap::new();

        for definition in definitions {
            if definition.name.trim().is_empty() {
                return Err(CatalogError::BlankName);
            }
            if definition.description.trim().is_empty() {
                return Err(CatalogError::BlankDescription(definition.name));
            }
            if index.contains_key(&definition.name) {
                return Err(CatalogError::DuplicateName(definition.name));
            }
            index.insert(definition.name.clone(), ordered.len());
            ordered.push(definition);
        }

        Ok(Self {
            definitions: ordered,
            index,
        })
    }

    pub fn builtin() -> Result<Self, CatalogError> {
        Self::try_new(builtin_definitions())
    }

    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }

    pub fn resolve(&self, name: &str) -> Option<&ToolDefinition> {
        self.index
            .get(name)
            .map(|position| &self.definitions[*position])
    }

    pub fn validate(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<&ToolDefinition, ToolValidationError> {
        let definition = self
            .resolve(&invocation.name)
            .ok_or_else(|| ToolValidationError::UnknownTool(invocation.name.clone()))?;
        validate_input(&definition.input_schema, &invocation.input)
            .map_err(ToolValidationError::InvalidInput)?;
        Ok(definition)
    }

    pub fn authorize(&self, invocation: &ToolInvocation, mode: PermissionMode) -> PolicyDecision {
        match self.validate(invocation) {
            Ok(definition) => crate::policy::evaluate(mode, definition.risk_hint),
            Err(error) => PolicyDecision::Deny {
                reason: error.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    #[error("capability name must not be blank")]
    BlankName,
    #[error("capability description must not be blank: {0}")]
    BlankDescription(String),
    #[error("duplicate capability name: {0}")]
    DuplicateName(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolValidationError {
    #[error("tool not found: {0}")]
    UnknownTool(String),
    #[error("invalid tool input: {0}")]
    InvalidInput(String),
}
