use futures_util::StreamExt;
use openwork_protocol::{
    model::{Message, ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest, Role},
    provider::{ProviderKind, ProviderRuntimeConfig},
};

use crate::{
    AnthropicProvider, DeepSeekProvider, GlmProvider, HttpProviderConfig, HttpTransport,
    KimiProvider, OpenAiCompatibleChatProvider, OpenAiProvider, QwenProvider, RetryPolicy,
    RetryingModelPort,
};

/// 在应用生命周期内持有共享 HTTP Transport，并为每份运行时配置组装 Adapter。
#[derive(Debug, Clone)]
pub struct ProviderFactory {
    transport: HttpTransport,
    retry_policy: RetryPolicy,
}

impl ProviderFactory {
    pub fn new(transport: HttpTransport) -> Self {
        Self {
            transport,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// 将持久化配置组装为厂商 Adapter，并统一套用 transport retry。
    pub fn build(&self, config: &ProviderRuntimeConfig) -> Box<dyn ModelPort> {
        Box::new(RetryingModelPort::new(
            self.build_adapter(config),
            self.retry_policy.clone(),
        ))
    }

    fn build_adapter(&self, config: &ProviderRuntimeConfig) -> Box<dyn ModelPort> {
        let http = HttpProviderConfig::new(&config.profile.base_url, config.credential.expose());
        let transport = self.transport.clone();
        match config.profile.kind {
            ProviderKind::Openai => Box::new(OpenAiProvider::new(http, transport)),
            ProviderKind::Glm => {
                let mut provider = GlmProvider::new(http, transport);
                if let Some(extra_body) = config.adapter_options.as_ref() {
                    provider = provider.with_extra_body(extra_body.clone());
                }
                Box::new(provider)
            }
            ProviderKind::Kimi => {
                let mut provider = KimiProvider::new(http, transport);
                if let Some(extra_body) = config.adapter_options.as_ref() {
                    provider = provider.with_extra_body(extra_body.clone());
                }
                Box::new(provider)
            }
            ProviderKind::Deepseek => {
                let mut provider = DeepSeekProvider::new(http, transport);
                if let Some(extra_body) = config.adapter_options.as_ref() {
                    provider = provider.with_extra_body(extra_body.clone());
                }
                Box::new(provider)
            }
            ProviderKind::Qwen => {
                let mut provider = QwenProvider::new(http, transport);
                if let Some(extra_body) = config.adapter_options.as_ref() {
                    provider = provider.with_extra_body(extra_body.clone());
                }
                Box::new(provider)
            }
            ProviderKind::Anthropic => Box::new(AnthropicProvider::new(http, transport)),
            ProviderKind::OpenaiCompatible => {
                let mut provider = OpenAiCompatibleChatProvider::new(http, transport);
                if let Some(extra_body) = config.adapter_options.as_ref() {
                    provider = provider.with_extra_body(extra_body.clone());
                }
                Box::new(provider)
            }
        }
    }

    /// 发出最小生成请求验证配置。UI 如何呈现结果不属于 Provider Adapter。
    pub async fn test(
        &self,
        config: &ProviderRuntimeConfig,
        model: &str,
    ) -> Result<(), ModelError> {
        let provider = self.build(config);
        let mut stream = provider
            .invoke(
                ModelRequest {
                    model: model.to_string(),
                    messages: vec![Message::text(Role::User, "ping")],
                    temperature: None,
                    max_output_tokens: Some(16),
                    thinking: None,
                    tools: Vec::new(),
                },
                ModelCallOptions::new("provider-test"),
            )
            .await?;
        while let Some(item) = stream.next().await {
            if matches!(item?, ModelEvent::ResponseCompleted { .. }) {
                return Ok(());
            }
        }
        Err(ModelError::protocol(
            "provider test stream ended without ResponseCompleted",
        ))
    }
}

impl Default for ProviderFactory {
    fn default() -> Self {
        Self::new(HttpTransport::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::provider::{ApiCredential, ModelTier, ProviderModel, ProviderProfile};

    fn sample_config(kind: ProviderKind) -> ProviderRuntimeConfig {
        ProviderRuntimeConfig {
            profile: ProviderProfile {
                id: "test".to_string(),
                name: "provider".to_string(),
                base_url: "https://example.com".to_string(),
                kind,
                models: vec![ProviderModel {
                    model_id: "test-model".to_string(),
                    display_name: None,
                    model_tier: ModelTier::Plus,
                    enabled: true,
                }],
                enabled: true,
            },
            credential: ApiCredential::new("test-key"),
            adapter_options: None,
        }
    }

    #[test]
    fn builds_every_supported_adapter_from_one_transport() {
        let transport = HttpTransport::default();
        let factory = ProviderFactory::new(transport.clone());

        for kind in [
            ProviderKind::Openai,
            ProviderKind::Glm,
            ProviderKind::Kimi,
            ProviderKind::Deepseek,
            ProviderKind::Qwen,
            ProviderKind::Anthropic,
            ProviderKind::OpenaiCompatible,
        ] {
            let _provider = factory.build(&sample_config(kind));
        }

        assert!(transport.shares_lifecycle_with(&factory.transport));
    }
}
