use async_trait::async_trait;
use openwork_protocol::ai::{
    ContentBlock, DataSource, EmbeddingProvider, EmbeddingRequest, EmbeddingResponse,
    GenerateRequest, GenerateResponse, GenerateStreamCallback, GenerateStreamEvent, LlmProvider,
    Message, ProviderError,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::{
    config::HttpProviderConfig,
    error::{map_error_response, map_reqwest_error},
    serde_helpers::usage_from_openai,
    sse::consume_sse_response,
};

#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    config: HttpProviderConfig,
}

impl OpenAiProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new("https://api.openai.com", api_key))
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        let auth =
            HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).map_err(|error| {
                ProviderError::InvalidRequest {
                    message: error.to_string(),
                }
            })?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    pub(crate) fn responses_request_body(req: &GenerateRequest) -> Result<Value, ProviderError> {
        if !req.tools.is_empty() {
            return Err(ProviderError::InvalidRequest {
                message: "tool calling is not supported for the OpenAI Responses API provider yet; use openai_compatible"
                    .to_string(),
            });
        }
        if req.thinking.is_some() {
            return Err(ProviderError::InvalidRequest {
                message: "OpenAI thinking mode is not mapped yet".to_string(),
            });
        }

        let input = req
            .messages
            .iter()
            .map(openai_response_message)
            .collect::<Result<Vec<_>, _>>()?;

        let mut body = json!({
            "model": req.model,
            "input": input,
            "stream": req.stream,
        });

        if let Some(temperature) = req.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = req.max_tokens {
            body["max_output_tokens"] = json!(max_tokens);
        }

        Ok(body)
    }

    pub(crate) fn embeddings_request_body(req: &EmbeddingRequest) -> Value {
        let mut body = json!({
            "model": req.model,
            "input": req.input,
        });

        if let Some(dimensions) = req.dimensions {
            body["dimensions"] = json!(dimensions);
        }

        body
    }

    pub(crate) fn parse_generate_response(raw: Value) -> GenerateResponse {
        let text = raw
            .get("output_text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| output_text_from_responses(&raw));
        let usage = usage_from_openai(&raw);

        GenerateResponse {
            text,
            reasoning_text: None,
            usage,
            raw,
            tool_calls: Vec::new(),
        }
    }

    pub(crate) fn parse_embedding_response(raw: Value) -> Result<EmbeddingResponse, ProviderError> {
        let data = raw.get("data").and_then(Value::as_array).ok_or_else(|| {
            ProviderError::Serialization {
                message: "missing embeddings data array".to_string(),
            }
        })?;

        let vectors = data
            .iter()
            .map(|item| {
                item.get("embedding")
                    .and_then(Value::as_array)
                    .ok_or_else(|| ProviderError::Serialization {
                        message: "missing embedding vector".to_string(),
                    })?
                    .iter()
                    .map(|value| {
                        value.as_f64().map(|float| float as f32).ok_or_else(|| {
                            ProviderError::Serialization {
                                message: "embedding vector contains non-number value".to_string(),
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(EmbeddingResponse {
            vectors,
            usage: usage_from_openai(&raw),
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        if req.stream {
            return Err(ProviderError::InvalidRequest {
                message: "streaming is not implemented for OpenAI provider yet".to_string(),
            });
        }

        let body = Self::responses_request_body(&req)?;
        let response = self
            .client
            .post(self.config.endpoint("/v1/responses"))
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
        let body = Self::responses_request_body(&req)?;
        let response = self
            .client
            .post(self.config.endpoint("/v1/responses"))
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
            if let Some(delta) = responses_text_delta(event) {
                text.push_str(&delta);
                on_event(GenerateStreamEvent::TextDelta { delta });
            }
            if let Some(delta) = responses_reasoning_delta(event) {
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

#[async_trait]
impl EmbeddingProvider for OpenAiProvider {
    async fn embed(&self, req: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError> {
        let body = Self::embeddings_request_body(&req);
        let response = self
            .client
            .post(self.config.endpoint("/v1/embeddings"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let raw = response.json::<Value>().await.map_err(map_reqwest_error)?;
        Self::parse_embedding_response(raw)
    }
}

fn openai_response_message(message: &Message) -> Result<Value, ProviderError> {
    let content = message
        .content
        .iter()
        .map(openai_response_content_part)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!({
        "role": message.role.as_provider_str(),
        "content": content,
    }))
}

fn openai_response_content_part(part: &ContentBlock) -> Result<Value, ProviderError> {
    match part {
        ContentBlock::Text(block) => Ok(json!({ "type": "input_text", "text": block.text })),
        ContentBlock::Thinking(_) => Err(ProviderError::InvalidRequest {
            message: "OpenAI Responses thinking history is not mapped yet".to_string(),
        }),
        ContentBlock::Data(block) => match &block.source {
            DataSource::Url { url, media_type } if media_type.starts_with("image/") => {
                Ok(json!({ "type": "input_image", "image_url": url }))
            }
            DataSource::Base64(source) if source.media_type.starts_with("image/") => Ok(json!({
                "type": "input_image",
                "image_url": format!("data:{};base64,{}", source.media_type, source.data),
            })),
            DataSource::FileId { id } => Ok(json!({ "type": "input_file", "file_id": id })),
            DataSource::Url { media_type, .. } => Err(ProviderError::InvalidRequest {
                message: format!("OpenAI Responses data media type is not mapped: {media_type}"),
            }),
            DataSource::Base64(source) => Err(ProviderError::InvalidRequest {
                message: format!(
                    "OpenAI Responses base64 media type is not mapped: {}",
                    source.media_type
                ),
            }),
        },
        ContentBlock::ToolCall(_) | ContentBlock::ToolResult(_) => {
            Err(ProviderError::InvalidRequest {
                message: "tool blocks are not mapped for OpenAI Responses yet".to_string(),
            })
        }
    }
}

fn output_text_from_responses(raw: &Value) -> String {
    raw.get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("output_text") => part.get("text").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn responses_text_delta(event: &Value) -> Option<String> {
    match event.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") | Some("response.refusal.delta") => event
            .get("delta")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn responses_reasoning_delta(event: &Value) -> Option<String> {
    match event.get("type").and_then(Value::as_str) {
        Some("response.reasoning_text.delta") | Some("response.reasoning_summary_text.delta") => {
            event
                .get("delta")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::ai::{Message, Role};

    use crate::serde_helpers::text_from_content;

    #[test]
    fn builds_responses_body() {
        let req = GenerateRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![Message::text(Role::User, "hello")],
            temperature: Some(0.2),
            max_tokens: Some(128),
            stream: false,
            thinking: None,
            tools: Vec::new(),
        };

        let body = OpenAiProvider::responses_request_body(&req).unwrap();

        assert_eq!(body["model"], "gpt-4.1");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["max_output_tokens"], 128);
    }

    #[test]
    fn rejects_unmapped_thinking_mode() {
        let req = GenerateRequest::text("gpt-4.1", "hello")
            .with_thinking(openwork_protocol::ai::ThinkingConfig::enabled());

        assert!(OpenAiProvider::responses_request_body(&req).is_err());
    }

    #[test]
    fn parses_embedding_response() {
        let raw = json!({
            "data": [{ "embedding": [0.1, 0.2] }],
            "usage": { "prompt_tokens": 3, "total_tokens": 3 }
        });

        let response = OpenAiProvider::parse_embedding_response(raw).unwrap();

        assert_eq!(response.vectors, vec![vec![0.1, 0.2]]);
        assert_eq!(response.usage.unwrap().input_tokens, Some(3));
    }

    #[test]
    fn joins_response_output_text_parts() {
        let raw = json!({
            "output": [{
                "content": [
                    { "type": "output_text", "text": "hello" },
                    { "type": "output_text", "text": " world" }
                ]
            }]
        });

        assert_eq!(
            OpenAiProvider::parse_generate_response(raw).text,
            "hello world"
        );
    }

    #[test]
    fn extracts_text_content_parts() {
        let message = Message::text(Role::System, "system prompt");
        assert_eq!(text_from_content(&message.content), "system prompt");
    }

    #[test]
    fn extracts_responses_stream_deltas() {
        let text = json!({ "type": "response.output_text.delta", "delta": "hello" });
        let reasoning = json!({ "type": "response.reasoning_text.delta", "delta": "think" });

        assert_eq!(responses_text_delta(&text).as_deref(), Some("hello"));
        assert_eq!(
            responses_reasoning_delta(&reasoning).as_deref(),
            Some("think")
        );
    }
}
