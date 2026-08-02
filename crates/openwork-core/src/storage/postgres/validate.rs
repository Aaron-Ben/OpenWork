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

pub(super) fn validate_resolved_model(model: &ResolvedModel) -> Result<(), StorageError> {
    if model.provider_kind.trim().is_empty() || model.model_name.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "resolved provider kind and model name must not be blank".to_string(),
        ));
    }
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
