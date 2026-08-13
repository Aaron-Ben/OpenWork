use std::collections::HashSet;

use openwork_chat_state::ConversationView;
use openwork_models::model::{Message, ModelRequest, Role, ToolDefinition};
use thiserror::Error;

use crate::context::{ContextBudgetError, ContextBudgetEstimate, ResolvedSystemContext};

/// The three materialized input regions plus the resolved model for one call.
pub(crate) struct ModelRequestInput<'a> {
    model: &'a str,
    system_context: &'a ResolvedSystemContext,
    conversation: ConversationView,
    tool_definitions: &'a [ToolDefinition],
}

impl<'a> ModelRequestInput<'a> {
    pub(crate) fn new(
        model: &'a str,
        system_context: &'a ResolvedSystemContext,
        conversation: ConversationView,
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

/// The only Core boundary that assembles a provider-neutral model request.
pub(crate) struct ModelRequestBuilder;

impl ModelRequestBuilder {
    pub(crate) fn build(
        input: ModelRequestInput<'_>,
    ) -> Result<BuiltModelRequest, ModelRequestBuildError> {
        validate_system_context(input.system_context)?;
        validate_conversation(&input.conversation)?;
        let conversation = input.conversation;

        let max_output_tokens = None;
        let context_budget = ContextBudgetEstimate::measure(
            input.system_context,
            &conversation,
            input.tool_definitions,
            max_output_tokens,
        )?;

        let mut messages =
            Vec::with_capacity(input.system_context.parts().len() + conversation.messages.len());
        messages.extend(input.system_context.parts().iter().map(|part| Message {
            role: Role::System,
            content: part.content.clone(),
        }));
        messages.extend(conversation.messages);

        Ok(BuiltModelRequest {
            request: ModelRequest {
                model: input.model.to_string(),
                messages,
                temperature: None,
                top_p: None,
                max_output_tokens,
                thinking: None,
                tools: input.tool_definitions.to_vec(),
            },
            context_budget,
        })
    }
}

fn validate_system_context(
    system_context: &ResolvedSystemContext,
) -> Result<(), ModelRequestBuildError> {
    let mut seen_keys = HashSet::with_capacity(system_context.parts().len());
    for part in system_context.parts() {
        if part.key.trim().is_empty() {
            return Err(ModelRequestBuildError::EmptySystemContextKey);
        }
        if part.content.is_empty() {
            return Err(ModelRequestBuildError::EmptySystemContext(part.key.clone()));
        }
        if !seen_keys.insert(part.key.as_str()) {
            return Err(ModelRequestBuildError::DuplicateSystemContext(
                part.key.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_conversation(conversation: &ConversationView) -> Result<(), ModelRequestBuildError> {
    if conversation
        .messages
        .iter()
        .any(|message| message.role == Role::System)
    {
        return Err(ModelRequestBuildError::SystemMessageInConversation);
    }
    Ok(())
}

pub(crate) struct BuiltModelRequest {
    pub(crate) request: ModelRequest,
    pub(crate) context_budget: ContextBudgetEstimate,
}

#[derive(Debug, Error)]
pub(crate) enum ModelRequestBuildError {
    #[error("system context key must not be empty")]
    EmptySystemContextKey,
    #[error("system context part must not be empty: {0}")]
    EmptySystemContext(String),
    #[error("duplicate system context key: {0}")]
    DuplicateSystemContext(String),
    #[error("conversation view must not contain system messages")]
    SystemMessageInConversation,
    #[error(transparent)]
    Budget(#[from] ContextBudgetError),
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{ContentBlock, Message, Role, ToolDefinition};

    use super::*;
    use crate::context::{ResolvedSystemContext, SystemContextPart};

    fn system_context(parts: Vec<SystemContextPart>) -> ResolvedSystemContext {
        ResolvedSystemContext::for_test(parts)
    }

    fn part(key: &str, text: &str) -> SystemContextPart {
        SystemContextPart::new(key, vec![ContentBlock::text(text)])
    }

    #[test]
    fn builder_is_the_single_pre_provider_entrypoint() {
        let system_context = system_context(vec![part("core/agent-system", "system")]);
        let tool = ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };

        let prepared = ModelRequestBuilder::build(ModelRequestInput::new(
            "test-model",
            &system_context,
            ConversationView {
                messages: vec![Message::text(Role::User, "hello")],
            },
            std::slice::from_ref(&tool),
        ))
        .expect("request");

        assert_eq!(prepared.request.model, "test-model");
        assert_eq!(
            prepared.request.messages,
            [
                Message::text(Role::System, "system"),
                Message::text(Role::User, "hello"),
            ]
        );
        assert_eq!(prepared.request.tools, [tool]);
        assert_eq!(prepared.request.temperature, None);
        assert_eq!(prepared.request.max_output_tokens, None);
        assert_eq!(prepared.request.thinking, None);
        assert!(prepared.context_budget.estimated_input_tokens > 0);
    }

    #[test]
    fn preserves_the_system_materialization_order() {
        let system_context = system_context(vec![
            part("core/agent-system", "agent"),
            part("project/z", "project-z"),
            part("project/a", "project-a"),
        ]);

        let prepared = ModelRequestBuilder::build(ModelRequestInput::new(
            "test-model",
            &system_context,
            ConversationView {
                messages: vec![Message::text(Role::User, "hello")],
            },
            &[],
        ))
        .expect("request");

        let text: Vec<_> = prepared
            .request
            .messages
            .iter()
            .map(|message| match &message.content[0] {
                ContentBlock::Text(block) => block.text.as_str(),
                other => panic!("unexpected block: {other:?}"),
            })
            .collect();
        assert_eq!(text, ["agent", "project-z", "project-a", "hello"]);
    }

    #[test]
    fn rejects_duplicate_system_context_keys() {
        let system_context = system_context(vec![
            part("core/agent-system", "first"),
            part("core/agent-system", "second"),
        ]);

        let result = ModelRequestBuilder::build(ModelRequestInput::new(
            "test-model",
            &system_context,
            ConversationView {
                messages: Vec::new(),
            },
            &[],
        ));

        assert!(matches!(
            result,
            Err(ModelRequestBuildError::DuplicateSystemContext(key))
                if key == "core/agent-system"
        ));
    }

    #[test]
    fn rejects_invalid_system_context_and_conversation() {
        let empty_key = system_context(vec![part(" ", "system")]);
        assert!(matches!(
            ModelRequestBuilder::build(ModelRequestInput::new(
                "test-model",
                &empty_key,
                ConversationView {
                    messages: Vec::new(),
                },
                &[],
            )),
            Err(ModelRequestBuildError::EmptySystemContextKey)
        ));

        let empty_content = ResolvedSystemContext::for_test(vec![SystemContextPart::new(
            "core/agent-system",
            Vec::new(),
        )]);
        assert!(matches!(
            ModelRequestBuilder::build(ModelRequestInput::new(
                "test-model",
                &empty_content,
                ConversationView {
                    messages: Vec::new(),
                },
                &[],
            )),
            Err(ModelRequestBuildError::EmptySystemContext(key))
                if key == "core/agent-system"
        ));

        let valid = system_context(vec![part("core/agent-system", "system")]);
        assert!(matches!(
            ModelRequestBuilder::build(ModelRequestInput::new(
                "test-model",
                &valid,
                ConversationView {
                    messages: vec![Message::text(Role::System, "not allowed")],
                },
                &[],
            )),
            Err(ModelRequestBuildError::SystemMessageInConversation)
        ));
    }
}
