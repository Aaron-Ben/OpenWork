use serde_json::{Map, Value};

pub(crate) fn validate_input(schema: &Value, input: &Value) -> Result<(), String> {
    validate_value(schema, input, "input")
}

fn validate_value(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(schema_type) = schema.get("type").and_then(Value::as_str) else {
        return Err(format!("{path} schema is missing a supported 'type'"));
    };

    match schema_type {
        "object" => validate_object(schema, value, path),
        "string" if value.is_string() => validate_enum(schema, value, path),
        "number" if value.is_number() => Ok(()),
        "integer" if value.as_i64().is_some() || value.as_u64().is_some() => Ok(()),
        "boolean" if value.is_boolean() => Ok(()),
        "array" if value.is_array() => Ok(()),
        "null" if value.is_null() => Ok(()),
        "string" | "number" | "integer" | "boolean" | "array" | "null" => Err(format!(
            "{path} must be of type {schema_type}, got {}",
            value_type(value)
        )),
        unsupported => Err(format!(
            "{path} uses unsupported schema type '{unsupported}'"
        )),
    }
}

fn validate_object(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path} must be of type object, got {}", value_type(value)))?;

    if let Some(required) = schema.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| format!("{path} schema 'required' must be an array"))?;
        for name in required {
            let name = name
                .as_str()
                .ok_or_else(|| format!("{path} schema required entries must be strings"))?;
            if !object.contains_key(name) {
                return Err(format!("{path}.{name} is required"));
            }
        }
    }

    let properties = match schema.get("properties") {
        Some(properties) => Some(
            properties
                .as_object()
                .ok_or_else(|| format!("{path} schema 'properties' must be an object"))?,
        ),
        None => None,
    };

    if let Some(properties) = properties {
        validate_properties(properties, object, path)?;
    }
    Ok(())
}

fn validate_properties(
    properties: &Map<String, Value>,
    object: &Map<String, Value>,
    path: &str,
) -> Result<(), String> {
    for (name, value) in object {
        if let Some(property_schema) = properties.get(name) {
            validate_value(property_schema, value, &format!("{path}.{name}"))?;
        }
    }
    Ok(())
}

fn validate_enum(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(allowed) = schema.get("enum") else {
        return Ok(());
    };
    let allowed = allowed
        .as_array()
        .ok_or_else(|| format!("{path} schema 'enum' must be an array"))?;
    if allowed.contains(value) {
        Ok(())
    } else {
        Err(format!("{path} is not one of the allowed values"))
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_required_fields_types_and_enum() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "enum": ["a", "b"]},
                "count": {"type": "number"}
            },
            "required": ["name"]
        });
        assert!(validate_input(&schema, &json!({"name": "a", "count": 1})).is_ok());
        assert!(validate_input(&schema, &json!({"count": 1})).is_err());
        assert!(validate_input(&schema, &json!({"name": "c"})).is_err());
        assert!(validate_input(&schema, &json!({"name": 1})).is_err());
    }
}
