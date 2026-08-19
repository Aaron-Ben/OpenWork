use futures_util::StreamExt;
use openwork_credentials::PostgresCredentialStore;
use openwork_models::{
    ProviderFactory,
    model::{ModelCallOptions, ModelError, ModelEvent, ModelRequest, ThinkingConfig},
    provider::{ApiCredential, ProviderKind, ProviderProfile, ProviderRuntimeConfig},
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use super::{ParsedDecision, TriageParseError, parse_decision};
use crate::model::{AgendaCandidate, Agent, TriageSettings};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageMessage {
    pub author_id: String,
    pub sequence: i64,
    pub body: String,
}

#[derive(Debug)]
pub struct TriageContext<'a> {
    pub agent: &'a Agent,
    pub room_id: &'a str,
    pub messages: &'a [TriageMessage],
}

#[derive(Debug)]
pub struct AgendaTriageContext<'a> {
    pub agent: &'a Agent,
    pub candidate: &'a AgendaCandidate,
}

#[derive(Debug)]
pub struct DmLoopContext<'a> {
    pub agent: &'a Agent,
    pub room_id: &'a str,
    pub messages: &'a [TriageMessage],
}

#[derive(Debug)]
pub struct SupportDecision {
    pub decision: ParsedDecision,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Clone)]
pub struct TriageClient {
    credentials: Result<PostgresCredentialStore, String>,
    factory: ProviderFactory,
}

impl TriageClient {
    pub fn new(pool: PgPool) -> Self {
        Self {
            credentials: PostgresCredentialStore::from_env(pool).map_err(|error| error.to_string()),
            factory: ProviderFactory::default(),
        }
    }

    pub fn with_credential_store(credentials: PostgresCredentialStore) -> Self {
        Self {
            credentials: Ok(credentials),
            factory: ProviderFactory::default(),
        }
    }

    pub async fn decide(
        &self,
        settings: &TriageSettings,
        context: TriageContext<'_>,
    ) -> Result<SupportDecision, TriageClientError> {
        let prompt = json!({
            "task": "Decide whether this candidate Agent should wake for the new room messages. Speaking, reacting, or carrying out another requested action are all actionable; an explicitly requested reaction is actionable even when no prose reply is needed. Return only the required JSON object. Do not choose an Agent by responseMode; judge only whether this candidate has something useful to contribute or do.",
            "requiredShape": {
                "actionable": "boolean",
                "responseMode": "me|each|one-of-us",
                "reason": "short string",
                "promptNote": "short instruction for the candidate's main reasoning"
            },
            "candidate": {
                "id": context.agent.id,
                "displayName": context.agent.display_name,
                "role": context.agent.role,
                "bio": context.agent.bio,
                "systemPrompt": context.agent.system_prompt,
            },
            "roomId": context.room_id,
            "newMessages": context.messages,
        });
        self.invoke_decision(settings, "triage", prompt).await
    }

    pub async fn decide_agenda(
        &self,
        settings: &TriageSettings,
        context: AgendaTriageContext<'_>,
    ) -> Result<SupportDecision, TriageClientError> {
        let prompt = json!({
            "task": "Before spending an OpenCode main-reasoning turn, decide whether this Agent has real actionable shared work in this one room. Assigned or explicitly mentioned unfinished cards and a genuinely stalled exchange can be actionable. Return actionable=false when waking the main Agent would only make it inspect and conclude that there is nothing to do. Return only the required JSON object.",
            "requiredShape": {
                "actionable": "boolean",
                "responseMode": "me|each|one-of-us",
                "reason": "short explanation of why the main Agent should or should not wake",
                "promptNote": "focused execution brief for the main Agent"
            },
            "candidate": {
                "id": context.agent.id,
                "displayName": context.agent.display_name,
                "role": context.agent.role,
                "bio": context.agent.bio,
                "systemPrompt": context.agent.system_prompt,
            },
            "agenda": context.candidate,
        });
        self.invoke_decision(settings, "agenda", prompt).await
    }

    pub async fn decide_dm_progress(
        &self,
        settings: &TriageSettings,
        context: DmLoopContext<'_>,
    ) -> Result<SupportDecision, TriageClientError> {
        let prompt = json!({
            "task": "This is the mandatory every-eighth-message check for an Agent-to-Agent direct conversation. Decide whether the exchange is making concrete progress and should continue. Repetition, mutual acknowledgement with no new work, or cycling over the same point is not progress. actionable=true means continue; actionable=false means stop the loop. Return only the required JSON object.",
            "requiredShape": {
                "actionable": "boolean",
                "responseMode": "me|each|one-of-us",
                "reason": "short progress or loop explanation",
                "promptNote": "short continuation instruction, empty when stopping"
            },
            "candidate": {
                "id": context.agent.id,
                "displayName": context.agent.display_name,
                "role": context.agent.role,
            },
            "roomId": context.room_id,
            "recentExchange": context.messages,
        });
        self.invoke_decision(settings, "dm-loop", prompt).await
    }

    async fn invoke_decision(
        &self,
        settings: &TriageSettings,
        task_prefix: &str,
        prompt: Value,
    ) -> Result<SupportDecision, TriageClientError> {
        let runtime = self.load_runtime(&settings.provider_id).await?;
        let provider = self.factory.build(&runtime);
        let mut request = ModelRequest::text(&settings.model_id, prompt.to_string());
        request.temperature = Some(0.0);
        request.max_output_tokens = Some(512);
        request.thinking = Some(ThinkingConfig::disabled());
        let mut stream = provider
            .invoke(
                request,
                ModelCallOptions::new(format!("{task_prefix}-{}", Uuid::new_v4().simple())),
            )
            .await?;
        let mut deltas = String::new();
        let mut completed = None;
        while let Some(event) = stream.next().await {
            match event? {
                ModelEvent::TextDelta { delta, .. } => deltas.push_str(&delta),
                ModelEvent::ResponseCompleted { response } => {
                    completed = Some(response);
                    break;
                }
                _ => {}
            }
        }
        let response = completed.ok_or(TriageClientError::IncompleteResponse)?;
        let output = if response.text.trim().is_empty() {
            deltas
        } else {
            response.text.clone()
        };
        let decision = parse_decision(&output)?;
        let usage = response.usage;
        Ok(SupportDecision {
            decision,
            input_tokens: usage
                .and_then(|value| value.input_tokens)
                .map(i64::try_from)
                .transpose()
                .map_err(|_| TriageClientError::UsageOverflow)?,
            output_tokens: usage
                .and_then(|value| value.output_tokens)
                .map(i64::try_from)
                .transpose()
                .map_err(|_| TriageClientError::UsageOverflow)?,
        })
    }

    async fn load_runtime(
        &self,
        provider_id: &str,
    ) -> Result<ProviderRuntimeConfig, TriageClientError> {
        let credentials = self
            .credentials
            .as_ref()
            .map_err(|error| TriageClientError::Credential(error.clone()))?;
        let credential = credentials
            .load(provider_id)
            .await
            .map_err(|error| TriageClientError::Credential(error.to_string()))?
            .ok_or_else(|| TriageClientError::ProviderNotFound(provider_id.to_string()))?;
        if !credential.enabled {
            return Err(TriageClientError::ProviderDisabled(provider_id.to_string()));
        }
        let kind = ProviderKind::parse(&credential.provider_kind).ok_or_else(|| {
            TriageClientError::InvalidProviderKind(credential.provider_kind.clone())
        })?;
        let adapter_options = credential
            .config
            .get("extraBody")
            .and_then(Value::as_object)
            .filter(|value| !value.is_empty())
            .cloned();
        Ok(ProviderRuntimeConfig {
            profile: ProviderProfile {
                id: credential.provider_id.clone(),
                name: credential.display_name.clone(),
                base_url: credential.base_url.clone(),
                kind,
                models: Vec::new(),
                enabled: credential.enabled,
            },
            credential: ApiCredential::new(credential.api_key().to_string()),
            adapter_options: adapter_options.map(|value: Map<String, Value>| value),
        })
    }
}

#[derive(Debug, Error)]
pub enum TriageClientError {
    #[error("triage credential access failed: {0}")]
    Credential(String),
    #[error("triage provider was not found: {0}")]
    ProviderNotFound(String),
    #[error("triage provider is disabled: {0}")]
    ProviderDisabled(String),
    #[error("triage provider kind is invalid: {0}")]
    InvalidProviderKind(String),
    #[error("triage model call failed: {0}")]
    Model(#[from] ModelError),
    #[error("triage model stream ended without a completed response")]
    IncompleteResponse,
    #[error(transparent)]
    Parse(#[from] TriageParseError),
    #[error("triage token usage exceeded the database integer range")]
    UsageOverflow,
}
