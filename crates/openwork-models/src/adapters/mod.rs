pub(crate) mod anthropic_messages;
pub(crate) mod error;
mod error_dialect;
pub(crate) mod openai_chat;
pub(crate) mod openai_responses;

use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::model::{ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream};
use crate::provider::ProviderDriver;
use crate::{HttpProviderConfig, HttpTransport};

/// One HTTP adapter for every supported model wire protocol.
///
/// Protocol-specific request encoding and stream decoding stay in their own
/// modules; this type only owns shared transport/configuration and dispatches
/// exhaustively by [`ProviderDriver`].
#[derive(Clone)]
pub(crate) struct ProviderAdapter {
    config: HttpProviderConfig,
    transport: HttpTransport,
    driver: ProviderDriver,
    extra_body: Map<String, Value>,
}

impl ProviderAdapter {
    pub(crate) fn new(
        config: HttpProviderConfig,
        transport: HttpTransport,
        driver: ProviderDriver,
        extra_body: Option<Map<String, Value>>,
    ) -> Self {
        Self {
            config,
            transport,
            driver,
            extra_body: extra_body.unwrap_or_default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn driver(&self) -> ProviderDriver {
        self.driver
    }
}

impl std::fmt::Debug for ProviderAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderAdapter")
            .field("config", &self.config)
            .field("driver", &self.driver)
            .field("has_extra_body", &(!self.extra_body.is_empty()))
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl ModelPort for ProviderAdapter {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        match self.driver {
            ProviderDriver::OpenaiResponses => {
                openai_responses::start_stream(&self.config, &self.transport, request).await
            }
            ProviderDriver::AnthropicMessages => {
                anthropic_messages::start_stream(&self.config, &self.transport, request).await
            }
            ProviderDriver::OpenaiChat(dialect) => {
                openai_chat::start_stream(
                    &self.config,
                    &self.transport,
                    dialect,
                    &self.extra_body,
                    request,
                )
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderAdapter;
    use crate::provider::{OpenAiChatDialect, ProviderDriver};
    use crate::{HttpProviderConfig, HttpTransport};

    #[test]
    fn one_adapter_represents_every_wire_protocol() {
        for driver in [
            ProviderDriver::OpenaiResponses,
            ProviderDriver::AnthropicMessages,
            ProviderDriver::OpenaiChat(OpenAiChatDialect::Deepseek),
            ProviderDriver::OpenaiChat(OpenAiChatDialect::Kimi),
            ProviderDriver::OpenaiChat(OpenAiChatDialect::Qwen),
            ProviderDriver::OpenaiChat(OpenAiChatDialect::Glm),
        ] {
            let adapter = ProviderAdapter::new(
                HttpProviderConfig::new("https://example.com", "test-key"),
                HttpTransport::default(),
                driver,
                None,
            );

            assert_eq!(adapter.driver(), driver);
        }
    }

    #[test]
    fn adapter_debug_output_redacts_the_api_key() {
        let adapter = ProviderAdapter::new(
            HttpProviderConfig::new("https://example.com", "super-secret-key"),
            HttpTransport::default(),
            ProviderDriver::OpenaiResponses,
            None,
        );

        let debug = format!("{adapter:?}");
        assert!(!debug.contains("super-secret-key"));
        assert!(debug.contains("[REDACTED]"));
    }
}
