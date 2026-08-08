use crate::model::{ContentBlock, DataSource, ModelError, ModelRequest, Role, ThinkingMode};
use serde_json::{Value, json};

pub(crate) fn encode_request(req: &ModelRequest, stream: bool) -> Result<Value, ModelError> {
    if matches!(
        req.thinking.map(|thinking| thinking.mode),
        Some(ThinkingMode::Enabled)
    ) {
        return Err(ModelError::invalid_request(
            "Anthropic thinking mode is not mapped yet",
        ));
    }

    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    for message in &req.messages {
        if message.role == Role::System {
            let text = text_from_content(&message.content);
            if !text.is_empty() {
                system_parts.push(text);
            }
            continue;
        }

        if message.role != Role::Tool && !role_supported_by_anthropic(message.role) {
            return Err(ModelError::invalid_request(format!(
                "Anthropic does not support role {:?}",
                message.role
            )));
        }
        if message.role == Role::Tool
            && !message
                .content
                .iter()
                .any(|part| matches!(part, ContentBlock::ToolResult(_)))
        {
            return Err(ModelError::invalid_request(
                "Anthropic tool messages require a tool_result block",
            ));
        }

        let content = message
            .content
            .iter()
            .map(encode_content_part)
            .collect::<Result<Vec<_>, _>>()?;
        messages.push(json!({
            "role": if message.role == Role::Tool { "user" } else { message.role.as_provider_str() },
            "content": content,
        }));
    }

    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_output_tokens.unwrap_or(1024),
        "stream": stream,
    });

    if !system_parts.is_empty() {
        body["system"] = json!(system_parts.join("\n"));
    }
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = req.top_p {
        body["top_p"] = json!(top_p);
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters,
                    })
                })
                .collect(),
        );
    }

    Ok(body)
}

fn role_supported_by_anthropic(role: Role) -> bool {
    matches!(role, Role::User | Role::Assistant)
}

fn text_from_content(parts: &[ContentBlock]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            ContentBlock::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn encode_content_part(part: &ContentBlock) -> Result<Value, ModelError> {
    match part {
        ContentBlock::Text(block) => Ok(json!({ "type": "text", "text": block.text })),
        ContentBlock::Thinking(_) => Err(ModelError::invalid_request(
            "Anthropic thinking blocks are not mapped yet",
        )),
        ContentBlock::Data(block) => match &block.source {
            DataSource::Url { url, media_type } if media_type.starts_with("image/") => Ok(json!({
                "type": "image",
                "source": { "type": "url", "url": url },
            })),
            DataSource::Base64(source) if source.media_type.starts_with("image/") => Ok(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": source.media_type,
                    "data": source.data,
                },
            })),
            DataSource::FileId { id } => Err(ModelError::invalid_request(format!(
                "Anthropic file id content is not mapped yet: {id}"
            ))),
            DataSource::Url { media_type, .. } => Err(ModelError::invalid_request(format!(
                "Anthropic data media type is not mapped: {media_type}"
            ))),
            DataSource::Base64(source) => Err(ModelError::invalid_request(format!(
                "Anthropic base64 media type is not mapped: {}",
                source.media_type
            ))),
        },
        ContentBlock::ToolCall(block) => {
            let input = serde_json::from_str::<Value>(&block.input).map_err(|error| {
                ModelError::invalid_request(format!(
                    "Anthropic tool call '{}' has invalid JSON input: {error}",
                    block.name
                ))
            })?;
            Ok(json!({
                "type": "tool_use",
                "id": block.id,
                "name": block.name,
                "input": input,
            }))
        }
        ContentBlock::ToolResult(block) => Ok(json!({
            "type": "tool_result",
            "tool_use_id": block.id,
            "content": text_from_content(&block.output),
            "is_error": matches!(
                block.state,
                crate::model::ToolResultState::Error
                    | crate::model::ToolResultState::Interrupted
                    | crate::model::ToolResultState::Denied
            ),
        })),
        ContentBlock::ProviderOpaque(block) => {
            if block.driver != crate::provider::ProviderDriver::AnthropicMessages {
                return Err(ModelError::invalid_request(
                    "provider opaque block belongs to a different driver",
                ));
            }
            if !matches!(block.kind.as_str(), "thinking" | "redacted_thinking") {
                return Err(ModelError::invalid_request(format!(
                    "unsupported Anthropic opaque block kind: {}",
                    block.kind
                )));
            }
            if serde_json::to_vec(&block.payload)
                .map_err(|error| ModelError::invalid_request(error.to_string()))?
                .len()
                > 64 * 1024
            {
                return Err(ModelError::invalid_request(
                    "Anthropic opaque block exceeds 64 KiB",
                ));
            }
            Ok(block.payload.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Message, ProviderOpaqueBlock, ThinkingConfig, ToolCallBlock, ToolCallState, ToolDefinition,
        ToolResultBlock, ToolResultState,
    };
    use crate::provider::ProviderDriver;

    #[test]
    fn builds_messages_body_with_system_prompt() {
        let req = ModelRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![
                Message::text(Role::System, "You are concise."),
                Message::text(Role::User, "hello"),
            ],
            temperature: Some(0.1),
            top_p: Some(0.9),
            max_output_tokens: Some(128),
            thinking: None,
            tools: Vec::new(),
        };

        let body = encode_request(&req, false).unwrap();

        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["system"], "You are concise.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["temperature"].as_f64(), Some(f64::from(0.1_f32)));
        assert_eq!(body["top_p"].as_f64(), Some(f64::from(0.9_f32)));
        assert_eq!(body["max_tokens"], 128);
    }

    #[test]
    fn omits_explicitly_disabled_thinking() {
        let req = ModelRequest::text("claude-sonnet-4-5", "hello")
            .with_thinking(ThinkingConfig::disabled());

        let body = encode_request(&req, false).expect("disabled thinking should be supported");

        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn rejects_enabled_thinking_until_it_is_mapped() {
        let req = ModelRequest::text("claude-sonnet-4-5", "hello")
            .with_thinking(ThinkingConfig::enabled());

        assert!(encode_request(&req, false).is_err());
    }

    #[test]
    fn rejects_plain_text_tool_role_without_tool_result_block() {
        let req = ModelRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message::text(Role::Tool, "tool result")],
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            thinking: None,
            tools: Vec::new(),
        };

        assert!(encode_request(&req, false).is_err());
    }

    #[test]
    fn maps_tools_tool_use_and_tool_result_blocks() {
        let req = ModelRequest {
            model: "claude-sonnet".to_string(),
            messages: vec![
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolCall(ToolCallBlock {
                        id: "toolu_1".to_string(),
                        name: "read".to_string(),
                        input: r#"{"path":"Cargo.toml"}"#.to_string(),
                        state: ToolCallState::Submitted,
                    })],
                },
                Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult(ToolResultBlock {
                        id: "toolu_1".to_string(),
                        name: "read".to_string(),
                        output: vec![ContentBlock::text("workspace")],
                        state: ToolResultState::Success,
                        artifacts: Vec::new(),
                    })],
                },
            ],
            temperature: None,
            top_p: None,
            max_output_tokens: Some(1024),
            thinking: None,
            tools: vec![ToolDefinition {
                name: "read".to_string(),
                description: "Read a file".to_string(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
        };

        let body = encode_request(&req, true).unwrap();

        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "toolu_1");
    }

    #[test]
    fn agent_message_after_tool_results_is_a_user_input_not_an_assistant_prefill() {
        let req = ModelRequest {
            model: "claude-sonnet".to_string(),
            messages: vec![
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolCall(ToolCallBlock {
                        id: "toolu_1".to_string(),
                        name: "read".to_string(),
                        input: r#"{"path":"Cargo.toml"}"#.to_string(),
                        state: ToolCallState::Submitted,
                    })],
                },
                Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult(ToolResultBlock {
                        id: "toolu_1".to_string(),
                        name: "read".to_string(),
                        output: vec![ContentBlock::text("workspace")],
                        state: ToolResultState::Success,
                        artifacts: Vec::new(),
                    })],
                },
                Message::text(
                    Role::User,
                    "<agent_message>\n<task>find_auth</task>\n<kind>final_answer</kind>\n<body>\nFound it.\n</body>\n</agent_message>",
                ),
            ],
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            thinking: None,
            tools: Vec::new(),
        };

        let body = encode_request(&req, false).expect("valid Anthropic request");
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.last().expect("agent message")["role"], "user");
        assert!(
            messages.last().expect("agent message")["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.contains("<task>find_auth</task>"))
        );
    }

    #[test]
    fn preserves_anthropic_opaque_thinking_blocks_in_history() {
        let req = ModelRequest {
            model: "claude-sonnet".to_string(),
            messages: vec![Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ProviderOpaque(ProviderOpaqueBlock {
                    driver: ProviderDriver::AnthropicMessages,
                    kind: "thinking".to_string(),
                    payload: json!({
                        "type": "thinking",
                        "thinking": "summary",
                        "signature": "signed-state"
                    }),
                })],
            }],
            temperature: None,
            top_p: None,
            max_output_tokens: Some(1024),
            thinking: None,
            tools: Vec::new(),
        };

        let body = encode_request(&req, false).unwrap();

        assert_eq!(body["messages"][0]["content"][0]["type"], "thinking");
        assert_eq!(
            body["messages"][0]["content"][0]["signature"],
            "signed-state"
        );
    }
}
