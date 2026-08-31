use std::time::Duration;

use async_trait::async_trait;
use openwork_chat_state::{ConversationItem, MessageKind};
use openwork_models::model::{ContentBlock, Message, ModelCapabilities, Role, TokenUsage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Acquire, Executor, PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

use crate::session::{
    ClientRequestId, CompactionRuntimeState, ConversationCompaction, ConversationCompactionKind,
    ConversationProjectionSelector, NewConversationCompaction, ResolvedModel, SessionId,
    SessionStorage, TracePayloadSlot, TurnId, TurnOutcome,
};
use crate::session::{
    compacted_items, compaction_summary_message, validate_summary_text, validate_system_reminder,
};

use super::TRACE_PAYLOAD_ADVISORY_LOCK;
mod compaction;
mod conversation;
mod internal;
mod model;
mod plan;
mod session;
mod session_storage;
mod skill_status;
mod trace_query;
mod types;
mod validate;

const DEFAULT_DATABASE_URL: &str = "postgres://openwork:openwork@localhost:5432/openwork";

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

use internal::*;
pub(crate) use validate::is_valid_task_name;

use types::{ConversationCompactionRow, UndeliveredSubAgentResultRow};
pub use types::{
    ConversationProjectionRecord, ConversationTranscriptPage, ConversationTranscriptQuery,
    DEFAULT_COMPACTION_TRANSCRIPT_PAGE_LIMIT, DeletedOrphanSubAgent,
    MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT, ModelInput, ModelRecord, SessionInput, SessionRecord,
    StorageError, StoredMessageRecord, SubAgentReconciliation, SubAgentSessionInput,
    TraceCompleteness, TraceCompletenessState, TraceSpanPayloadRecord, TraceSpanRecord,
    TraceTurnSummary, TurnTrace, UndeliveredSubAgentResult,
};
use validate::*;

#[derive(Debug, Clone)]
pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    pub async fn connect(database_url: Option<&str>) -> Result<Self, StorageError> {
        let database_url = database_url.unwrap_or(DEFAULT_DATABASE_URL);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|connection, _metadata| {
                Box::pin(async move {
                    connection.execute("SET TIME ZONE 'UTC'").await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StorageError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }
}
