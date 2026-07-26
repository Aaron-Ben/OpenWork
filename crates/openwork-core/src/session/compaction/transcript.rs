use std::sync::Arc;

use async_trait::async_trait;
use openwork_tools::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::storage::{ConversationTranscriptQuery, MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT};

use super::super::{SessionId, SessionStorage};

pub(crate) const COMPACTION_TRANSCRIPT_TOOL_NAME: &str = "conversation_history";

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConversationTranscriptToolInput {
    /// Optional checkpoint ID. Omit it to read the latest compaction checkpoint.
    pub compaction_id: Option<String>,
    /// Return messages with a sequence greater than this cursor. Defaults to zero.
    pub after_sequence: Option<i64>,
    /// Number of exact raw messages to return. Defaults to 20 and cannot exceed 50.
    pub limit: Option<u32>,
}

pub(crate) struct ConversationTranscriptTool {
    session_id: SessionId,
    storage: Arc<dyn SessionStorage>,
}

impl ConversationTranscriptTool {
    pub(crate) fn new(session_id: SessionId, storage: Arc<dyn SessionStorage>) -> Self {
        Self {
            session_id,
            storage,
        }
    }
}

#[async_trait]
impl Tool for ConversationTranscriptTool {
    type Input = ConversationTranscriptToolInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static(COMPACTION_TRANSCRIPT_TOOL_NAME)
    }

    fn description(&self) -> &'static str {
        "Read an exact, paginated slice of raw messages covered by a conversation compaction checkpoint. Omit compactionId to use the latest checkpoint, then follow nextAfterSequence while hasMore is true. Use this only when the compacted summary lacks an exact historical detail; it never replays tools or changes session state."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        _session: &ToolSessionContext,
        _call: ToolCallContext,
        input: Self::Input,
    ) -> Result<Self::Output, ToolExecutionError> {
        if input.after_sequence.is_some_and(|sequence| sequence < 0) {
            return Err(ToolExecutionError::invalid_arguments(
                "afterSequence must be non-negative",
            ));
        }
        if input
            .limit
            .is_some_and(|limit| !(1..=MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT).contains(&limit))
        {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "limit must be between 1 and {MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT}"
            )));
        }
        let page = self
            .storage
            .read_compaction_transcript(
                &self.session_id,
                ConversationTranscriptQuery {
                    compaction_id: input.compaction_id,
                    after_sequence: input.after_sequence,
                    limit: input.limit,
                },
            )
            .await
            .map_err(ToolExecutionError::execution)?;
        let text = serde_json::to_string(&page).map_err(|error| {
            ToolExecutionError::execution(format!(
                "failed to serialize compaction transcript page: {error}"
            ))
        })?;
        Ok(TextToolOutput::new(text))
    }
}
