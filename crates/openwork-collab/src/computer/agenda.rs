use crate::protocol::AgendaDecision;

pub fn parse_agenda_decision(text: &str) -> Result<AgendaDecision, AgendaParseError> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(AgendaParseError::NotExactJson);
    }
    let decision: AgendaDecision = serde_json::from_str(trimmed)?;
    match &decision {
        AgendaDecision::Act {
            candidate_id,
            reason,
        } if candidate_id.trim().is_empty() || reason.trim().is_empty() => {
            Err(AgendaParseError::EmptyField)
        }
        AgendaDecision::Decline { reason } if reason.trim().is_empty() => {
            Err(AgendaParseError::EmptyField)
        }
        _ => Ok(decision),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgendaParseError {
    #[error("agenda response must be exactly one JSON object")]
    NotExactJson,
    #[error("agenda response JSON was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agenda response contains an empty candidate or reason")]
    EmptyField,
}

#[cfg(test)]
mod tests {
    use super::parse_agenda_decision;

    #[test]
    fn parser_accepts_only_the_closed_agenda_shape() {
        assert!(
            parse_agenda_decision(
                r#"{"decision":"act","candidateId":"card:1","reason":"assigned work"}"#
            )
            .is_ok()
        );
        assert!(
            parse_agenda_decision(r#"{"decision":"decline","reason":"nothing pending"}"#).is_ok()
        );
        assert!(
            parse_agenda_decision(
                r#"```json
{"decision":"decline","reason":"quiet only"}
```"#
            )
            .is_err()
        );
        assert!(
            parse_agenda_decision(r#"{"decision":"decline","reason":"quiet only","extra":true}"#)
                .is_err()
        );
        assert!(
            parse_agenda_decision(
                r#"{"decision":"act","candidateId":"missing reason","reason":""}"#
            )
            .is_err()
        );
    }
}
