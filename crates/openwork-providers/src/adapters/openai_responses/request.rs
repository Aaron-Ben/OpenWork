//! OpenAI Responses request codec.

use openwork_protocol::model::{ContentBlock, DataSource, Message, ModelError, ModelRequest};
use serde_json::{Value, json};

pub(crate) fn encode_request(req: &ModelRequest, stream: bool) -> Result<Value, ModelError> {
    if req.thinking.is_some() {
        return Err(ModelError::invalid_request(
            "OpenAI thinking mode is not mapped yet",
        ));
    }

    let mut input = Vec::new();
    for message in &req.messages {
        input.extend(encode_message_items(message)?);
    }

    let mut body = json!({
        "model": req.model,
        "input": input,
        "stream": stream,
    });
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = req.max_output_tokens {
        body["max_output_tokens"] = json!(max_tokens);
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                        "strict": true,
                    })
                })
                .collect(),
        );
        body["tool_choice"] = json!("auto");
    }
    Ok(body)
}

fn encode_message_items(message: &Message) -> Result<Vec<Value>, ModelError> {
    let mut items = Vec::new();
    let content = message
        .content
        .iter()
        .filter(|part| {
            !matches!(
                part,
                ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_)
            )
        })
        .map(encode_content_part)
        .collect::<Result<Vec<_>, _>>()?;
    if !content.is_empty() {
        items.push(json!({
            "role": message.role.as_provider_str(),
            "content": content,
        }));
    }
    for part in &message.content {
        match part {
            ContentBlock::ToolCall(block) => items.push(json!({
                "type": "function_call",
                "call_id": block.id,
                "name": block.name,
                "arguments": block.input,
            })),
            ContentBlock::ToolResult(block) => items.push(json!({
                "type": "function_call_output",
                "call_id": block.id,
                "output": text_from_content(&block.output),
            })),
            ContentBlock::Text(_)
            | ContentBlock::Thinking(_)
            | ContentBlock::Data(_)
            | ContentBlock::ProviderOpaque(_) => {}
        }
    }
    Ok(items)
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
        ContentBlock::Text(block) => Ok(json!({ "type": "input_text", "text": block.text })),
        ContentBlock::Thinking(_) => Err(ModelError::invalid_request(
            "OpenAI Responses thinking history is not mapped yet",
        )),
        ContentBlock::Data(block) => match &block.source {
            DataSource::Url { url, media_type } if media_type.starts_with("image/") => {
                Ok(json!({ "type": "input_image", "image_url": url }))
            }
            DataSource::Base64(source) if source.media_type.starts_with("image/") => Ok(json!({
                "type": "input_image",
                "image_url": format!("data:{};base64,{}", source.media_type, source.data),
            })),
            DataSource::FileId { id } => Ok(json!({ "type": "input_file", "file_id": id })),
            DataSource::Url { media_type, .. } => Err(ModelError::invalid_request(format!(
                "OpenAI Responses data media type is not mapped: {media_type}"
            ))),
            DataSource::Base64(source) => Err(ModelError::invalid_request(format!(
                "OpenAI Responses base64 media type is not mapped: {}",
                source.media_type
            ))),
        },
        ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_) => Err(
            ModelError::invalid_request("tool blocks must be encoded as Responses input items"),
        ),
        ContentBlock::ProviderOpaque(_) => Err(ModelError::invalid_request(
            "provider opaque blocks cannot cross into OpenAI Responses",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::model::{
        Message, Role, ToolCallBlock, ToolCallState, ToolDefinition, ToolResultBlock,
        ToolResultState,
    };

    #[test]
    fn builds_responses_body() {
        let req = ModelRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![Message::text(Role::User, "hello")],
            temperature: Some(0.2),
            max_output_tokens: Some(128),
            thinking: None,
            tools: Vec::new(),
        };
        let body = encode_request(&req, false).unwrap();
        assert_eq!(body["model"], "gpt-4.1");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["max_output_tokens"], 128);
    }

    #[test]
    fn maps_function_tools_and_tool_history() {
        let req = ModelRequest {
            model: "gpt-5".to_string(),
            messages: vec![
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolCall(ToolCallBlock {
                        id: "call_1".to_string(),
                        name: "read".to_string(),
                        input: r#"{"path":"Cargo.toml"}"#.to_string(),
                        state: ToolCallState::Submitted,
                    })],
                },
                Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult(ToolResultBlock {
                        id: "call_1".to_string(),
                        name: "read".to_string(),
                        output: vec![ContentBlock::text("workspace")],
                        state: ToolResultState::Success,
                    })],
                },
            ],
            temperature: None,
            max_output_tokens: None,
            thinking: None,
            tools: vec![ToolDefinition {
                name: "read".to_string(),
                description: "Read a file".to_string(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
        };
        let body = encode_request(&req, true).unwrap();
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][1]["type"], "function_call_output");
    }

    #[test]
    fn rejects_unmapped_thinking_mode() {
        let req = ModelRequest::text("gpt-4.1", "hello")
            .with_thinking(openwork_protocol::model::ThinkingConfig::enabled());
        assert!(encode_request(&req, false).is_err());
    }
}
