use openwork_chat_state::ConversationContextView;
use openwork_models::model::{ModelRequest, ToolDefinition};
use thiserror::Error;

use crate::model_call::{ModelRequestBuildError, ModelRequestBuilder, ModelRequestInput};

use super::normalize::NormalizationError;
use super::{
    ContextBudgetEstimate, ModelContextLimits, NormalizationPolicy, ResolvedSystemContext,
    ProjectionSummary, normalize_for_request, project_items,
};

pub(crate) struct ContextEngine {
    limits: ModelContextLimits,
}

impl ContextEngine {
    pub(crate) fn new(limits: ModelContextLimits) -> Self {
        Self { limits }
    }

    pub(crate) fn limits(&self) -> &ModelContextLimits {
        &self.limits
    }

    pub(crate) fn prepare(
        &self,
        input: PrepareContextInput<'_>,
    ) -> Result<PreparedModelCall, ContextError> {
        let projected = project_items(&input.conversation.items, &self.limits);
        let normalized = normalize_for_request(
            &projected.items,
            &NormalizationPolicy { accepts_data_blocks: self.limits.accepts_data_blocks },
        )?;
        let messages = normalized.messages;
        let _normalization_report = normalized.report;
        let built = ModelRequestBuilder::build(ModelRequestInput::new(
            input.model,
            input.system_context,
            messages,
            input.tool_definitions,
        ))?;

        Ok(PreparedModelCall {
            request: built.request,
            context_budget: built.context_budget,
            projection_summary: projected.summary,
            effective_input_tokens: self.limits.effective_input_tokens,
        })
    }
}

pub(crate) struct PrepareContextInput<'a> {
    model: &'a str,
    system_context: &'a ResolvedSystemContext,
    conversation: ConversationContextView,
    tool_definitions: &'a [ToolDefinition],
}

impl<'a> PrepareContextInput<'a> {
    pub(crate) fn new(
        model: &'a str,
        system_context: &'a ResolvedSystemContext,
        conversation: ConversationContextView,
        tool_definitions: &'a [ToolDefinition],
    ) -> Self {
        Self {
            model,
            system_context,
            conversation,
            tool_definitions,
        }
    }
}

pub(crate) struct PreparedModelCall {
    pub(crate) request: ModelRequest,
    pub(crate) context_budget: ContextBudgetEstimate,
    pub(crate) projection_summary: ProjectionSummary,
    effective_input_tokens: u64,
}

impl PreparedModelCall {
    pub(crate) fn reaches_input_threshold(&self, threshold_percent: u8) -> bool {
        u128::from(self.context_budget.estimated_input_tokens) * 100
            >= u128::from(self.effective_input_tokens) * u128::from(threshold_percent)
    }
}

#[derive(Debug, Error)]
pub(crate) enum ContextError {
    #[error(transparent)]
    Normalization(#[from] NormalizationError),
    #[error(transparent)]
    RequestBuild(#[from] ModelRequestBuildError),
}
