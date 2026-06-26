use openwork_protocol::ai::{
    ContentBlock, DataSource, Message, ProviderError, Role, TokenUsage, ToolCallBlock,
    ToolCallState, ToolDefinition,
};
use serde_json::{Value, json};

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

pub fn usage_from_openai(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
    })
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

pub fn openai_chat_message(message: &Message) -> Result<Value, ProviderError> {
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

pub fn openai_chat_content_part(part: &ContentBlock) -> Result<Value, ProviderError> {
    match part {
        ContentBlock::Text(block) => Ok(json!({ "type": "text", "text": block.text })),
        ContentBlock::Thinking(_) => Err(ProviderError::InvalidRequest {
            message: "thinking blocks must be mapped at message level".to_string(),
        }),
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
            DataSource::Base64(source) => Err(ProviderError::InvalidRequest {
                message: format!("unsupported base64 media type: {}", source.media_type),
            }),
            DataSource::Url { media_type, .. } => Err(ProviderError::InvalidRequest {
                message: format!("unsupported URL media type: {media_type}"),
            }),
            DataSource::FileId { id } => Ok(json!({
                "type": "file",
                "file": { "file_id": id },
            })),
        },
        ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_) => {
            Err(ProviderError::InvalidRequest {
                message: "tool blocks are not mapped for OpenAI-compatible chat yet".to_string(),
            })
        }
    }
}

pub fn response_text_from_chat_completion(value: &Value) -> String {
    let Some(content) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    else {
        return String::new();
    };

    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

pub fn reasoning_text_from_chat_completion(value: &Value) -> Option<String> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("reasoning_content"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// 从 chat completion 响应里提取工具调用(choices[0].message.tool_calls)。
pub fn tool_calls_from_chat_completion(value: &Value) -> Vec<ToolCallBlock> {
    let Some(tool_calls) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    tool_calls
        .iter()
        .filter_map(|tc| {
            let id = tc.get("id").and_then(Value::as_str)?;
            let function = tc.get("function")?;
            let name = function.get("name").and_then(Value::as_str)?;
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            Some(ToolCallBlock {
                id: id.to_string(),
                name: name.to_string(),
                input: arguments.to_string(),
                state: ToolCallState::Submitted,
            })
        })
        .collect()
}

pub fn role_supported_by_anthropic(role: Role) -> bool {
    matches!(role, Role::User | Role::Assistant)
}
