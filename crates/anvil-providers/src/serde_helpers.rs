use anvil_core::ai::{ContentBlock, DataSource, Message, ProviderError, Role, TokenUsage};
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

pub fn openai_chat_message(message: &Message) -> Result<Value, ProviderError> {
    let role = message.role.as_provider_str();

    if message.content.len() == 1
        && let Some(ContentBlock::Text(block)) = message.content.first()
    {
        return Ok(json!({ "role": role, "content": block.text }));
    }

    let content = message
        .content
        .iter()
        .filter(|part| !matches!(part, ContentBlock::Thinking(_)))
        .map(openai_chat_content_part)
        .collect::<Result<Vec<_>, _>>()?;

    let mut message_json = json!({ "role": role, "content": content });
    if message.role == Role::Assistant
        && let Some(thinking) = thinking_from_content(&message.content)
    {
        message_json["reasoning_content"] = json!(thinking);
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

pub fn role_supported_by_anthropic(role: Role) -> bool {
    matches!(role, Role::User | Role::Assistant)
}
