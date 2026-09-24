use std::collections::{BTreeMap, BTreeSet};

use crate::protocol::{
    DeliveryRange, InboxResponse, MessageView, QuotedMessageView, TriggerEnvelope, entity_id,
};
use sqlx::{FromRow, PgExecutor, PgPool, Postgres, Transaction};

use super::{agents::Agents, auth::AgentClaims, climate::Climate, rooms::Rooms};

const INBOX_MESSAGE_LIMIT: usize = 200;
/// Characters of a quoted message shown under a reply (collaboration.md §9.3, Cumora `cli.ts` inbox).
const QUOTE_BODY_MAX_CHARS: i32 = 180;
/// Characters of the peer message shown back in a DUPLICATE rejection (collaboration.md §9.2, Cumora `cli.ts`).
const DUPLICATE_PEER_MAX_CHARS: i32 = 200;

#[derive(Clone)]
pub(crate) struct Messages {
    pool: PgPool,
}

impl Messages {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 用户发消息；`quoted_message_id` 不是本房间的消息时返回 `NOT_FOUND`。
    pub(crate) async fn send_user_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        body: &str,
        quoted_message_id: Option<&str>,
    ) -> Result<MessageView, sqlx::Error> {
        if !Self::valid_body(body) {
            return Err(sqlx::Error::Protocol(
                "INVALID_ARGUMENT: message body is invalid".to_string(),
            ));
        }
        let quoted = match quoted_message_id {
            Some(quoted_id) => Some(
                Self::quote_in(transaction, room_id, quoted_id)
                    .await?
                    .ok_or_else(|| {
                        sqlx::Error::Protocol(format!(
                            "NOT_FOUND: {}",
                            Self::quote_not_found(quoted_id, room_id)
                        ))
                    })?,
            ),
            None => None,
        };
        Self::insert_in(transaction, room_id, "local-user", body, quoted).await
    }

    /// 本房间里 `quoted_id` 这条消息的引用摘要；不在本房间时返回 `None`。
    pub(crate) async fn quote_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        quoted_id: &str,
    ) -> Result<Option<QuotedMessageView>, sqlx::Error> {
        sqlx::query_as::<_, QuoteRow>(
            "SELECT message.id, message.author_id, author.display_name AS author_name,
                    LEFT(message.body, $3) AS body
             FROM collab_messages message
             JOIN collab_participants author ON author.id = message.author_id
             WHERE message.room_id = $1 AND message.id = $2",
        )
        .bind(room_id)
        .bind(quoted_id)
        .bind(QUOTE_BODY_MAX_CHARS)
        .fetch_optional(&mut **transaction)
        .await
        .map(|row| row.map(QuotedMessageView::from))
    }

    /// 逐字重复拦截（collaboration.md §9.2，Cumora `cli.ts` 的 VERBATIM-DUP）：先锁住房间行，
    /// 再比较 `body` 与本房间最近一条别人发的 normal 消息（两边都去掉首尾空白）。相同时返回
    /// 给模型的拒绝说明。锁保持到事务结束，所以随后的插入不会被并发的同一内容抢先。
    pub(crate) async fn duplicate_of_last_peer_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        author_id: &str,
        body: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query("SELECT id FROM collab_rooms WHERE id = $1 FOR UPDATE")
            .bind(room_id)
            .execute(&mut **transaction)
            .await?;
        let last_peer: Option<(String, String, String)> = sqlx::query_as(
            "SELECT message.body, LEFT(message.body, $3), author.display_name
             FROM collab_messages message
             JOIN collab_participants author ON author.id = message.author_id
             WHERE message.room_id = $1 AND message.author_id <> $2 AND message.kind = 'normal'
             ORDER BY message.sequence DESC
             LIMIT 1",
        )
        .bind(room_id)
        .bind(author_id)
        .bind(DUPLICATE_PEER_MAX_CHARS)
        .fetch_optional(&mut **transaction)
        .await?;
        Ok(last_peer
            .filter(|(peer_body, _, _)| peer_body.trim() == body.trim())
            .map(|(_, shown, name)| {
                let shown = shown.split_whitespace().collect::<Vec<_>>().join(" ");
                format!(
                    "your message is identical to the latest message from {name} in {room_id}: \"{shown}\". {name} already said it; pick a different angle, the next item in a sequence, or stay silent."
                )
            }))
    }

    /// 引用目标不在本房间时给模型与用户的说明。
    pub(crate) fn quote_not_found(quoted_id: &str, room_id: &str) -> String {
        format!("{quoted_id} is not a message in {room_id}; quote an id from this room's messages")
    }

    /// 把查询行转成 `MessageView`，并用一次查询补齐被引用消息的摘要。
    pub(crate) async fn views<'e, E>(
        executor: E,
        rows: Vec<MessageRow>,
    ) -> Result<Vec<MessageView>, sqlx::Error>
    where
        E: PgExecutor<'e>,
    {
        let quoted_ids = rows
            .iter()
            .filter_map(|row| row.quoted_message_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let quotes = if quoted_ids.is_empty() {
            BTreeMap::new()
        } else {
            sqlx::query_as::<_, QuoteRow>(
                "SELECT message.id, message.author_id, author.display_name AS author_name,
                        LEFT(message.body, $2) AS body
                 FROM collab_messages message
                 JOIN collab_participants author ON author.id = message.author_id
                 WHERE message.id = ANY($1)",
            )
            .bind(&quoted_ids)
            .bind(QUOTE_BODY_MAX_CHARS)
            .fetch_all(executor)
            .await?
            .into_iter()
            .map(|row| (row.id.clone(), QuotedMessageView::from(row)))
            .collect::<BTreeMap<_, _>>()
        };
        Ok(rows
            .into_iter()
            .map(|row| MessageView {
                quoted: row
                    .quoted_message_id
                    .as_ref()
                    .and_then(|id| quotes.get(id).cloned()),
                id: row.id,
                room_id: row.room_id,
                sequence: row.sequence,
                author_id: row.author_id,
                body: row.body,
            })
            .collect())
    }

    pub(crate) fn valid_body(body: &str) -> bool {
        !body.trim().is_empty()
            && body.len() <= crate::protocol::MESSAGE_BODY_MAX_BYTES
            && !body.as_bytes().contains(&0)
    }

    /// 插入一条 normal 消息；`quoted` 必须已由 [`Self::quote_in`] 在同一房间确认。
    pub(crate) async fn insert_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        author_id: &str,
        body: &str,
        quoted: Option<QuotedMessageView>,
    ) -> Result<MessageView, sqlx::Error> {
        let id = entity_id("msg");
        let sequence = insert(
            transaction,
            &NewMessage {
                id: &id,
                room_id,
                author_id,
                kind: "normal",
                body,
                quoted_message_id: quoted.as_ref().map(|quoted| quoted.id.as_str()),
            },
        )
        .await?;
        Ok(MessageView {
            id,
            room_id: room_id.to_string(),
            sequence,
            author_id: author_id.to_string(),
            body: body.to_string(),
            quoted,
        })
    }

    pub(crate) async fn glance_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        compose_anchor: i64,
        agent_id: &str,
    ) -> Result<Vec<MessageView>, sqlx::Error> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body, quoted_message_id FROM (
                SELECT id, room_id, sequence, author_id, body, quoted_message_id
                FROM collab_messages
                WHERE room_id = $1 AND sequence > $2 AND author_id <> $3
                ORDER BY sequence DESC LIMIT 50
             ) recent ORDER BY sequence",
        )
        .bind(room_id)
        .bind(compose_anchor)
        .bind(agent_id)
        .fetch_all(&mut **transaction)
        .await?;
        Self::views(&mut **transaction, rows).await
    }

    pub(crate) async fn between_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        after_sequence: i64,
        up_to_sequence: i64,
        agent_id: &str,
    ) -> Result<Vec<MessageView>, sqlx::Error> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body, quoted_message_id
             FROM collab_messages
             WHERE room_id = $1 AND sequence > $2 AND sequence <= $3
               AND author_id <> $4
             ORDER BY sequence LIMIT 50",
        )
        .bind(room_id)
        .bind(after_sequence)
        .bind(up_to_sequence)
        .bind(agent_id)
        .fetch_all(&mut **transaction)
        .await?;
        Self::views(&mut **transaction, rows).await
    }

    pub(crate) async fn peer_max_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        agent_id: &str,
        seen_baseline: i64,
    ) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT MAX(sequence) FROM collab_messages
             WHERE room_id = $1 AND sequence > $2 AND author_id <> $3",
        )
        .bind(room_id)
        .bind(seen_baseline)
        .bind(agent_id)
        .fetch_one(&mut **transaction)
        .await
    }

    pub(crate) async fn agent_loop_capped_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        hard_cap: i64,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT COUNT(*) >= $2
             FROM collab_messages message
             JOIN collab_participants author ON author.id = message.author_id
             WHERE message.room_id = $1
               AND message.sequence > GREATEST(COALESCE((
                   SELECT MAX(previous.sequence)
                   FROM collab_messages previous
                   JOIN collab_participants previous_author
                     ON previous_author.id = previous.author_id
                   WHERE previous.room_id = $1 AND previous_author.kind <> 'agent'
               ), 0), (SELECT user_viewed_seq FROM collab_rooms WHERE id = $1))
               AND author.kind = 'agent' AND message.kind <> 'system'",
        )
        .bind(room_id)
        .bind(hard_cap)
        .fetch_one(&mut **transaction)
        .await
    }

    pub(crate) async fn list(&self, room_id: &str) -> Result<Vec<MessageView>, sqlx::Error> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body, quoted_message_id
             FROM collab_messages WHERE room_id = $1 ORDER BY sequence",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await?;
        Self::views(&self.pool, rows).await
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
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body, quoted_message_id FROM (
                SELECT message.id, message.room_id, message.sequence,
                       message.author_id, message.body, message.quoted_message_id
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
        .await?;
        Self::views(&mut **transaction, rows).await
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
                   ) OR EXISTS (
                       SELECT 1 FROM collab_messages quoted
                       WHERE quoted.room_id = message.room_id
                         AND quoted.id = message.quoted_message_id
                         AND quoted.author_id = rm.participant_id
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
        let unread_rooms = sqlx::query_as::<_, UnreadRoomRow>(
            "WITH unread AS (
                SELECT m.room_id, rm.last_read_seq, COUNT(*) AS unread_count,
                       MIN(m.created_at) AS oldest_at
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
                      ) OR EXISTS (
                          SELECT 1
                          FROM collab_messages reply
                          JOIN collab_messages quoted
                            ON quoted.room_id = reply.room_id AND quoted.id = reply.quoted_message_id
                          WHERE reply.room_id = rm.room_id
                            AND reply.sequence > rm.last_read_seq
                            AND reply.author_id <> $1
                            AND quoted.author_id = rm.participant_id
                      )
                  )
                  AND m.sequence > rm.last_read_seq AND m.author_id <> $1
                GROUP BY m.room_id, rm.last_read_seq
             ), counted AS (
                SELECT room_id, last_read_seq, unread_count, oldest_at,
                       SUM(unread_count) OVER()::BIGINT AS total_count,
                       COUNT(*) OVER() AS total_rooms
                FROM unread
             )
             SELECT room_id, last_read_seq, unread_count, total_count, total_rooms
             FROM counted
             ORDER BY oldest_at, room_id
             LIMIT $2",
        )
        .bind(&claims.sub)
        .bind(INBOX_MESSAGE_LIMIT as i64)
        .fetch_all(&self.pool)
        .await?;
        if unread_rooms.is_empty() {
            return Ok(InboxResponse {
                trigger: None,
                messages: Vec::new(),
                climates: Vec::new(),
                carried_over: false,
                rooms: Vec::new(),
                team: Agents::team(&self.pool, &[]).await?,
            });
        }
        let allocations = water_fill(
            &unread_rooms
                .iter()
                .map(|room| room.unread_count as usize)
                .collect::<Vec<_>>(),
            INBOX_MESSAGE_LIMIT,
        );
        let room_ids = unread_rooms
            .iter()
            .map(|room| room.room_id.clone())
            .collect::<Vec<_>>();
        let allocations = allocations
            .into_iter()
            .map(|allocation| allocation as i64)
            .collect::<Vec<_>>();
        let rows = sqlx::query_as::<_, InboxRow>(
            "WITH allocation AS (
                SELECT *
                FROM UNNEST($2::TEXT[], $3::BIGINT[]) AS selected(room_id, take_count)
             ), ranked AS (
                SELECT m.id, m.room_id, m.sequence, m.author_id, m.body, m.quoted_message_id,
                       rm.last_read_seq, m.created_at, allocation.take_count,
                       ROW_NUMBER() OVER (
                           PARTITION BY m.room_id ORDER BY m.sequence
                       ) AS room_position
                FROM allocation
                JOIN collab_room_members rm ON rm.room_id = allocation.room_id
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
                      ) OR EXISTS (
                          SELECT 1
                          FROM collab_messages reply
                          JOIN collab_messages quoted
                            ON quoted.room_id = reply.room_id AND quoted.id = reply.quoted_message_id
                          WHERE reply.room_id = rm.room_id
                            AND reply.sequence > rm.last_read_seq
                            AND reply.author_id <> $1
                            AND quoted.author_id = rm.participant_id
                      )
                  )
                  AND m.sequence > rm.last_read_seq AND m.author_id <> $1
             )
             SELECT id, room_id, sequence, author_id, body, quoted_message_id, last_read_seq
             FROM ranked
             WHERE room_position <= take_count
             ORDER BY created_at, room_id, sequence",
        )
        .bind(&claims.sub)
        .bind(&room_ids)
        .bind(&allocations)
        .fetch_all(&self.pool)
        .await?;
        let total_count = unread_rooms[0].total_count;
        let total_rooms = unread_rooms[0].total_rooms;
        let carried_over =
            total_count > rows.len() as i64 || total_rooms > unread_rooms.len() as i64;
        let mut ranges = BTreeMap::<String, (i64, i64)>::new();
        let mut participant_ids = BTreeSet::<String>::new();
        let mut message_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let message = row.message;
            ranges
                .entry(message.room_id.clone())
                .and_modify(|range| range.1 = message.sequence)
                .or_insert((row.last_read_seq + 1, message.sequence));
            participant_ids.insert(message.author_id.clone());
            message_rows.push(message);
        }
        let messages = Self::views(&self.pool, message_rows).await?;
        let participant_ids = participant_ids.into_iter().collect::<Vec<_>>();
        let climates = Climate::for_participants(&self.pool, &claims.sub, &participant_ids).await?;
        let team = Agents::team(&self.pool, &participant_ids).await?;
        let rooms = Rooms::views(&self.pool, &ranges.keys().cloned().collect::<Vec<_>>()).await?;
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
            rooms,
            team,
        })
    }
}

/// 一条待插入的消息。
struct NewMessage<'a> {
    id: &'a str,
    room_id: &'a str,
    author_id: &'a str,
    kind: &'a str,
    body: &'a str,
    quoted_message_id: Option<&'a str>,
}

async fn insert(
    transaction: &mut Transaction<'_, Postgres>,
    message: &NewMessage<'_>,
) -> Result<i64, sqlx::Error> {
    let sequence: i64 = sqlx::query_scalar(
        "UPDATE collab_rooms
         SET next_seq = next_seq + 1,
             last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1 RETURNING next_seq",
    )
    .bind(message.room_id)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body, quoted_message_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(message.id)
    .bind(message.room_id)
    .bind(sequence)
    .bind(message.author_id)
    .bind(message.kind)
    .bind(message.body)
    .bind(message.quoted_message_id)
    .execute(&mut **transaction)
    .await?;
    Ok(sequence)
}

/// 读取消息的公共列；经 [`Messages::views`] 补齐引用摘要后才成为 `MessageView`。
#[derive(FromRow)]
pub(crate) struct MessageRow {
    pub(crate) id: String,
    pub(crate) room_id: String,
    pub(crate) sequence: i64,
    pub(crate) author_id: String,
    pub(crate) body: String,
    pub(crate) quoted_message_id: Option<String>,
}

#[derive(FromRow)]
struct QuoteRow {
    id: String,
    author_id: String,
    author_name: String,
    body: String,
}

impl From<QuoteRow> for QuotedMessageView {
    fn from(row: QuoteRow) -> Self {
        Self {
            id: row.id,
            author_id: row.author_id,
            author_name: row.author_name,
            body: row.body,
        }
    }
}

#[derive(FromRow)]
struct InboxRow {
    #[sqlx(flatten)]
    message: MessageRow,
    last_read_seq: i64,
}

#[derive(FromRow)]
struct UnreadRoomRow {
    room_id: String,
    unread_count: i64,
    total_count: i64,
    total_rooms: i64,
}

fn water_fill(counts: &[usize], budget: usize) -> Vec<usize> {
    let mut order = (0..counts.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| (counts[*index], *index));
    let mut allocations = vec![0; counts.len()];
    let mut remaining = budget;
    let mut unserved = counts.len();
    for index in order {
        let fair_share = remaining / unserved;
        let allocated = counts[index].min(fair_share);
        allocations[index] = allocated;
        remaining -= allocated;
        unserved -= 1;
    }
    allocations
}

#[cfg(test)]
mod tests {
    use super::water_fill;

    #[test]
    fn inbox_budget_water_fills_quiet_rooms_before_busy_rooms() {
        assert_eq!(water_fill(&[200, 1], 200), vec![199, 1]);
        assert_eq!(water_fill(&[100, 2, 3], 10), vec![5, 2, 3]);
        assert_eq!(water_fill(&[2, 3], 10), vec![2, 3]);
    }
}
