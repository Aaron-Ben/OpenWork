use serde::Deserialize;
use thiserror::Error;

mod client;

pub use client::{
    AgendaTriageContext, DmLoopContext, SupportDecision, TriageClient, TriageContext,
    TriageMessage, TriageRoom,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseMode {
    Me,
    Each,
    OneOfUs,
}

impl ResponseMode {
    pub fn as_database_str(self) -> &'static str {
        match self {
            Self::Me => "me",
            Self::Each => "each",
            Self::OneOfUs => "one_of_us",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDecision {
    pub actionable: bool,
    pub response_mode: ResponseMode,
    pub reason: String,
    pub prompt_note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageSource {
    EmptyInbox,
    RateLimited,
    LoopCap,
    SupportModel,
    FailOpen,
    FailClosed,
    DmAgentEngage,
}

impl TriageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyInbox => "empty_inbox",
            Self::RateLimited => "rate_limited",
            Self::LoopCap => "loop_cap",
            Self::SupportModel => "support_model",
            Self::FailOpen => "fail_open",
            Self::FailClosed => "fail_closed",
            Self::DmAgentEngage => "dm_agent_engage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackDecision {
    pub actionable: bool,
    pub source: TriageSource,
    pub reason: String,
}

pub fn resolve_failure(human_waiting: bool, reason: impl Into<String>) -> FallbackDecision {
    FallbackDecision {
        actionable: human_waiting,
        source: if human_waiting {
            TriageSource::FailOpen
        } else {
            TriageSource::FailClosed
        },
        reason: reason.into(),
    }
}

pub fn resolve_agenda_failure(reason: impl Into<String>) -> FallbackDecision {
    FallbackDecision {
        actionable: true,
        source: TriageSource::FailOpen,
        reason: reason.into(),
    }
}

pub fn parse_decision(output: &str) -> Result<ParsedDecision, TriageParseError> {
    let start = output.find('{').ok_or(TriageParseError::MissingObject)?;
    let end = output.rfind('}').ok_or(TriageParseError::MissingObject)?;
    if end < start {
        return Err(TriageParseError::MissingObject);
    }
    let raw: RawDecision = serde_json::from_str(&output[start..=end])?;
    let response_mode = match raw.response_mode.as_str() {
        "me" => ResponseMode::Me,
        "each" => ResponseMode::Each,
        "one-of-us" => ResponseMode::OneOfUs,
        value => return Err(TriageParseError::InvalidResponseMode(value.to_string())),
    };
    Ok(ParsedDecision {
        actionable: raw.actionable,
        response_mode,
        reason: raw.reason,
        prompt_note: raw.prompt_note,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawDecision {
    actionable: bool,
    response_mode: String,
    reason: String,
    prompt_note: String,
}

#[derive(Debug, Error)]
pub enum TriageParseError {
    #[error("triage output did not contain a JSON object")]
    MissingObject,
    #[error("triage output JSON was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("triage responseMode was invalid: {0}")]
    InvalidResponseMode(String),
}
