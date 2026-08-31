use std::collections::{BTreeMap, BTreeSet};

use crate::protocol::{DeliveryRange, InboxResponse, MessageView, TriggerEnvelope, entity_id};
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use super::{auth::AgentClaims, climate::Climate};

#[derive(Clone)]
pub(crate) struct Messages {
    pool: PgPool,
}

impl Messages {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn send_user_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        body: &str,
    ) -> Result<MessageView, sqlx::Error> {
        if body.trim().is_empty()
            || body.len() > crate::protocol::MESSAGE_BODY_MAX_BYTES
            || body.as_bytes().contains(&0)
        {
            return Err(sqlx::Error::Protocol(
                "INVALID_ARGUMENT: message body is invalid".to_string(),
            ));
        }
        let message_id = entity_id("msg");
        let sequence = insert(
            transaction,
            room_id,
            "local-user",
            "normal",
            body,
            Some(&message_id),
        )
        .await?;
        Ok(MessageView {
            id: message_id,
            room_id: room_id.to_string(),
            sequence,
            author_id: "local-user".to_string(),
            body: body.to_string(),
        })
    }

    pub(crate) async fn list(&self, room_id: &str) -> Result<Vec<MessageView>, sqlx::Error> {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body
             FROM collab_messages WHERE room_id = $1 ORDER BY sequence",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(MessageView::from).collect())
    }

    pub(crate) async fn list_for_agent_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        room_id: &str,
        tail: u32,
    ) -> Result<Vec<MessageView>, sqlx::Error> {
        let membership: Option<(time::PrimitiveDateTime, String)> = sqlx::query_as(
            "SELECT member.joined_at, room.kind
             FROM collab_room_members member
             JOIN collab_rooms room ON room.id = member.room_id
             WHERE member.room_id = $1 AND member.participant_id = $2",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some((joined_at, room_kind)) = membership else {
            return Err(sqlx::Error::Protocol(
                "NOT_FOUND: Room is not visible to this Agent".to_string(),
            ));
        };
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body FROM (
                SELECT message.id, message.room_id, message.sequence,
                       message.author_id, message.body
                FROM collab_messages message
                WHERE message.room_id = $1
                  AND ($3 = 'direct' OR message.created_at >= $2)
                ORDER BY message.sequence DESC
                LIMIT $4
             ) recent
             ORDER BY sequence",
        )
        .bind(room_id)
        .bind(joined_at)
        .bind(room_kind)
        .bind(i64::from(tail.clamp(1, 200)))
        .fetch_all(&mut **transaction)
        .await
        .map(|rows| rows.into_iter().map(MessageView::from).collect())
    }

    pub(crate) async fn wake_recipients(
        &self,
        message_id: &str,
        room_id: &str,
        author_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT rm.participant_id
             FROM collab_room_members rm
             JOIN collab_rooms r ON r.id = rm.room_id
             JOIN collab_agent_profiles a ON a.agent_id = rm.participant_id
             JOIN collab_messages message
               ON message.id = $1 AND message.room_id = rm.room_id
             WHERE rm.room_id = $2 AND rm.participant_id <> $3 AND a.archived_at IS NULL
               AND (
                   NOT rm.muted OR r.kind = 'direct' OR (
                       message.kind = 'normal'
                       AND message.body ~ (
                           '(^|[^A-Za-z0-9_-])@' || rm.participant_id ||
                           '([^A-Za-z0-9_-]|$)'
                       )
                   )
               )
             ORDER BY rm.participant_id",
        )
        .bind(message_id)
        .bind(room_id)
        .bind(author_id)
        .fetch_all(&self.pool)
        .await
    }

    pub(crate) async fn inbox(&self, claims: &AgentClaims) -> Result<InboxResponse, sqlx::Error> {
        let rows = sqlx::query_as::<_, InboxRow>(
            "SELECT m.id, m.room_id, m.sequence, m.author_id, m.body, rm.last_read_seq,
                    COUNT(*) OVER() AS total_count
             FROM collab_room_members rm
             JOIN collab_rooms r ON r.id = rm.room_id
             JOIN collab_messages m ON m.room_id = rm.room_id
             WHERE rm.participant_id = $1
               AND (
                   NOT rm.muted OR r.kind = 'direct' OR EXISTS (
                       SELECT 1
                       FROM collab_messages mention
                       WHERE mention.room_id = rm.room_id
                         AND mention.sequence > rm.last_read_seq
                         AND mention.author_id <> $1
                         AND mention.kind = 'normal'
                         AND mention.body ~ (
                             '(^|[^A-Za-z0-9_-])@' || rm.participant_id ||
                             '([^A-Za-z0-9_-]|$)'
                         )
                   )
               )
               AND m.sequence > rm.last_read_seq AND m.author_id <> $1
             ORDER BY m.created_at, m.room_id, m.sequence
             LIMIT 200",
        )
        .bind(&claims.sub)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(InboxResponse {
                trigger: None,
                messages: Vec::new(),
                climates: Vec::new(),
                carried_over: false,
            });
        }
        let carried_over = rows
            .first()
            .is_some_and(|row| row.total_count > rows.len() as i64);
        let mut ranges = BTreeMap::<String, (i64, i64)>::new();
        let mut participant_ids = BTreeSet::<String>::new();
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            ranges
                .entry(row.room_id.clone())
                .and_modify(|range| range.1 = row.sequence)
                .or_insert((row.last_read_seq + 1, row.sequence));
            participant_ids.insert(row.author_id.clone());
            messages.push(MessageView {
                id: row.id,
                room_id: row.room_id,
                sequence: row.sequence,
                author_id: row.author_id,
                body: row.body,
            });
        }
        let participant_ids = participant_ids.into_iter().collect::<Vec<_>>();
        let climates = Climate::for_participants(&self.pool, &claims.sub, &participant_ids).await?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        Ok(InboxResponse {
            trigger: Some(TriggerEnvelope {
                dispatch_id: entity_id("run"),
                agent_id: claims.sub.clone(),
                runtime_session_id: claims.runtime_session_id.clone(),
                trigger: "message".to_string(),
                deliveries: ranges
                    .into_iter()
                    .map(|(room_id, (from_seq, up_to_seq))| DeliveryRange {
                        room_id,
                        from_seq,
                        up_to_seq,
                    })
                    .collect(),
                agenda_focus: None,
                carried_over,
                issued_at: now,
                expires_at: now + 5 * 60,
                signature: String::new(),
            }),
            messages,
            climates,
            carried_over,
        })
    }
}

async fn insert(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    author_id: &str,
    kind: &str,
    body: &str,
    message_id: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let sequence: i64 = sqlx::query_scalar(
        "UPDATE collab_rooms
         SET next_seq = next_seq + 1,
             last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1 RETURNING next_seq",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?;
    let generated;
    let message_id = match message_id {
        Some(message_id) => message_id,
        None => {
            generated = entity_id("msg");
            &generated
        }
    };
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(message_id)
    .bind(room_id)
    .bind(sequence)
    .bind(author_id)
    .bind(kind)
    .bind(body)
    .execute(&mut **transaction)
    .await?;
    Ok(sequence)
}

#[derive(FromRow)]
struct MessageRow {
    id: String,
    room_id: String,
    sequence: i64,
    author_id: String,
    body: String,
}

impl From<MessageRow> for MessageView {
    fn from(row: MessageRow) -> Self {
        Self {
            id: row.id,
            room_id: row.room_id,
            sequence: row.sequence,
            author_id: row.author_id,
            body: row.body,
        }
    }
}

#[derive(FromRow)]
struct InboxRow {
    id: String,
    room_id: String,
    sequence: i64,
    author_id: String,
    body: String,
    last_read_seq: i64,
    total_count: i64,
}
