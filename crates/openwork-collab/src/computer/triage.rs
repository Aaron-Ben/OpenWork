use serde::Deserialize;

use crate::protocol::TriageVerdict;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelVerdict {
    actionable: bool,
    reason: String,
    prompt_note: String,
}

pub fn parse_triage(text: &str) -> Result<TriageVerdict, TriageParseError> {
    let parsed = match complete_json_object(text) {
        Some(object) => match serde_json::from_str::<ModelVerdict>(object) {
            Ok(parsed) => parsed,
            Err(error) if serde_json::from_str::<serde_json::Value>(object).is_ok() => {
                return Err(error.into());
            }
            Err(_) => salvage_triage(text)?,
        },
        None => salvage_triage(text)?,
    };
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

fn complete_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let end = start + offset + character.len_utf8();
                    return Some(&text[start..end]);
                }
            }
            _ => {}
        }
    }
    None
}

fn salvage_triage(text: &str) -> Result<ModelVerdict, TriageParseError> {
    let actionable = json_bool_field(text, "actionable").ok_or(TriageParseError::MissingObject)?;
    let reason = json_string_field(text, "reason").ok_or(TriageParseError::MissingReason)?;
    let prompt_note = json_string_field(text, "promptNote").unwrap_or_default();
    Ok(ModelVerdict {
        actionable,
        reason,
        prompt_note,
    })
}

fn json_field_tail<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let field = format!("\"{name}\"");
    let after_name = text.get(text.find(&field)? + field.len()..)?;
    let after_colon = after_name.get(after_name.find(':')? + 1..)?;
    Some(after_colon.trim_start())
}

fn json_bool_field(text: &str, name: &str) -> Option<bool> {
    let tail = json_field_tail(text, name)?;
    if json_literal_prefix(tail, "true") {
        Some(true)
    } else if json_literal_prefix(tail, "false") {
        Some(false)
    } else {
        None
    }
}

fn json_literal_prefix(text: &str, literal: &str) -> bool {
    text.strip_prefix(literal).is_some_and(|rest| {
        rest.chars().next().is_none_or(|character| {
            character.is_ascii_whitespace() || matches!(character, ',' | '}')
        })
    })
}

fn json_string_field(text: &str, name: &str) -> Option<String> {
    let tail = json_field_tail(text, name)?;
    String::deserialize(&mut serde_json::Deserializer::from_str(tail)).ok()
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
