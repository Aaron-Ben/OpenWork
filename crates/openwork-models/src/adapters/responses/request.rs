//! Responses 请求体。照 Codex 的 `build_responses_request`（`codex-rs/core/src/client.rs`）：
//! `store: false`，每次带完整的 input；开启思考时请求加密的 reasoning，并在下一次请求中原样回传。

use crate::model::{ContentBlock, DataSource, Message, ModelError, ModelRequest, Role};
use serde_json::{Map, Value, json};

/// 回传 reasoning 条目时使用的 [`crate::model::ProviderOpaqueBlock::kind`]。
pub(crate) const REASONING_ITEM_KIND: &str = "reasoning";

pub(crate) fn encode_request(req: &ModelRequest) -> Result<Value, ModelError> {
    let mut instructions = Vec::new();
    let mut input = Vec::new();
    for message in &req.messages {
        if message.role == Role::System {
            instructions.push(system_text(message)?);
            continue;
        }
        input.extend(encode_message_items(message)?);
    }

    let mut body = Map::new();
    body.insert("model".to_string(), json!(req.model));
    body.insert("input".to_string(), Value::Array(input));
    body.insert("stream".to_string(), json!(true));
    // 不让服务端保存会话。多数厂商的 Responses 实现本来就是无状态的（DeepSeek、Kimi）。
    body.insert("store".to_string(), json!(false));
    if !instructions.is_empty() {
        body.insert("instructions".to_string(), json!(instructions.join("\n\n")));
    }
    if let Some(temperature) = req.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = req.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(max_tokens) = req.max_output_tokens {
        body.insert("max_output_tokens".to_string(), json!(max_tokens));
    }
    // 不发 `include`：OpenAI 在 `store: false` 时默认返回 `encrypted_content`，其他厂商不支持它。
    if let Some(effort) = &req.reasoning_effort {
        body.insert("reasoning".to_string(), json!({ "effort": effort }));
    }
    if !req.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                            // 工具参数中有可选项；严格模式要求全部参数必填。
                            "strict": false,
                        })
                    })
                    .collect(),
            ),
        );
        body.insert("tool_choice".to_string(), json!("auto"));
    }
    if let Some(key) = &req.prompt_cache_key {
        body.insert("prompt_cache_key".to_string(), json!(key));
    }
    Ok(Value::Object(body))
}

fn system_text(message: &Message) -> Result<String, ModelError> {
    message
        .content
        .iter()
        .map(|part| match part {
            ContentBlock::Text(block) => Ok(block.text.as_str()),
            _ => Err(ModelError::invalid_request(
                "system messages may only contain text",
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("\n"))
}

/// 一条消息展开成若干 input 条目：先回传 reasoning，再是消息内容，最后是 Tool Call 与结果。
/// 这与模型输出时的顺序一致。
fn encode_message_items(message: &Message) -> Result<Vec<Value>, ModelError> {
    let mut items = Vec::new();
    for part in &message.content {
        if let ContentBlock::ProviderOpaque(block) = part
            && block.kind == REASONING_ITEM_KIND
        {
            items.push(replayable_item(&block.payload));
        }
    }

    let content = message
        .content
        .iter()
        .filter_map(|part| encode_content_part(message.role, part).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    if !content.is_empty() {
        items.push(json!({
            "type": "message",
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
            _ => {}
        }
    }
    Ok(items)
}

/// 服务端给的条目 id（`rs_…`、`msg_…`）原样回传。
/// 没有前缀的 id 去掉。与 Codex 的 `prepare_response_items_for_request`
/// （`codex-rs/core/src/client.rs`）和 `ResponseItemId::is_prefixed` 一致。
fn replayable_item(payload: &Value) -> Value {
    let mut item = payload.clone();
    if let Some(object) = item.as_object_mut()
        && !object
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(is_prefixed_id)
    {
        object.remove("id");
    }
    item
}

fn is_prefixed_id(id: &str) -> bool {
    id.split_once('_')
        .is_some_and(|(prefix, suffix)| !prefix.is_empty() && !suffix.is_empty())
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

/// 消息内容中的一个块。Tool Call、Tool Result、思考文本与回传条目不在这里编码，返回 `None`。
fn encode_content_part(role: Role, part: &ContentBlock) -> Result<Option<Value>, ModelError> {
    let encoded = match part {
        ContentBlock::Text(block) => {
            // 助手输出在回传时是 `output_text`，其他角色的文本是 `input_text`。
            let kind = if role == Role::Assistant {
                "output_text"
            } else {
                "input_text"
            };
            json!({ "type": kind, "text": block.text })
        }
        ContentBlock::Data(block) => match &block.source {
            DataSource::Url { url, media_type } if media_type.starts_with("image/") => {
                json!({ "type": "input_image", "image_url": url })
            }
            DataSource::Base64(source) if source.media_type.starts_with("image/") => json!({
                "type": "input_image",
                "image_url": format!("data:{};base64,{}", source.media_type, source.data),
            }),
            DataSource::FileId { id } => json!({ "type": "input_file", "file_id": id }),
            DataSource::Url { media_type, .. } => {
                return Err(ModelError::invalid_request(format!(
                    "Responses input does not accept media type {media_type}"
                )));
            }
            DataSource::Base64(source) => {
                return Err(ModelError::invalid_request(format!(
                    "Responses input does not accept media type {}",
                    source.media_type
                )));
            }
        },
        // 思考文本只用于显示；回传靠 reasoning 条目。
        ContentBlock::Thinking(_)
        | ContentBlock::ProviderOpaque(_)
        | ContentBlock::ToolCall(_)
        | ContentBlock::ToolResult(_) => return Ok(None),
    };
    Ok(Some(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderOpaqueBlock, ToolDefinition};

    fn assistant_with_reasoning() -> Message {
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::thinking("checking the file"),
                ContentBlock::text("Done."),
                ContentBlock::ProviderOpaque(ProviderOpaqueBlock {
                    kind: REASONING_ITEM_KIND.to_string(),
                    payload: json!({
                        "type": "reasoning",
                        "id": "rs_1",
                        "summary": [],
                        "encrypted_content": "gAAA"
                    }),
                }),
            ],
        }
    }

    #[test]
    fn drops_only_unprefixed_item_ids() {
        assert_eq!(replayable_item(&json!({"id": "msg_1"}))["id"], "msg_1");
        assert!(replayable_item(&json!({"id": "local"})).get("id").is_none());
        assert!(replayable_item(&json!({"id": "_x"})).get("id").is_none());
    }

    #[test]
    fn system_text_becomes_instructions_and_store_is_false() {
        let mut req = ModelRequest::text("m", "hi");
        req.messages
            .insert(0, Message::text(Role::System, "be brief"));
        let body = encode_request(&req).unwrap();

        assert_eq!(body["instructions"], "be brief");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"].as_array().unwrap().len(), 1);
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    }

    #[test]
    fn replays_reasoning_before_the_assistant_message_with_its_server_id() {
        let mut req = ModelRequest::text("m", "hi");
        req.messages.push(assistant_with_reasoning());
        let body = encode_request(&req).unwrap();
        let input = body["input"].as_array().unwrap();

        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(input[1]["encrypted_content"], "gAAA");
        assert_eq!(input[1]["id"], "rs_1");
        assert_eq!(input[2]["role"], "assistant");
        assert_eq!(input[2]["content"][0]["type"], "output_text");
        assert_eq!(input[2]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_selected_effort_is_sent_verbatim_and_tools_are_not_strict() {
        let mut req = ModelRequest::text("m", "hi").with_reasoning_effort("xhigh");
        req.tools.push(ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({ "type": "object" }),
        });
        req.prompt_cache_key = Some("session-1".to_string());
        let body = encode_request(&req).unwrap();

        assert_eq!(body["reasoning"]["effort"], "xhigh");
        assert!(body.get("include").is_none());
        assert_eq!(body["tools"][0]["strict"], false);
        assert_eq!(body["prompt_cache_key"], "session-1");
    }

    #[test]
    fn no_effort_sends_no_reasoning_fields() {
        let req = ModelRequest::text("m", "hi");
        let body = encode_request(&req).unwrap();

        assert!(body.get("reasoning").is_none());
        assert!(body.get("include").is_none());
    }
}
