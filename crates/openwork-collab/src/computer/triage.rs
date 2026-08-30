use serde::Deserialize;

use crate::protocol::TriageVerdict;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelVerdict {
    actionable: bool,
    reason: String,
    prompt_note: String,
}

pub fn parse_triage(text: &str) -> Result<TriageVerdict, TriageParseError> {
    let text = text.trim();
    if !text.starts_with('{') || !text.ends_with('}') {
        return Err(TriageParseError::MissingObject);
    }
    let parsed: ModelVerdict = serde_json::from_str(text)?;
    if parsed.reason.trim().is_empty() {
        return Err(TriageParseError::MissingReason);
    }
    Ok(TriageVerdict {
        actionable: parsed.actionable,
        reason: parsed.reason,
        prompt_note: parsed.prompt_note,
        source: "local_model".to_string(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum TriageParseError {
    #[error("triage response did not contain a JSON object")]
    MissingObject,
    #[error("triage response JSON was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("triage response reason was empty")]
    MissingReason,
}
