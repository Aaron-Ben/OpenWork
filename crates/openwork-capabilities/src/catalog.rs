use std::collections::HashMap;

use async_trait::async_trait;
use openwork_protocol::capability::{
    CapabilityResolveError, CapabilityResolverPort, CapabilitySpec,
};
use thiserror::Error;

use crate::builtin::builtin_specs;

#[derive(Debug, Clone)]
pub struct CapabilityCatalog {
    specs: Vec<CapabilitySpec>,
    index: HashMap<String, usize>,
}

impl CapabilityCatalog {
    pub fn try_new(specs: impl IntoIterator<Item = CapabilitySpec>) -> Result<Self, CatalogError> {
        let mut ordered = Vec::new();
        let mut index = HashMap::new();

        for spec in specs {
            if spec.name.trim().is_empty() {
                return Err(CatalogError::BlankName);
            }
            if spec.description.trim().is_empty() {
                return Err(CatalogError::BlankDescription(spec.name));
            }
            if index.contains_key(&spec.name) {
                return Err(CatalogError::DuplicateName(spec.name));
            }
            index.insert(spec.name.clone(), ordered.len());
            ordered.push(spec);
        }

        Ok(Self {
            specs: ordered,
            index,
        })
    }

    /// 构造由源码控制并经过合同测试的内置 Catalog。
    pub fn builtin() -> Result<Self, CatalogError> {
        Self::try_new(builtin_specs())
    }
}

#[async_trait]
impl CapabilityResolverPort for CapabilityCatalog {
    async fn list(&self) -> Result<Vec<CapabilitySpec>, CapabilityResolveError> {
        Ok(self.specs.clone())
    }

    async fn resolve(&self, name: &str) -> Result<Option<CapabilitySpec>, CapabilityResolveError> {
        Ok(self
            .index
            .get(name)
            .map(|position| self.specs[*position].clone()))
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
