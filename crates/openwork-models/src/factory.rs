use crate::{
    model::{Message, ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest, Role},
    provider::ProviderRuntimeConfig,
};
use futures_util::StreamExt;

use crate::adapters::ProviderAdapter;
use crate::{HttpProviderConfig, HttpTransport, RetryPolicy, RetryingModelPort};

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
        Box::new(ProviderAdapter::new(
            http,
            self.transport.clone(),
            config.profile.kind.driver(),
            config.adapter_options.clone(),
        ))
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
                    top_p: None,
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
    use crate::provider::{ApiCredential, ModelTier, ProviderKind, ProviderModel, ProviderProfile};

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
                    capabilities: None,
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
        ] {
            let _provider = factory.build(&sample_config(kind));
        }

        assert!(transport.shares_lifecycle_with(&factory.transport));
    }
}
