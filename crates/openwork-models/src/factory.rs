use std::time::Duration;

use crate::{
    model::{Message, ModelCallOptions, ModelError, ModelEvent, ModelPort, ModelRequest, Role},
    provider::ProviderRuntimeConfig,
};
use futures_util::StreamExt;

use crate::adapters::ResponsesAdapter;
use crate::{HttpProviderConfig, HttpTransport, RetryPolicy, RetryingModelPort};

/// Codex 的连接重试退避从 200ms 起指数增长（`codex-rs/codex-client/src/retry.rs`）。
const RETRY_BASE_DELAY: Duration = Duration::from_millis(200);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(8);

/// 在应用生命周期内持有共享 HTTP Transport，并为每份 Provider 配置组装 Responses 适配器。
#[derive(Debug, Clone, Default)]
pub struct ProviderFactory {
    transport: HttpTransport,
}

impl ProviderFactory {
    pub fn new(transport: HttpTransport) -> Self {
        Self { transport }
    }

    /// 组装可调用的模型端口，重试次数取自 Provider 配置。调用方还应把
    /// [`crate::provider::ProviderSettings::apply_to`] 用在每次调用的参数上。
    pub fn build(&self, config: &ProviderRuntimeConfig) -> Box<dyn ModelPort> {
        let policy = RetryPolicy::new(
            config.settings.request_max_retries() as usize + 1,
            RETRY_BASE_DELAY,
            RETRY_MAX_DELAY,
        );
        Box::new(RetryingModelPort::new(self.build_adapter(config), policy))
    }

    fn build_adapter(&self, config: &ProviderRuntimeConfig) -> Box<dyn ModelPort> {
        let http = HttpProviderConfig::new(
            &config.settings.base_url,
            config.credential.expose(),
            config.settings.http_headers.clone(),
            config.settings.query_params.clone(),
        );
        Box::new(ResponsesAdapter::new(http, self.transport.clone()))
    }

    /// 发出最小生成请求验证配置。界面怎样呈现结果不属于这里。
    pub async fn test(
        &self,
        config: &ProviderRuntimeConfig,
        model: &str,
    ) -> Result<(), ModelError> {
        let provider = self.build(config);
        let mut request = ModelRequest::text(model, "ping");
        request.messages = vec![Message::text(Role::User, "ping")];
        request.max_output_tokens = Some(16);
        let options = config
            .settings
            .apply_to(ModelCallOptions::new("provider-test"));
        let mut stream = provider.invoke(request, options).await?;
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
