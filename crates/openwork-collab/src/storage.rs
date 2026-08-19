use std::time::Duration;

use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use thiserror::Error;
use time::PrimitiveDateTime;
use uuid::Uuid;

use crate::{
    domain::unread_count,
    migration::{self, MigrationError},
    model::{
        Agent, AgentInput, Inbox, Message, MessagePage, MessagePageAnchor, MessagePageQuery, Room,
        RoomMember, RoomSummary, SendMessageOutcome,
    },
    time::{china_now, format_china},
};

pub const DEFAULT_DATABASE_URL: &str = "postgres://openwork:openwork@localhost:5432/openwork";
pub const MESSAGE_DEDUP_WINDOW: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct CollabStorage {
    pool: PgPool,
}

impl CollabStorage {
    pub async fn connect(database_url: Option<&str>) -> Result<Self, StorageError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    connection.execute("SET TIME ZONE 'UTC'").await?;
                    Ok(())
                })
            })
            .connect(database_url.unwrap_or(DEFAULT_DATABASE_URL))
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
        migration::migrate(&self.pool).await?;
        Ok(())
    }

    pub async fn repair_interrupted_runs(&self) -> Result<u64, StorageError> {
        let result = sqlx::query(
            "UPDATE collab_runs
                SET status = 'interrupted',
                    ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                    error_code = COALESCE(error_code, 'daemon_restarted'),
                    error_message = COALESCE(error_message, 'daemon restarted while run was active')
              WHERE status = 'running'",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn create_agent(&self, input: &AgentInput) -> Result<Agent, StorageError> {
        validate_agent(input)?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO collab_participants (id, kind, display_name)
             VALUES ($1, 'agent', $2)",
        )
        .bind(&input.id)
        .bind(input.display_name.trim())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collab_agents (
                id, role, bio, system_prompt, provider_id, model_id, enabled
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&input.id)
        .bind(input.role.as_deref())
        .bind(input.bio.as_deref())
        .bind(input.system_prompt.trim())
        .bind(input.provider_id.trim())
        .bind(input.model_id.trim())
        .bind(input.enabled)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.agent(&input.id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("agent {}", input.id)))
    }

    pub async fn update_agent(&self, input: &AgentInput) -> Result<Agent, StorageError> {
        validate_agent(input)?;
        let mut transaction = self.pool.begin().await?;
        let participant = sqlx::query(
            "UPDATE collab_participants SET display_name = $2
             WHERE id = $1 AND kind = 'agent'",
        )
        .bind(&input.id)
        .bind(input.display_name.trim())
        .execute(&mut *transaction)
        .await?;
        let agent = sqlx::query(
            "UPDATE collab_agents SET
                role = $2, bio = $3, system_prompt = $4,
                provider_id = $5, model_id = $6, enabled = $7,
                updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(&input.id)
        .bind(input.role.as_deref())
        .bind(input.bio.as_deref())
        .bind(input.system_prompt.trim())
        .bind(input.provider_id.trim())
        .bind(input.model_id.trim())
        .bind(input.enabled)
        .execute(&mut *transaction)
        .await?;
        if participant.rows_affected() == 0 || agent.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("agent {}", input.id)));
        }
        transaction.commit().await?;
        self.agent(&input.id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("agent {}", input.id)))
    }

    pub async fn agent(&self, id: &str) -> Result<Option<Agent>, StorageError> {
        Ok(sqlx::query_as::<_, AgentRow>(
            "SELECT a.id, p.display_name, a.role, a.bio, a.system_prompt,
                    a.provider_id, a.model_id, a.opencode_session_id, a.enabled
               FROM collab_agents a
               JOIN collab_participants p ON p.id = a.id
              WHERE a.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .map(AgentRow::into_agent))
    }

    pub async fn agents(&self) -> Result<Vec<Agent>, StorageError> {
        Ok(sqlx::query_as::<_, AgentRow>(
            "SELECT a.id, p.display_name, a.role, a.bio, a.system_prompt,
                    a.provider_id, a.model_id, a.opencode_session_id, a.enabled
               FROM collab_agents a
               JOIN collab_participants p ON p.id = a.id
              ORDER BY a.created_at, a.id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(AgentRow::into_agent)
        .collect())
    }

    pub async fn set_opencode_session(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<(), StorageError> {
        let result = sqlx::query(
            "UPDATE collab_agents
                SET opencode_session_id = $2,
                    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
              WHERE id = $1",
        )
        .bind(agent_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("agent {agent_id}")));
        }
        Ok(())
    }

    pub async fn create_group_room(&self, id: &str, title: &str) -> Result<Room, StorageError> {
        if id.trim().is_empty() || title.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "room id and title must not be blank".to_string(),
            ));
        }
        sqlx::query("INSERT INTO collab_rooms (id, kind, title) VALUES ($1, 'group', $2)")
            .bind(id)
            .bind(title.trim())
            .execute(&self.pool)
            .await?;
        self.room(id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("room {id}")))
    }

    pub async fn room(&self, id: &str) -> Result<Option<Room>, StorageError> {
        Ok(sqlx::query_as::<_, RoomRow>(
            "SELECT id, kind, title, next_seq FROM collab_rooms WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .map(RoomRow::into_room))
    }

    pub async fn room_summaries(
        &self,
        participant_id: &str,
    ) -> Result<Vec<RoomSummary>, StorageError> {
        let rooms = sqlx::query_as::<_, RoomSummaryRow>(
            "SELECT r.id, r.kind, r.title, r.next_seq,
                    rm.last_read_seq, rm.muted
               FROM collab_room_members rm
               JOIN collab_rooms r ON r.id = rm.room_id
              WHERE rm.participant_id = $1
              ORDER BY r.last_message_at DESC NULLS LAST, r.created_at, r.id",
        )
        .bind(participant_id)
        .fetch_all(&self.pool)
        .await?;
        let mut summaries = Vec::with_capacity(rooms.len());
        for room in rooms {
            let members = sqlx::query_as::<_, RoomMemberRow>(
                "SELECT p.id, p.display_name, p.kind, COALESCE(a.enabled, TRUE) AS enabled
                   FROM collab_room_members rm
                   JOIN collab_participants p ON p.id = rm.participant_id
                   LEFT JOIN collab_agents a ON a.id = p.id
                  WHERE rm.room_id = $1
                  ORDER BY p.created_at, p.id",
            )
            .bind(&room.id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(RoomMemberRow::into_member)
            .collect();
            summaries.push(RoomSummary {
                id: room.id,
                kind: room.kind,
                title: room.title,
                next_sequence: room.next_seq,
                last_read_sequence: room.last_read_seq,
                unread_count: unread_count(room.last_read_seq, room.next_seq),
                muted: room.muted,
                members,
            });
        }
        Ok(summaries)
    }

    /// Advances only the human user's persisted read cursor.
    ///
    /// Agent delivery uses its own unread cursor and must never call this method.
    pub async fn mark_user_read(
        &self,
        room_id: &str,
        through_sequence: i64,
    ) -> Result<i64, StorageError> {
        if through_sequence < 0 {
            return Err(StorageError::InvalidInput(
                "read sequence must not be negative".to_string(),
            ));
        }
        sqlx::query_scalar::<_, i64>(
            "UPDATE collab_room_members rm
                SET last_read_seq = GREATEST(
                    rm.last_read_seq,
                    LEAST($2, (SELECT next_seq FROM collab_rooms WHERE id = $1))
                )
              WHERE rm.room_id = $1 AND rm.participant_id = 'user'
              RETURNING rm.last_read_seq",
        )
        .bind(room_id)
        .bind(through_sequence)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("user membership in room {room_id}")))
    }

    pub async fn add_member(
        &self,
        room_id: &str,
        participant_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO collab_room_members (room_id, participant_id)
             VALUES ($1, $2)
             ON CONFLICT (room_id, participant_id) DO NOTHING",
        )
        .bind(room_id)
        .bind(participant_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn is_member(
        &self,
        room_id: &str,
        participant_id: &str,
    ) -> Result<bool, StorageError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM collab_room_members
                 WHERE room_id = $1 AND participant_id = $2
             )",
        )
        .bind(room_id)
        .bind(participant_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn send_message(
        &self,
        room_id: &str,
        author_id: &str,
        body: &str,
    ) -> Result<SendMessageOutcome, StorageError> {
        if body.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "message body must not be blank".to_string(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let sequence = sqlx::query_scalar::<_, i64>(
            "UPDATE collab_rooms
                SET next_seq = next_seq + 1,
                    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
              WHERE id = $1 AND next_seq < 9223372036854775807
              RETURNING next_seq",
        )
        .bind(room_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| StorageError::NotFoundOrExhausted(format!("room {room_id}")))?;

        let dedup_window_ms = i64::try_from(MESSAGE_DEDUP_WINDOW.as_millis())
            .map_err(|_| StorageError::InvalidInput("dedup window is too large".to_string()))?;
        if let Some(record) = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, kind, body, created_at
               FROM collab_messages
              WHERE room_id = $1 AND author_id = $2 AND body = $3
                AND created_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                    - ($4::BIGINT * INTERVAL '1 millisecond')
              ORDER BY sequence DESC
              LIMIT 1",
        )
        .bind(room_id)
        .bind(author_id)
        .bind(body)
        .bind(dedup_window_ms)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.rollback().await?;
            return Ok(SendMessageOutcome {
                message: record.try_into_message()?,
                deduplicated: true,
            });
        }

        let id = format!("msg_{}", Uuid::new_v4().simple());
        let created_at = china_now();
        let record = sqlx::query_as::<_, MessageRow>(
            "INSERT INTO collab_messages (
                id, room_id, sequence, author_id, kind, body, created_at
             ) VALUES ($1, $2, $3, $4, 'normal', $5, $6)
             RETURNING id, room_id, sequence, author_id, kind, body, created_at",
        )
        .bind(&id)
        .bind(room_id)
        .bind(sequence)
        .bind(author_id)
        .bind(body)
        .bind(created_at)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE collab_rooms
                SET last_message_at = $2, updated_at = $2
              WHERE id = $1",
        )
        .bind(room_id)
        .bind(created_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(SendMessageOutcome {
            message: record.try_into_message()?,
            deduplicated: false,
        })
    }

    pub async fn room_messages(&self, room_id: &str) -> Result<Vec<Message>, StorageError> {
        records_to_messages(
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, room_id, sequence, author_id, kind, body, created_at
                   FROM collab_messages
                  WHERE room_id = $1
                  ORDER BY sequence",
            )
            .bind(room_id)
            .fetch_all(&self.pool)
            .await?,
        )
    }

    pub async fn message_page(
        &self,
        room_id: &str,
        query: MessagePageQuery,
    ) -> Result<MessagePage, StorageError> {
        if !(1..=100).contains(&query.limit) {
            return Err(StorageError::InvalidInput(
                "message page limit must be between 1 and 100".to_string(),
            ));
        }
        let limit = i64::from(query.limit);
        let records = match query.anchor {
            MessagePageAnchor::Around(sequence) => {
                sqlx::query_as::<_, MessageRow>(
                    "SELECT id, room_id, sequence, author_id, kind, body, created_at
                       FROM (
                            SELECT id, room_id, sequence, author_id, kind, body, created_at
                              FROM collab_messages
                             WHERE room_id = $1
                             ORDER BY ABS(sequence - $2), sequence
                             LIMIT $3
                       ) nearest
                      ORDER BY sequence",
                )
                .bind(room_id)
                .bind(sequence)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            MessagePageAnchor::Before(sequence) => {
                sqlx::query_as::<_, MessageRow>(
                    "SELECT id, room_id, sequence, author_id, kind, body, created_at
                       FROM (
                            SELECT id, room_id, sequence, author_id, kind, body, created_at
                              FROM collab_messages
                             WHERE room_id = $1 AND sequence < $2
                             ORDER BY sequence DESC
                             LIMIT $3
                       ) previous
                      ORDER BY sequence",
                )
                .bind(room_id)
                .bind(sequence)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            MessagePageAnchor::After(sequence) => {
                sqlx::query_as::<_, MessageRow>(
                    "SELECT id, room_id, sequence, author_id, kind, body, created_at
                       FROM collab_messages
                      WHERE room_id = $1 AND sequence > $2
                      ORDER BY sequence
                      LIMIT $3",
                )
                .bind(room_id)
                .bind(sequence)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };
        let room = self
            .room(room_id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("room {room_id}")))?;
        let messages = records_to_messages(records)?;
        let has_older = messages.first().is_some_and(|message| message.sequence > 1);
        let has_newer = messages
            .last()
            .is_some_and(|message| message.sequence < room.next_sequence);
        Ok(MessagePage {
            messages,
            has_older,
            has_newer,
        })
    }

    pub async fn inbox(&self, participant_id: &str) -> Result<Inbox, StorageError> {
        let records = sqlx::query_as::<_, MessageRow>(
            "SELECT m.id, m.room_id, m.sequence, m.author_id, m.kind, m.body, m.created_at
               FROM collab_room_members rm
               JOIN collab_messages m ON m.room_id = rm.room_id
              WHERE rm.participant_id = $1
                AND m.sequence > rm.last_read_seq
              ORDER BY m.created_at, m.room_id, m.sequence",
        )
        .bind(participant_id)
        .fetch_all(&self.pool)
        .await?;
        let messages = records_to_messages(records)?;
        let unread = sqlx::query_as::<_, UnreadRow>(
            "SELECT rm.last_read_seq, r.next_seq
               FROM collab_room_members rm
               JOIN collab_rooms r ON r.id = rm.room_id
              WHERE rm.participant_id = $1",
        )
        .bind(participant_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| unread_count(row.last_read_seq, row.next_seq))
        .sum();
        Ok(Inbox {
            messages,
            unread_count: unread,
        })
    }

    pub async fn mentioned_agents(
        &self,
        room_id: &str,
        body: &str,
    ) -> Result<Vec<Agent>, StorageError> {
        let agents = sqlx::query_as::<_, AgentRow>(
            "SELECT a.id, p.display_name, a.role, a.bio, a.system_prompt,
                    a.provider_id, a.model_id, a.opencode_session_id, a.enabled
               FROM collab_room_members rm
               JOIN collab_agents a ON a.id = rm.participant_id
               JOIN collab_participants p ON p.id = a.id
              WHERE rm.room_id = $1 AND a.enabled = TRUE AND rm.muted = FALSE
              ORDER BY a.id",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(agents
            .into_iter()
            .map(AgentRow::into_agent)
            .filter(|agent| contains_mention(body, &agent.id))
            .collect())
    }

    pub async fn begin_run(
        &self,
        agent: &Agent,
        room_id: Option<&str>,
        trigger: &str,
    ) -> Result<String, StorageError> {
        let id = format!("run_{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO collab_runs (
                id, agent_id, room_id, trigger, status, provider_id, model_id
             ) VALUES ($1, $2, $3, $4, 'running', $5, $6)",
        )
        .bind(&id)
        .bind(&agent.id)
        .bind(room_id)
        .bind(trigger)
        .bind(&agent.provider_id)
        .bind(&agent.model_id)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        error: Option<(&str, &str)>,
    ) -> Result<(), StorageError> {
        if !matches!(status, "completed" | "failed" | "cancelled" | "interrupted") {
            return Err(StorageError::InvalidInput(format!(
                "invalid terminal run status: {status}"
            )));
        }
        let (error_code, error_message) = error.unzip();
        sqlx::query(
            "UPDATE collab_runs
                SET status = $2,
                    ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                    error_code = $3,
                    error_message = $4
              WHERE id = $1 AND status = 'running'",
        )
        .bind(run_id)
        .bind(status)
        .bind(error_code)
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn validate_agent(input: &AgentInput) -> Result<(), StorageError> {
    for (field, value) in [
        ("id", input.id.as_str()),
        ("display_name", input.display_name.as_str()),
        ("system_prompt", input.system_prompt.as_str()),
        ("provider_id", input.provider_id.as_str()),
        ("model_id", input.model_id.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(StorageError::InvalidInput(format!(
                "agent {field} must not be blank"
            )));
        }
    }
    Ok(())
}

fn contains_mention(body: &str, agent_id: &str) -> bool {
    let needle = format!("@{agent_id}");
    body.match_indices(&needle).any(|(start, matched)| {
        let end = start + matched.len();
        body[end..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_ascii_alphanumeric() || next == '_'))
    })
}

fn records_to_messages(records: Vec<MessageRow>) -> Result<Vec<Message>, StorageError> {
    records
        .into_iter()
        .map(MessageRow::try_into_message)
        .collect()
}

#[derive(Debug, sqlx::FromRow)]
struct AgentRow {
    id: String,
    display_name: String,
    role: Option<String>,
    bio: Option<String>,
    system_prompt: String,
    provider_id: String,
    model_id: String,
    opencode_session_id: Option<String>,
    enabled: bool,
}

impl AgentRow {
    fn into_agent(self) -> Agent {
        Agent {
            id: self.id,
            display_name: self.display_name,
            role: self.role,
            bio: self.bio,
            system_prompt: self.system_prompt,
            provider_id: self.provider_id,
            model_id: self.model_id,
            opencode_session_id: self.opencode_session_id,
            enabled: self.enabled,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct RoomRow {
    id: String,
    kind: String,
    title: Option<String>,
    next_seq: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct RoomSummaryRow {
    id: String,
    kind: String,
    title: Option<String>,
    next_seq: i64,
    last_read_seq: i64,
    muted: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct RoomMemberRow {
    id: String,
    display_name: String,
    kind: String,
    enabled: bool,
}

impl RoomMemberRow {
    fn into_member(self) -> RoomMember {
        RoomMember {
            id: self.id,
            display_name: self.display_name,
            kind: self.kind,
            enabled: self.enabled,
        }
    }
}

impl RoomRow {
    fn into_room(self) -> Room {
        Room {
            id: self.id,
            kind: self.kind,
            title: self.title,
            next_sequence: self.next_seq,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct MessageRow {
    id: String,
    room_id: String,
    sequence: i64,
    author_id: String,
    kind: String,
    body: String,
    created_at: PrimitiveDateTime,
}

impl MessageRow {
    fn try_into_message(self) -> Result<Message, StorageError> {
        Ok(Message {
            id: self.id,
            room_id: self.room_id,
            sequence: self.sequence,
            author_id: self.author_id,
            kind: self.kind,
            body: self.body,
            created_at: format_china(self.created_at)?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct UnreadRow {
    last_read_seq: i64,
    next_seq: i64,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("collaboration database failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrationError),
    #[error("collaboration record not found: {0}")]
    NotFound(String),
    #[error("collaboration record is missing or exhausted: {0}")]
    NotFoundOrExhausted(String),
    #[error("invalid collaboration input: {0}")]
    InvalidInput(String),
    #[error("collaboration time formatting failed: {0}")]
    TimeFormat(#[from] time::error::Format),
}

#[cfg(test)]
mod tests {
    use super::contains_mention;

    #[test]
    fn mention_matching_respects_identifier_boundaries() {
        assert!(contains_mention("hello @alice", "alice"));
        assert!(contains_mention("@alice, please respond", "alice"));
        assert!(!contains_mention("@alice_2", "alice"));
        assert!(!contains_mention("alice", "alice"));
    }
}
