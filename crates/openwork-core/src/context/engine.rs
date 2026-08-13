use std::collections::HashSet;

use openwork_chat_state::ConversationContextView;
use openwork_models::model::{Message, ModelRequest, Role, ToolDefinition};
use thiserror::Error;

use super::normalize::{NormalizationError, ProjectedMessageOrigin};
use super::{
    ContextBudgetError, ContextBudgetEstimate, ModelContextLimits, NormalizationPolicy,
    ProjectionSummary, ResolvedSystemContext, normalize_for_request, project_items,
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
            &NormalizationPolicy {
                accepts_data_blocks: self.limits.accepts_data_blocks,
            },
        )?;
        let messages = normalized.messages;
        let provenance = normalized.provenance;
        let _normalization_report = normalized.report;
        validate_system_context(input.system_context)?;
        validate_conversation(&messages)?;

        let max_output_tokens = self.limits.max_output_tokens;
        let context_budget = ContextBudgetEstimate::measure(
            input.system_context,
            &messages,
            input.tool_definitions,
            max_output_tokens,
        )?;
        let system_message_count = input.system_context.parts().len();
        let mut request_messages = Vec::with_capacity(system_message_count + messages.len());
        request_messages.extend(input.system_context.parts().iter().map(|part| Message {
            role: Role::System,
            content: part.content.clone(),
        }));
        request_messages.extend(messages);

        Ok(PreparedModelCall {
            request: ModelRequest {
                model: input.model.to_string(),
                messages: request_messages,
                temperature: None,
                top_p: None,
                max_output_tokens,
                thinking: None,
                tools: input.tool_definitions.to_vec(),
            },
            context_budget,
            projection_summary: projected.summary,
            effective_input_tokens: self.limits.effective_input_tokens,
            auto_compact_token_limit: self.limits.auto_compact_token_limit,
            system_message_count,
            conversation_provenance: provenance,
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
    auto_compact_token_limit: u64,
    system_message_count: usize,
    conversation_provenance: Vec<ProjectedMessageOrigin>,
}

impl PreparedModelCall {
    /// 请求副本里属于 Conversation 的部分，即 System 前缀之后的全部消息。
    ///
    /// Inspector 必须用这个，而不是自己再组装一遍——两次组装就是两份事实。
    pub(crate) fn conversation_messages(&self) -> &[Message] {
        &self.request.messages[self.system_message_count..]
    }

    /// 每条 Conversation 消息的来源，与 `conversation_messages` 一一对应。
    pub(crate) fn conversation_provenance(&self) -> &[ProjectedMessageOrigin] {
        &self.conversation_provenance
    }

    /// 这次请求是否已经触到自动压缩线。
    ///
    /// 判据是**投影后的输入加上输出预留**，而不是只看输入：窗口是两者共用的，
    /// 只按输入判断会在最后一次采样时把输出挤出窗口。
    pub(crate) fn reaches_auto_compact_limit(&self) -> bool {
        let reserved_output_tokens = u64::from(
            self.context_budget
                .reserved_output_tokens
                .expect("ContextEngine always sets an explicit output limit"),
        );
        self.context_budget
            .estimated_input_tokens
            .saturating_add(reserved_output_tokens)
            >= self
                .auto_compact_token_limit
                .min(self.effective_input_tokens)
    }
}

#[derive(Debug, Error)]
pub(crate) enum ContextError {
    #[error(transparent)]
    Normalization(#[from] NormalizationError),
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

fn validate_system_context(system_context: &ResolvedSystemContext) -> Result<(), ContextError> {
    let mut seen_keys = HashSet::with_capacity(system_context.parts().len());
    for part in system_context.parts() {
        if part.key.trim().is_empty() {
            return Err(ContextError::EmptySystemContextKey);
        }
        if part.content.is_empty() {
            return Err(ContextError::EmptySystemContext(part.key.clone()));
        }
        if !seen_keys.insert(part.key.as_str()) {
            return Err(ContextError::DuplicateSystemContext(part.key.clone()));
        }
    }
    Ok(())
}

fn validate_conversation(conversation: &[Message]) -> Result<(), ContextError> {
    if conversation
        .iter()
        .any(|message| message.role == Role::System)
    {
        return Err(ContextError::SystemMessageInConversation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use openwork_chat_state::ConversationItem;
    use openwork_models::model::{
        ContentBlock, ModelCapabilities, Role, ToolCallBlock, ToolCallState,
    };

    use super::*;
    use crate::context::{ResolvedSystemContext, SystemContextPart};

    fn engine() -> ContextEngine {
        ContextEngine::new(ModelContextLimits::from_capabilities(ModelCapabilities {
            context_window_tokens: 200_000,
            max_output_tokens: 32_000,
            max_reasoning_tokens: None,
            accepts_data_blocks: true,
        }))
    }

    fn system_context() -> ResolvedSystemContext {
        ResolvedSystemContext::for_test(vec![
            SystemContextPart::new("core/agent-system", vec![ContentBlock::text("agent")]),
            SystemContextPart::new("project/AGENTS.md", vec![ContentBlock::text("project")]),
        ])
    }

    fn user(text: &str) -> ConversationItem {
        ConversationItem::persisted(text, 1, Message::text(Role::User, text))
    }

    fn part(key: &str, text: &str) -> SystemContextPart {
        SystemContextPart::new(key, vec![ContentBlock::text(text)])
    }

    fn try_prepare(
        system_context: &ResolvedSystemContext,
        items: Vec<ConversationItem>,
    ) -> Result<PreparedModelCall, ContextError> {
        engine().prepare(PrepareContextInput::new(
            "model-under-test",
            system_context,
            ConversationContextView { items },
            &[],
        ))
    }

    fn prepare(
        engine: &ContextEngine,
        system_context: &ResolvedSystemContext,
        items: Vec<ConversationItem>,
    ) -> PreparedModelCall {
        engine
            .prepare(PrepareContextInput::new(
                "model-under-test",
                system_context,
                ConversationContextView { items },
                &[],
            ))
            .expect("prepare succeeds")
    }

    /// System 片段先于 Conversation，工具面独立，且不替调用方决定采样参数。
    #[test]
    fn assembles_the_system_prefix_ahead_of_the_conversation() {
        let tool = ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({ "type": "object" }),
        };
        let system_context = ResolvedSystemContext::for_test(vec![part("core/agent-system", "s")]);

        let prepared = engine()
            .prepare(PrepareContextInput::new(
                "model-under-test",
                &system_context,
                ConversationContextView {
                    items: vec![user("hello")],
                },
                std::slice::from_ref(&tool),
            ))
            .expect("prepare succeeds");

        assert_eq!(prepared.request.model, "model-under-test");
        assert_eq!(
            prepared.request.messages,
            [
                Message::text(Role::System, "s"),
                Message::text(Role::User, "hello"),
            ]
        );
        assert_eq!(prepared.request.tools, [tool]);
        assert_eq!(prepared.request.temperature, None);
        assert_eq!(prepared.request.thinking, None);
        assert!(prepared.context_budget.estimated_input_tokens > 0);
    }

    /// System 片段按来源顺序物化，不按 key 排序——顺序是上下文优先级。
    #[test]
    fn preserves_the_system_materialization_order() {
        let system_context = ResolvedSystemContext::for_test(vec![
            part("core/agent-system", "agent"),
            part("project/z", "project-z"),
            part("project/a", "project-a"),
        ]);

        let prepared = try_prepare(&system_context, vec![user("hello")]).expect("prepare succeeds");

        let text = prepared
            .request
            .messages
            .iter()
            .map(|message| match &message.content[0] {
                ContentBlock::Text(block) => block.text.as_str(),
                other => std::panic::panic_any(format!("unexpected block: {other:?}")),
            })
            .collect::<Vec<_>>();
        assert_eq!(text, ["agent", "project-z", "project-a", "hello"]);
    }

    /// key 是片段的身份，重复意味着某个来源被静默覆盖或注入了两次。
    #[test]
    fn rejects_duplicate_system_context_keys() {
        let system_context = ResolvedSystemContext::for_test(vec![
            part("core/agent-system", "first"),
            part("core/agent-system", "second"),
        ]);

        let result = try_prepare(&system_context, Vec::new());

        assert!(matches!(
            result,
            Err(ContextError::DuplicateSystemContext(key)) if key == "core/agent-system"
        ));
    }

    /// 空 key、空正文、以及混进 Conversation 的 System 消息都必须被拒绝。
    ///
    /// 最后一条尤其重要：Conversation 里的 System 消息会绕过 System Context 的
    /// 准入检查，等于一条没人管辖的最高优先级指令。
    #[test]
    fn rejects_malformed_system_context_and_system_messages_in_conversation() {
        assert!(matches!(
            try_prepare(
                &ResolvedSystemContext::for_test(vec![part(" ", "system")]),
                Vec::new()
            ),
            Err(ContextError::EmptySystemContextKey)
        ));

        assert!(matches!(
            try_prepare(
                &ResolvedSystemContext::for_test(vec![SystemContextPart::new(
                    "core/agent-system",
                    Vec::new()
                )]),
                Vec::new()
            ),
            Err(ContextError::EmptySystemContext(key)) if key == "core/agent-system"
        ));

        assert!(matches!(
            try_prepare(
                &system_context(),
                vec![ConversationItem::persisted(
                    "system-1",
                    1,
                    Message::text(Role::System, "not allowed")
                )]
            ),
            Err(ContextError::SystemMessageInConversation)
        ));
    }

    /// 请求必须带显式输出上限，不能把额度交给 Provider 的默认值。
    ///
    /// 默认值因 Provider 而异，也会随时间变化；不设它等于放弃对窗口的控制，
    /// 预算里的输出预留也就成了一个没人兑现的数字。
    #[test]
    fn prepared_requests_carry_an_explicit_output_limit() {
        let engine = ContextEngine::new(ModelContextLimits::from_capabilities(ModelCapabilities {
            context_window_tokens: 200_000,
            max_output_tokens: 32_000,
            max_reasoning_tokens: None,
            accepts_data_blocks: true,
        }));

        let prepared = prepare(&engine, &system_context(), vec![user("hello")]);

        assert_eq!(prepared.request.max_output_tokens, Some(32_000));
    }

    /// 自动压缩的判据必须包含输出预留。
    ///
    /// 只按输入判断，会让最后一次采样刚好把输出挤出窗口——那正是最该压缩的时候。
    #[test]
    fn the_auto_compact_check_counts_the_reserved_output() {
        let engine = ContextEngine::new(ModelContextLimits::from_capabilities(ModelCapabilities {
            context_window_tokens: 1_000,
            max_output_tokens: 400,
            max_reasoning_tokens: None,
            accepts_data_blocks: true,
        }));

        let prepared = prepare(&engine, &system_context(), vec![user(&"x".repeat(1_600))]);

        assert!(
            prepared.context_budget.estimated_input_tokens < prepared.effective_input_tokens,
            "前提：只看输入时还没到线"
        );
        assert!(
            prepared.reaches_auto_compact_limit(),
            "加上输出预留后必须触线"
        );
    }

    /// §14.1 #1：稳定前缀。
    ///
    /// 状态没变、只追加了一条消息时，上一次提交的完整 input 必须是这一次的前缀。
    /// 这是 Provider 端前缀缓存能命中的前提，也是整个改造在成本上的主要回报。
    #[test]
    fn appending_a_message_keeps_the_previous_input_as_a_prefix() {
        let engine = engine();
        let system_context = system_context();
        let first_items = vec![user("第一句"), user("第二句")];
        let mut second_items = first_items.clone();
        second_items.push(user("第三句"));

        let first = prepare(&engine, &system_context, first_items);
        let second = prepare(&engine, &system_context, second_items);

        assert_eq!(
            second.request.messages[..first.request.messages.len()],
            first.request.messages[..],
            "既有前缀必须逐项一致"
        );
    }

    /// 同一输入两次准备必须完全相同，否则前缀缓存每轮都会失效。
    #[test]
    fn preparing_the_same_input_twice_is_identical() {
        let engine = engine();
        let system_context = system_context();
        let items = vec![user("同一句")];

        let first = prepare(&engine, &system_context, items.clone());
        let second = prepare(&engine, &system_context, items);

        assert_eq!(first.request, second.request);
    }

    /// §14.1 #12：Inspector 看到的就是实际提交的那一份，不是重新组装的。
    #[test]
    fn the_conversation_view_is_the_one_actually_submitted() {
        let engine = engine();
        let system_context = system_context();
        let system_part_count = system_context.parts().len();

        let prepared = prepare(&engine, &system_context, vec![user("一句话")]);

        assert_eq!(
            prepared.conversation_messages(),
            &prepared.request.messages[system_part_count..]
        );
    }

    /// 合法化补出来的消息必须能被认出来，不能冒充库里的事实。
    #[test]
    fn synthesized_messages_are_distinguishable_from_persisted_ones() {
        let engine = engine();
        let system_context = system_context();
        let assistant = ConversationItem::persisted(
            "assistant-1",
            2,
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolCall(ToolCallBlock {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    input: "{}".to_string(),
                    state: ToolCallState::Submitted,
                })],
            },
        );

        let prepared = prepare(&engine, &system_context, vec![user("看一下"), assistant]);

        let provenance = prepared.conversation_provenance();
        assert_eq!(
            provenance.len(),
            prepared.conversation_messages().len(),
            "来源必须与消息一一对应"
        );
        assert_eq!(
            provenance.last(),
            Some(&ProjectedMessageOrigin::Synthesized),
            "补齐的 Tool Result 必须标记为合成"
        );
        assert!(
            matches!(
                provenance.first(),
                Some(ProjectedMessageOrigin::Persisted { message_id }) if message_id == "看一下"
            ),
            "库里的消息必须带上它的 message id"
        );
    }
}
