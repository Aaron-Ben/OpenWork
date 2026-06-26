use async_trait::async_trait;
use openwork_protocol::ai::{
    ContentBlock, DataSource, GenerateRequest, GenerateResponse, GenerateStreamCallback,
    GenerateStreamEvent, LlmProvider, ProviderError, Role, TokenUsage,
};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::{
    config::HttpProviderConfig,
    error::{map_error_response, map_reqwest_error},
    serde_helpers::{role_supported_by_anthropic, text_from_content},
    sse::consume_sse_response,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    config: HttpProviderConfig,
    anthropic_version: String,
}

impl AnthropicProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(
            "https://api.anthropic.com",
            api_key,
        ))
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_str(&self.anthropic_version).map_err(|error| {
                ProviderError::InvalidRequest {
                    message: error.to_string(),
                }
            })?,
        );
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&self.config.api_key).map_err(|error| {
                ProviderError::InvalidRequest {
                    message: error.to_string(),
                }
            })?,
        );
        Ok(headers)
    }

    pub(crate) fn messages_request_body(req: &GenerateRequest) -> Result<Value, ProviderError> {
        if !req.tools.is_empty() {
            return Err(ProviderError::InvalidRequest {
                message: "tool calling is not supported for the Anthropic provider yet".to_string(),
            });
        }
        if req.thinking.is_some() {
            return Err(ProviderError::InvalidRequest {
                message: "Anthropic thinking mode is not mapped yet".to_string(),
            });
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

            if !role_supported_by_anthropic(message.role) {
                return Err(ProviderError::InvalidRequest {
                    message: format!("Anthropic does not support role {:?}", message.role),
                });
            }

            let content = message
                .content
                .iter()
                .map(anthropic_content_part)
                .collect::<Result<Vec<_>, _>>()?;
            messages.push(json!({
                "role": message.role.as_provider_str(),
                "content": content,
            }));
        }

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens.unwrap_or(1024),
            "stream": req.stream,
        });

        if !system_parts.is_empty() {
            body["system"] = json!(system_parts.join("\n"));
        }
        if let Some(temperature) = req.temperature {
            body["temperature"] = json!(temperature);
        }

        Ok(body)
    }

    pub(crate) fn parse_generate_response(raw: Value) -> GenerateResponse {
        let text = raw
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => part.get("text").and_then(Value::as_str),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        let usage = raw.get("usage").map(|usage| TokenUsage {
            input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
            output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
            total_tokens: match (
                usage.get("input_tokens").and_then(Value::as_u64),
                usage.get("output_tokens").and_then(Value::as_u64),
            ) {
                (Some(input), Some(output)) => Some(input + output),
                _ => None,
            },
        });

        GenerateResponse {
            text,
            reasoning_text: None,
            usage,
            raw,
            tool_calls: Vec::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        if req.stream {
            return Err(ProviderError::InvalidRequest {
                message: "streaming is not implemented for Anthropic provider yet".to_string(),
            });
        }

        let body = Self::messages_request_body(&req)?;
        let response = self
            .client
            .post(self.config.endpoint("/v1/messages"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let raw = response.json::<Value>().await.map_err(map_reqwest_error)?;
        Ok(Self::parse_generate_response(raw))
    }

    async fn stream_generate(
        &self,
        mut req: GenerateRequest,
        mut on_event: GenerateStreamCallback,
    ) -> Result<GenerateResponse, ProviderError> {
        req.stream = true;
        let body = Self::messages_request_body(&req)?;
        let response = self
            .client
            .post(self.config.endpoint("/v1/messages"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let mut text = String::new();
        let mut reasoning_text = String::new();
        let raw_events = consume_sse_response(response, |event| {
            if let Some(delta) = anthropic_text_delta(event) {
                text.push_str(&delta);
                on_event(GenerateStreamEvent::TextDelta { delta });
            }
            if let Some(delta) = anthropic_reasoning_delta(event) {
                reasoning_text.push_str(&delta);
                on_event(GenerateStreamEvent::ReasoningDelta { delta });
            }
            Ok(())
        })
        .await?;

        Ok(GenerateResponse {
            text,
            reasoning_text: if reasoning_text.is_empty() {
                None
            } else {
                Some(reasoning_text)
            },
            usage: None,
            raw: Value::Array(raw_events),
            tool_calls: Vec::new(),
        })
    }
}

fn anthropic_text_delta(event: &Value) -> Option<String> {
    let delta = event.get("delta")?;
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => delta
            .get("text")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn anthropic_reasoning_delta(event: &Value) -> Option<String> {
    let delta = event.get("delta")?;
    match delta.get("type").and_then(Value::as_str) {
        Some("thinking_delta") => delta
            .get("thinking")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn anthropic_content_part(part: &ContentBlock) -> Result<Value, ProviderError> {
    match part {
        ContentBlock::Text(block) => Ok(json!({ "type": "text", "text": block.text })),
        ContentBlock::Thinking(_) => Err(ProviderError::InvalidRequest {
            message: "Anthropic thinking blocks are not mapped yet".to_string(),
        }),
        ContentBlock::Data(block) => match &block.source {
            DataSource::Url { url, media_type } if media_type.starts_with("image/") => Ok(json!({
                "type": "image",
                "source": {
                    "type": "url",
                    "url": url,
                },
            })),
            DataSource::Base64(source) if source.media_type.starts_with("image/") => Ok(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": source.media_type,
                    "data": source.data,
                },
            })),
            DataSource::FileId { id } => Err(ProviderError::InvalidRequest {
                message: format!("Anthropic file id content is not mapped yet: {id}"),
            }),
            DataSource::Url { media_type, .. } => Err(ProviderError::InvalidRequest {
                message: format!("Anthropic data media type is not mapped: {media_type}"),
            }),
            DataSource::Base64(source) => Err(ProviderError::InvalidRequest {
                message: format!(
                    "Anthropic base64 media type is not mapped: {}",
                    source.media_type
                ),
            }),
        },
        ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_) => {
            Err(ProviderError::InvalidRequest {
                message: "tool blocks are not mapped for Anthropic yet".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::ai::{GenerateRequest, Message, Role};

    #[test]
    fn builds_messages_body_with_system_prompt() {
        let req = GenerateRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![
                Message::text(Role::System, "You are concise."),
                Message::text(Role::User, "hello"),
            ],
            temperature: Some(0.1),
            max_tokens: Some(128),
            stream: false,
            thinking: None,
            tools: Vec::new(),
        };

        let body = AnthropicProvider::messages_request_body(&req).unwrap();

        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["system"], "You are concise.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["max_tokens"], 128);
    }

    #[test]
    fn rejects_tool_role_until_tool_mapping_exists() {
        let req = GenerateRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![Message::text(Role::Tool, "tool result")],
            temperature: None,
            max_tokens: None,
            stream: false,
            thinking: None,
            tools: Vec::new(),
        };

        assert!(AnthropicProvider::messages_request_body(&req).is_err());
    }

    #[test]
    fn parses_message_text_and_usage() {
        let raw = json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "text", "text": " world" }
            ],
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3
            }
        });

        let response = AnthropicProvider::parse_generate_response(raw);

        assert_eq!(response.text, "hello world");
        assert_eq!(response.usage.unwrap().total_tokens, Some(5));
    }

    #[test]
    fn extracts_anthropic_stream_deltas() {
        let text = json!({ "delta": { "type": "text_delta", "text": "hello" } });
        let reasoning = json!({ "delta": { "type": "thinking_delta", "thinking": "think" } });

        assert_eq!(anthropic_text_delta(&text).as_deref(), Some("hello"));
        assert_eq!(
            anthropic_reasoning_delta(&reasoning).as_deref(),
            Some("think")
        );
    }
}
