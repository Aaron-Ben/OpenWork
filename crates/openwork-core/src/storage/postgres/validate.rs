use super::*;

pub(super) fn validate_model(input: &ModelInput) -> Result<(), StorageError> {
    for (name, value) in [
        ("id", input.id.as_str()),
        ("display_name", input.display_name.as_str()),
        ("provider_kind", input.provider_kind.as_str()),
        ("model_name", input.model_name.as_str()),
        ("base_url", input.base_url.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(StorageError::InvalidInput(format!(
                "{name} must not be blank"
            )));
        }
    }
    if input
        .credential_ref
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(StorageError::InvalidInput(
            "credential_ref must not be blank".to_string(),
        ));
    }
    if !input.config.is_object() {
        return Err(StorageError::InvalidInput(
            "model config must be a JSON object".to_string(),
        ));
    }
    input.capabilities.validate().map_err(|error| {
        StorageError::InvalidInput(format!("model capabilities are invalid: {error}"))
    })?;
    Ok(())
}

pub(super) fn validate_session(input: &SessionInput) -> Result<(), StorageError> {
    if input.id.as_str().trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "session id must not be blank".to_string(),
        ));
    }
    if input.working_directory.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "working_directory must not be blank".to_string(),
        ));
    }
    if input
        .title
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(StorageError::InvalidInput(
            "session title must not be blank".to_string(),
        ));
    }
    Ok(())
}

/// Longest `task_name` accepted, mirroring `sessions_task_name_format`.
const MAX_TASK_NAME_LEN: usize = 48;

/// True when `value` matches `^[a-z][a-z0-9_]{0,47}$`.
///
/// Kept in lockstep with the `sessions_task_name_format` CHECK. Validating here
/// too is not redundant: the model picks this name, so it needs a message it can
/// act on rather than a raw constraint-violation error.
pub(crate) fn is_valid_task_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= MAX_TASK_NAME_LEN
        && first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub(super) fn validate_sub_agent_session(input: &SubAgentSessionInput) -> Result<(), StorageError> {
    if input.id.as_str().trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "session id must not be blank".to_string(),
        ));
    }
    if input.working_directory.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "working_directory must not be blank".to_string(),
        ));
    }
    if !is_valid_task_name(&input.task_name) {
        return Err(StorageError::InvalidInput(format!(
            "task_name {:?} must match ^[a-z][a-z0-9_]{{0,47}}$",
            input.task_name
        )));
    }
    if input.agent_role.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "agent_role must not be blank".to_string(),
        ));
    }
    if input.id == input.parent_session_id {
        return Err(StorageError::InvalidInput(
            "a session cannot be its own parent".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_resolved_model(model: &ResolvedModel) -> Result<(), StorageError> {
    if model.provider_kind.trim().is_empty() || model.model_name.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "resolved provider kind and model name must not be blank".to_string(),
        ));
    }
    model.capabilities.validate().map_err(|error| {
        StorageError::InvalidInput(format!("resolved model capabilities are invalid: {error}"))
    })?;
    Ok(())
}
pub(super) fn validate_runtime_checkpoint_state(
    runtime_state: &CompactionRuntimeState,
    runtime_reminder: &str,
) -> Result<(), StorageError> {
    if runtime_state.schema_version != 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported compaction runtime state schema version: {}",
            runtime_state.schema_version
        )));
    }
    validate_system_reminder(runtime_reminder)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}
