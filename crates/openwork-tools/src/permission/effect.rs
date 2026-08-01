use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::policy::AccessKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Read { path: PathBuf },
    Write { path: PathBuf },
    Exec { program: String, args: Vec<String> },
}

impl Effect {
    pub fn read(path: impl Into<PathBuf>) -> Self {
        Self::Read { path: path.into() }
    }

    pub fn write(path: impl Into<PathBuf>) -> Self {
        Self::Write { path: path.into() }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Read { path } | Self::Write { path } => Some(path),
            Self::Exec { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisUnit {
    pub display: String,
    pub effects: Vec<Effect>,
}

impl AnalysisUnit {
    pub fn new(display: impl Into<String>, effects: Vec<Effect>) -> Self {
        Self {
            display: display.into(),
            effects,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationAnalysis {
    pub raw: String,
    pub units: Vec<AnalysisUnit>,
    pub unparsed: bool,
}

impl InvocationAnalysis {
    pub fn new(raw: impl Into<String>, units: Vec<AnalysisUnit>) -> Self {
        Self {
            raw: raw.into(),
            units,
            unparsed: false,
        }
    }

    pub fn unparsed(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        Self {
            units: vec![AnalysisUnit::new(raw.clone(), Vec::new())],
            raw,
            unparsed: true,
        }
    }

    pub fn effects(&self) -> impl Iterator<Item = &Effect> {
        self.units.iter().flat_map(|unit| unit.effects.iter())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPermit {
    effects: Vec<Effect>,
}

impl ExecutionPermit {
    pub(crate) fn new(effects: Vec<Effect>) -> Self {
        Self { effects }
    }

    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    pub(crate) fn permits_path(&self, path: &Path, kind: AccessKind) -> bool {
        self.effects.iter().any(|effect| match (effect, kind) {
            (Effect::Read { path: permitted }, AccessKind::Read)
            | (Effect::Write { path: permitted }, AccessKind::Write) => permitted == path,
            _ => false,
        })
    }
}
