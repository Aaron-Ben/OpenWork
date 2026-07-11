use openwork_protocol::model::{
    ContentBlock, DataSource, Message, ModelError, ModelRequest, Role, ThinkingMode, ToolDefinition,
};
use openwork_protocol::provider::OpenAiChatDialect;
use serde_json::{Map, Value, json};

pub(crate) fn encode_request(
    req: &ModelRequest,
    stream: bool,
    dialect: OpenAiChatDialect,
    extra_body: &Map<String, Value>,
) -> Result<Value, ModelError> {
    let messages = req
        .messages
        .iter()
        .map(openai_chat_message)
        .collect::<Result<Vec<_>, _>>()?;

    let mut body = Map::new();
    body.insert("model".to_string(), json!(req.model));
    body.insert("messages".to_string(), json!(messages));
    body.insert("stream".to_string(), json!(stream));
    if stream {
        body.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }
    if let Some(temperature) = req.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_tokens) = req.max_output_tokens {
        let field = if dialect == OpenAiChatDialect::Kimi {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        body.insert(field.to_string(), json!(max_tokens));
    }
    if !req.tools.is_empty() {
        body.insert("tools".to_string(), openai_chat_tools(&req.tools));
        body.insert("tool_choice".to_string(), json!("auto"));
    }
    if let Some(thinking) = req.thinking {
        match dialect {
            OpenAiChatDialect::Qwen => {
                body.insert(
                    "enable_thinking".to_string(),
                    json!(matches!(thinking.mode, ThinkingMode::Enabled)),
                );
            }
            OpenAiChatDialect::Deepseek | OpenAiChatDialect::Glm | OpenAiChatDialect::Kimi => {
                let mode = match thinking.mode {
                    ThinkingMode::Enabled => "enabled",
                    ThinkingMode::Disabled => "disabled",
                };
                body.insert("thinking".to_string(), json!({ "type": mode }));
            }
        }
    }
    if stream && !req.tools.is_empty() && dialect == OpenAiChatDialect::Glm {
        body.insert("tool_stream".to_string(), json!(true));
    }

    validate_extra_body(extra_body)?;
    for (key, value) in extra_body {
        body.insert(key.clone(), value.clone());
    }

    Ok(Value::Object(body))
}

pub(crate) fn validate_extra_body(extra: &Map<String, Value>) -> Result<(), ModelError> {
    const RESERVED: &[&str] = &[
        "model",
        "messages",
        "tools",
        "tool_choice",
        "stream",
        "temperature",
        "max_tokens",
        "max_completion_tokens",
    ];
    if let Some(field) = RESERVED.iter().find(|field| extra.contains_key(**field)) {
        return Err(ModelError::invalid_request(format!(
            "adapter options cannot override reserved field '{field}'"
        )));
    }
    Ok(())
}

pub fn text_from_content(parts: &[ContentBlock]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            ContentBlock::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn thinking_from_content(parts: &[ContentBlock]) -> Option<String> {
    let thinking = parts
        .iter()
        .filter_map(|part| match part {
            ContentBlock::Thinking(block) => Some(block.thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if thinking.is_empty() {
        None
    } else {
        Some(thinking)
    }
}

/// 把内部工具声明序列化为 OpenAI chat completions 的 `tools` 数组。
pub fn openai_chat_tools(tools: &[ToolDefinition]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect(),
    )
}

pub fn openai_chat_message(message: &Message) -> Result<Value, ModelError> {
    let role = message.role.as_provider_str();

    // Tool 结果消息:OpenAI 期望 { role:"tool", tool_call_id, content }。
    if message.role == Role::Tool {
        let tool_result = message.content.iter().find_map(|part| match part {
            ContentBlock::ToolResult(block) => Some(block),
            _ => None,
        });
        if let Some(block) = tool_result {
            let content = text_from_content(&block.output);
            return Ok(json!({
                "role": "tool",
                "tool_call_id": block.id,
                "content": content,
            }));
        }
    }

    // 单文本块(且不含工具调用):扁平化为 { role, content: string }。
    if message.content.len() == 1
        && let Some(ContentBlock::Text(block)) = message.content.first()
    {
        return Ok(json!({ "role": role, "content": block.text }));
    }

    // 多块:content 只收集 Text/Data(Thinking/ToolCall/ToolResult 在 message 级处理)。
    let content = message
        .content
        .iter()
        .filter(|part| {
            !matches!(
                part,
                ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_)
            )
        })
        .map(openai_chat_content_part)
        .collect::<Result<Vec<_>, _>>()?;

    let mut message_json = json!({ "role": role });
    if content.is_empty() {
        message_json["content"] = Value::Null;
    } else {
        message_json["content"] = json!(content);
    }

    if message.role == Role::Assistant
        && let Some(thinking) = thinking_from_content(&message.content)
    {
        message_json["reasoning_content"] = json!(thinking);
    }

    // assistant 工具调用:映射到 message 级 tool_calls。
    if message.role == Role::Assistant {
        let tool_calls: Vec<Value> = message
            .content
            .iter()
            .filter_map(|part| match part {
                ContentBlock::ToolCall(block) => Some(json!({
                    "id": block.id,
                    "type": "function",
                    "function": {
                        "name": block.name,
                        "arguments": block.input,
                    }
                })),
                _ => None,
            })
            .collect();
        if !tool_calls.is_empty() {
            message_json["tool_calls"] = json!(tool_calls);
        }
    }

    Ok(message_json)
}

pub fn openai_chat_content_part(part: &ContentBlock) -> Result<Value, ModelError> {
    match part {
        ContentBlock::Text(block) => Ok(json!({ "type": "text", "text": block.text })),
        ContentBlock::Thinking(_) => Err(ModelError::invalid_request(
            "thinking blocks must be mapped at message level",
        )),
        ContentBlock::Data(block) => match &block.source {
            DataSource::Url { url, media_type } if media_type.starts_with("image/") => Ok(json!({
                "type": "image_url",
                "image_url": { "url": url },
            })),
            DataSource::Url { url, media_type } if media_type.starts_with("video/") => Ok(json!({
                "type": "video_url",
                "video_url": { "url": url },
            })),
            DataSource::Url { url, media_type } if media_type.starts_with("audio/") => Ok(json!({
                "type": "input_audio",
                "input_audio": {
                    "data": url,
                    "format": media_type.split('/').next_back().unwrap_or("wav"),
                },
            })),
            DataSource::Base64(source) if source.media_type.starts_with("image/") => Ok(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", source.media_type, source.data),
                },
            })),
            DataSource::Base64(source) if source.media_type.starts_with("video/") => Ok(json!({
                "type": "video_url",
                "video_url": {
                    "url": format!("data:{};base64,{}", source.media_type, source.data),
                },
            })),
            DataSource::Base64(source) if source.media_type.starts_with("audio/") => Ok(json!({
                "type": "input_audio",
                "input_audio": {
                    "data": source.data,
                    "format": source.media_type.split('/').next_back().unwrap_or("wav"),
                },
            })),
            DataSource::Base64(source) => Err(ModelError::invalid_request(format!(
                "unsupported base64 media type: {}",
                source.media_type
            ))),
            DataSource::Url { media_type, .. } => Err(ModelError::invalid_request(format!(
                "unsupported URL media type: {media_type}"
            ))),
            DataSource::FileId { id } => Ok(json!({
                "type": "file",
                "file": { "file_id": id },
            })),
        },
        ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_) => {
            Err(ModelError::invalid_request(
                "tool blocks are not mapped for OpenAI-compatible chat yet",
            ))
        }
        ContentBlock::ProviderOpaque(_) => Err(ModelError::invalid_request(
            "provider opaque blocks cannot cross into OpenAI-compatible chat",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::model::{Message, ThinkingConfig, ToolDefinition};

    #[test]
    fn builds_text_chat_completion_body() {
        let req = ModelRequest {
            model: "qwen-plus".to_string(),
            messages: vec![Message::text(Role::User, "hello")],
            temperature: None,
            max_output_tokens: Some(64),
            thinking: None,
            tools: Vec::new(),
        };

        let body = encode_request(&req, false, OpenAiChatDialect::Qwen, &Map::new()).unwrap();

        assert_eq!(body["model"], "qwen-plus");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert_eq!(body["max_tokens"], 64);
    }

    #[test]
    fn extra_body_cannot_override_stable_request_fields() {
        let extra = Map::from_iter([("model".to_string(), json!("attacker-model"))]);

        assert!(
            encode_request(
                &ModelRequest::text("deepseek-chat", "hello"),
                false,
                OpenAiChatDialect::Deepseek,
                &extra,
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_assistant_thinking_blocks_in_history() {
        let req = ModelRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![Message::assistant_with_thinking("answer", "reasoning")],
            temperature: None,
            max_output_tokens: None,
            thinking: None,
            tools: Vec::new(),
        };

        let body = encode_request(&req, false, OpenAiChatDialect::Deepseek, &Map::new()).unwrap();

        assert_eq!(body["messages"][0]["reasoning_content"], "reasoning");
        assert_eq!(body["messages"][0]["content"][0]["text"], "answer");
    }

    #[test]
    fn includes_tools_and_maps_typed_thinking() {
        let mut req = ModelRequest::text("deepseek-chat", "list files")
            .with_thinking(ThinkingConfig::enabled());
        req.tools.push(ToolDefinition {
            name: "list".to_string(),
            description: "list dir".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        });

        let body = encode_request(&req, true, OpenAiChatDialect::Deepseek, &Map::new()).unwrap();

        assert_eq!(body["tools"][0]["function"]["name"], "list");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }
}
