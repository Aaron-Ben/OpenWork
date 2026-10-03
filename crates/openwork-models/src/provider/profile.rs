//! Provider 连接配置。厂商是数据，不是代码：字段对应 Codex 的 `ModelProviderInfo`
//! （`codex-rs/model-provider-info/src/lib.rs`）。线协议只有 Responses，所以没有协议字段。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::model::{ModelCallOptions, ModelCapabilities};

/// Provider 下的一个可用模型。`capabilities` 为空时，能力从模型目录取（见 [`crate::catalog`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
}

/// 连接一个 Provider 所需的全部配置，不含密钥本身。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub name: String,
    /// Responses 接口的基础地址，含版本路径，例如 `https://api.moonshot.ai/v1`。请求发往 `{base_url}/responses`。
    pub base_url: String,
    /// 保存 API key 的环境变量名。与直接写入的 key 同时存在时，直接写入的 key 优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub http_headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query_params: BTreeMap<String, String>,
    /// 建立连接阶段的重试次数，不含第一次请求。为空时用 [`DEFAULT_REQUEST_MAX_RETRIES`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_max_retries: Option<u32>,
    /// 两个流事件之间的最长间隔。为空时用 [`DEFAULT_STREAM_IDLE_TIMEOUT_MS`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_idle_timeout_ms: Option<u64>,
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Codex `request_max_retries` 的默认值（`model-provider-info/src/lib.rs` 的 `to_api_provider`）。
pub const DEFAULT_REQUEST_MAX_RETRIES: u32 = 4;
/// Codex `stream_idle_timeout_ms` 的默认值：300 秒。
pub const DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 300_000;

impl ProviderSettings {
    pub fn request_max_retries(&self) -> u32 {
        self.request_max_retries
            .unwrap_or(DEFAULT_REQUEST_MAX_RETRIES)
    }

    pub fn stream_idle_timeout_ms(&self) -> u64 {
        self.stream_idle_timeout_ms
            .unwrap_or(DEFAULT_STREAM_IDLE_TIMEOUT_MS)
    }

    /// 把这个 Provider 的重试次数与流式空闲超时写进一次调用的参数。
    /// 调用方据此记录 Trace，重试层据此重试，两者使用同一份数字。
    pub fn apply_to(&self, options: ModelCallOptions) -> ModelCallOptions {
        options
            .with_max_transport_attempts(self.request_max_retries() as usize + 1)
            .with_idle_timeout(Duration::from_millis(self.stream_idle_timeout_ms()))
    }
}

/// 给界面列表用的 Provider 视图：只说明有没有直接写入的 key，不返回 key。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    #[serde(flatten)]
    pub settings: ProviderSettings,
    pub has_api_key: bool,
}

/// 新建或更新 Provider 的输入。更新时 `api_key` 为空表示保留原来的 key。
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    #[serde(flatten)]
    pub settings: ProviderSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl std::fmt::Debug for ProviderInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderInput")
            .field("settings", &self.settings)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ApiCredential(String);

impl ApiCredential {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiCredential([REDACTED])")
    }
}

/// 发起一次调用所需的配置：连接配置加上已解析的密钥。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRuntimeConfig {
    pub id: String,
    pub settings: ProviderSettings,
    pub credential: ApiCredential,
}
