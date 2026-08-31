use std::fmt;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EngineId(String);

impl EngineId {
    pub fn new(value: impl Into<String>) -> Result<Self, EngineIdError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase);
        if !valid {
            return Err(EngineIdError(value));
        }
        Ok(Self(value))
    }

    pub fn opencode() -> Self {
        Self("opencode".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid Engine id {0:?}")]
pub struct EngineIdError(String);

#[cfg(test)]
mod tests {
    use super::EngineId;

    #[test]
    fn engine_id_is_a_bounded_lowercase_protocol_value() {
        for valid in ["opencode", "codex", "engine-2"] {
            assert_eq!(EngineId::new(valid).unwrap().as_str(), valid);
        }
        for invalid in ["", "OpenCode", "2engine", "engine_name", &"a".repeat(65)] {
            assert!(EngineId::new(invalid).is_err(), "accepted {invalid:?}");
        }
    }
}
