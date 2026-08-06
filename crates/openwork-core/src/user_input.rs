use serde::{Deserialize, Serialize};

/// Structured input submitted by a host for one Turn.
///
/// Skill selections remain distinct from model `ContentBlock`s. Core resolves
/// them into contextual messages before the Session accepts the Turn.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserInput {
    Text { text: String },
    Skill { name: String, path: String },
}

impl UserInput {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn skill(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self::Skill {
            name: name.into(),
            path: path.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UserInput;

    #[test]
    fn structured_inputs_round_trip_without_model_content_blocks() {
        let input = vec![
            UserInput::skill("commit", "/skills/commit/SKILL.md"),
            UserInput::text("Use $commit."),
        ];

        let value = serde_json::to_value(&input).expect("serialize user input");
        assert_eq!(value[0]["type"], "skill");
        assert_eq!(value[1]["type"], "text");
        assert_eq!(
            serde_json::from_value::<Vec<UserInput>>(value).expect("deserialize user input"),
            input
        );
    }
}
