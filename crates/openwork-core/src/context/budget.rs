use std::io::{self, Write};

use openwork_chat_state::ConversationContextView;
use openwork_models::model::{Message, ToolDefinition};
use serde::Serialize;
use thiserror::Error;

use super::ResolvedSystemContext;

const ESTIMATED_BYTES_PER_TOKEN: u64 = 4;

/// Provider-neutral, preflight estimate of the three materialized input regions.
///
/// Core uses this provider-neutral estimate both for inspection and for the
/// application-configured pre-sampling compaction threshold. It excludes
/// provider framing and never performs truncation or rejection by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextBudgetEstimate {
    pub(crate) system_context_tokens: u64,
    pub(crate) conversation_tokens: u64,
    pub(crate) tool_surface_tokens: u64,
    pub(crate) estimated_input_tokens: u64,
    pub(crate) reserved_output_tokens: Option<u32>,
}

impl ContextBudgetEstimate {
    pub(crate) fn measure(
        system_context: &ResolvedSystemContext,
        conversation: &[Message],
        tool_definitions: &[ToolDefinition],
        reserved_output_tokens: Option<u32>,
    ) -> Result<Self, ContextBudgetError> {
        let mut system_context_bytes = 0_u64;
        for part in system_context.parts() {
            system_context_bytes =
                system_context_bytes.saturating_add(serialized_bytes(&part.content)?);
        }

        let mut conversation_bytes = 0_u64;
        for message in conversation {
            conversation_bytes =
                conversation_bytes.saturating_add(serialized_bytes(&message.content)?);
        }
        let tool_surface_bytes = if tool_definitions.is_empty() {
            0
        } else {
            serialized_bytes(&tool_definitions)?
        };

        let system_context_tokens = estimate_tokens(system_context_bytes);
        let conversation_tokens = estimate_tokens(conversation_bytes);
        let tool_surface_tokens = estimate_tokens(tool_surface_bytes);

        Ok(Self {
            system_context_tokens,
            conversation_tokens,
            tool_surface_tokens,
            estimated_input_tokens: system_context_tokens
                .saturating_add(conversation_tokens)
                .saturating_add(tool_surface_tokens),
            reserved_output_tokens,
        })
    }
}

/// Measure the conversation region on its own, on the same basis
/// [`ContextBudgetEstimate::measure`] uses.
///
/// Compaction only ever replaces the conversation, so a before/after pair
/// measured this way isolates what the compaction actually reclaimed from
/// unrelated System Context or tool-surface drift.
pub(crate) fn estimate_conversation_tokens(
    conversation: &ConversationContextView,
) -> Result<u64, ContextBudgetError> {
    let mut bytes = 0_u64;
    for item in &conversation.items {
        bytes = bytes.saturating_add(serialized_bytes(&item.message.content)?);
    }
    Ok(estimate_tokens(bytes))
}

fn serialized_bytes(value: &impl Serialize) -> Result<u64, ContextBudgetError> {
    let mut counter = ByteCounter::default();
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.bytes)
}

#[derive(Default)]
struct ByteCounter {
    bytes: u64,
}

impl Write for ByteCounter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn estimate_tokens(bytes: u64) -> u64 {
    bytes.saturating_add(ESTIMATED_BYTES_PER_TOKEN - 1) / ESTIMATED_BYTES_PER_TOKEN
}

#[derive(Debug, Error)]
pub(crate) enum ContextBudgetError {
    #[error("failed to measure model request context: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{ContentBlock, Message, Role};

    use super::*;
    use crate::context::{ResolvedSystemContext, SystemContextPart};

    #[test]
    fn measures_the_three_input_regions_without_mutating_them() {
        let system_context = ResolvedSystemContext::for_test(vec![
            SystemContextPart::new("core/agent-system", vec![ContentBlock::text("system")]),
            SystemContextPart::new("project/AGENTS.md", vec![ContentBlock::text("project")]),
        ]);
        let conversation = vec![Message::text(Role::User, "hello")];
        let tools = vec![ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let estimate =
            ContextBudgetEstimate::measure(&system_context, &conversation, &tools, Some(256))
                .expect("estimate");

        assert!(estimate.system_context_tokens > 0);
        assert!(estimate.conversation_tokens > 0);
        assert!(estimate.tool_surface_tokens > 0);
        assert_eq!(
            estimate.estimated_input_tokens,
            estimate.system_context_tokens
                + estimate.conversation_tokens
                + estimate.tool_surface_tokens
        );
        assert_eq!(estimate.reserved_output_tokens, Some(256));
        assert_eq!(system_context.parts().len(), 2);
        assert_eq!(conversation.len(), 1);
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn token_estimate_rounds_up_small_payloads() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
    }

    #[test]
    fn empty_tool_surface_has_no_tool_budget() {
        let system_context = ResolvedSystemContext::for_test(Vec::new());
        let conversation = vec![Message::text(Role::User, "hello")];

        let estimate = ContextBudgetEstimate::measure(&system_context, &conversation, &[], None)
            .expect("estimate");

        assert_eq!(estimate.tool_surface_tokens, 0);
    }
}
